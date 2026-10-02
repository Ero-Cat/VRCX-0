//! PostgreSQL implementation of the [`RemoteSyncStore`] port.
//!
//! PostgreSQL is the sync hub and single serialization point: every pushed
//! op is appended to `sync_ops` and materialized into business tables inside
//! one transaction, so concurrent pushes from several clients are ordered by
//! row locks and merged by per-field lattice expressions (`CASE` for LWW,
//! boolean OR, `GREATEST`, counter deltas, per-element JSON merges). Pull is
//! cursor-based over `server_seq`, the arrival order, which every replica
//! observes identically — that is what makes order-sensitive merges (element
//! sets) converge.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Map, Value};
use tokio::sync::Mutex as AsyncMutex;
use tokio_postgres::types::ToSql;
use tokio_postgres::Client;
use vrcx_0_application_sync::{
    MaterializedRow, PulledOp, RemoteColumnType, RemoteSyncStore, RemoteTableSchema,
    SyncStoreError, SyncStoreResult,
};
use vrcx_0_contracts::{
    SyncConnectionTestResult, SyncDeviceRecord, SyncFieldSemantic, SyncOpKind, SyncOpRecord,
    SyncRowSemantic,
};

const META_PROTOCOL_VERSION: &str = "sync.protocolVersion";
const META_PROVISIONED_AT: &str = "sync.provisionedAt";

/// Connection settings for the remote sync database.
#[derive(Clone, Debug)]
pub struct PostgresSyncStoreConfig {
    /// `postgres://user:pass@host:5432/db`; TLS is enforced unless plaintext
    /// was explicitly allowed for this trusted-LAN server.
    pub dsn: String,
    /// When false, certificate verification is skipped (self-signed homelab
    /// servers). Encryption still applies.
    pub tls_verify: bool,
    /// Explicit user opt-in for servers without TLS (trusted LAN only);
    /// downgrades the enforced `require` to `prefer`.
    pub allow_plaintext: bool,
}

pub struct PostgresSyncStore {
    config: tokio_postgres::Config,
    tls_verify: bool,
    client: AsyncMutex<Option<Client>>,
    schemas: std::sync::RwLock<HashMap<String, Arc<RemoteTableSchema>>>,
    /// Protocol tables are ensured once per connection; repeating the DDL in
    /// every push takes lock-manager contention straight into deadlocks with
    /// other concurrently pushing clients.
    protocol_ensured: std::sync::atomic::AtomicBool,
}

impl PostgresSyncStore {
    pub fn new(settings: &PostgresSyncStoreConfig) -> SyncStoreResult<Self> {
        let mut config = settings
            .dsn
            .parse::<tokio_postgres::Config>()
            .map_err(|error| {
                SyncStoreError::Other(format!("Invalid PostgreSQL connection string: {error}"))
            })?;
        // Never talk plaintext unless the user explicitly allowed it for a
        // trusted-LAN server without TLS.
        let floor = if settings.allow_plaintext {
            tokio_postgres::config::SslMode::Prefer
        } else {
            tokio_postgres::config::SslMode::Require
        };
        if matches!(
            config.get_ssl_mode(),
            tokio_postgres::config::SslMode::Disable | tokio_postgres::config::SslMode::Prefer
        ) {
            config.ssl_mode(floor);
        }
        Ok(Self {
            config,
            tls_verify: settings.tls_verify,
            client: AsyncMutex::new(None),
            schemas: std::sync::RwLock::new(HashMap::new()),
            protocol_ensured: std::sync::atomic::AtomicBool::new(false),
        })
    }

    async fn connect(&self) -> SyncStoreResult<Client> {
        let tls = make_tls_connector(self.tls_verify)?;
        let (client, connection) = self.config.connect(tls).await.map_err(|error| {
            SyncStoreError::Other(format!("PostgreSQL connect failed: {error}"))
        })?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::debug!(error = %error, "postgres sync connection closed");
            }
        });
        // Defense against abandoned transactions holding row locks forever
        // (they deadlock every later push): roll back any transaction left
        // open for 30s, and turn long lock waits into fast retryable errors.
        client
            .batch_execute(
                "SET idle_in_transaction_session_timeout = '30s';
                 SET lock_timeout = '15s';",
            )
            .await
            .map_err(|error| {
                SyncStoreError::Other(format!("PostgreSQL session guard setup failed: {error}"))
            })?;
        Ok(client)
    }

    /// Run one operation against the cached connection, reconnecting once if
    /// the cached handle went stale.
    async fn with_client<T, F, Fut>(&self, operation: F) -> SyncStoreResult<T>
    where
        F: FnOnce(Client) -> Fut,
        Fut: std::future::Future<Output = SyncStoreResult<(T, Client)>>,
    {
        // Protocol DDL runs once per store instance (not per reconnect):
        // CREATE INDEX takes table-level SHARE locks that deadlock against
        // concurrent pushes from other clients.
        if !self
            .protocol_ensured
            .load(std::sync::atomic::Ordering::Acquire)
        {
            let client = match self.client.lock().await.take() {
                Some(client) => client,
                None => self.connect().await?,
            };
            let mut failed = false;
            for sql in PROTOCOL_TABLE_DDL {
                if let Err(error) = client.execute(sql, &[]).await {
                    tracing::warn!(error = %error, "protocol DDL failed; will retry later");
                    failed = true;
                    break;
                }
            }
            if !failed {
                self.protocol_ensured
                    .store(true, std::sync::atomic::Ordering::Release);
            }
            *self.client.lock().await = Some(client);
        }
        let mut guard = self.client.lock().await;
        if guard.is_none() {
            *guard = Some(self.connect().await?);
        }
        let client = guard.take().expect("connection checked above");
        match operation(client).await {
            Ok((value, client)) => {
                *guard = Some(client);
                Ok(value)
            }
            // The (possibly broken) handle stays dropped; the next call reconnects.
            Err(error) => Err(error),
        }
    }

    fn remember_schemas(&self, tables: &[RemoteTableSchema]) {
        let mut schemas = self.schemas.write().unwrap();
        schemas.clear();
        for table in tables {
            schemas.insert(table.table.clone(), Arc::new(table.clone()));
        }
    }

    fn schema_for(&self, table: &str) -> SyncStoreResult<Arc<RemoteTableSchema>> {
        self.schemas
            .read()
            .unwrap()
            .get(table)
            .cloned()
            .ok_or_else(|| {
                SyncStoreError::Other(format!(
                    "Remote schema for table {table} has not been ensured yet."
                ))
            })
    }
}

fn make_tls_connector(verify: bool) -> SyncStoreResult<tokio_postgres_rustls::MakeRustlsConnect> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|error| SyncStoreError::Other(format!("TLS setup failed: {error}")))?;
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = if verify {
        builder.with_root_certificates(roots).with_no_client_auth()
    } else {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerifyVerifier { provider }))
            .with_no_client_auth()
    };
    Ok(tokio_postgres_rustls::MakeRustlsConnect::new(config))
}

/// Skip-name-check verifier for self-signed servers. Encryption is unchanged;
/// only identity verification is disabled, matching `sslmode=require`
/// semantics from libpq.
#[derive(Debug)]
struct NoVerifyVerifier {
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl rustls::client::danger::ServerCertVerifier for NoVerifyVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls_pki_types::CertificateDer<'_>,
        _intermediates: &[rustls_pki_types::CertificateDer<'_>],
        _server_name: &rustls_pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls_pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls_pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls_pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

// ───────────────────────────── SQL construction ─────────────────────────────

fn quoted(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn pg_type(column_type: RemoteColumnType) -> &'static str {
    match column_type {
        RemoteColumnType::Text => "TEXT",
        RemoteColumnType::BigInt => "BIGINT",
        RemoteColumnType::Double => "DOUBLE PRECISION",
        RemoteColumnType::Jsonb => "JSONB",
    }
}

fn field_semantic<'a>(
    schema: &'a RemoteTableSchema,
    column: &str,
) -> Option<&'a SyncFieldSemantic> {
    schema
        .field_semantics
        .iter()
        .find(|(field, _)| field == column)
        .map(|(_, semantic)| semantic)
}

fn create_table_sql(schema: &RemoteTableSchema) -> String {
    let mut columns = schema
        .columns
        .iter()
        .map(|column| {
            let is_counter = schema.field_semantics.iter().any(|(field, semantic)| {
                field == &column.name && matches!(semantic, SyncFieldSemantic::CounterDelta)
            });
            let default = if is_counter && column.column_type == RemoteColumnType::BigInt {
                " NOT NULL DEFAULT 0"
            } else {
                ""
            };
            format!(
                "{} {}{default}",
                quoted(&column.name),
                pg_type(column.column_type)
            )
        })
        .collect::<Vec<_>>();
    columns.push("sync_hlc TEXT NOT NULL DEFAULT ''".to_string());
    columns.push("sync_device TEXT NOT NULL DEFAULT ''".to_string());
    let keys = schema
        .key_columns
        .iter()
        .map(|key| quoted(key))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "CREATE TABLE IF NOT EXISTS {} ({}, UNIQUE ({}))",
        quoted(&schema.table),
        columns.join(", "),
        keys
    )
}

fn patch_columns_sql(schema: &RemoteTableSchema) -> Vec<String> {
    schema
        .columns
        .iter()
        .map(|column| {
            format!(
                "ALTER TABLE {} ADD COLUMN IF NOT EXISTS {} {}",
                quoted(&schema.table),
                quoted(&column.name),
                pg_type(column.column_type)
            )
        })
        .chain([format!(
            "ALTER TABLE {} ADD COLUMN IF NOT EXISTS sync_hlc TEXT NOT NULL DEFAULT ''",
            quoted(&schema.table)
        )])
        .chain([format!(
            "ALTER TABLE {} ADD COLUMN IF NOT EXISTS sync_device TEXT NOT NULL DEFAULT ''",
            quoted(&schema.table)
        )])
        .collect()
}

/// `ON CONFLICT ... DO UPDATE` merge expression for one merged column.
fn merge_expression(schema: &RemoteTableSchema, column: &str, table_ref: &str) -> String {
    let t = table_ref;
    let c = quoted(column);
    let semantic = field_semantic(schema, column)
        .copied()
        .unwrap_or(SyncFieldSemantic::Lww);
    match semantic {
        SyncFieldSemantic::Lww => format!(
            "CASE WHEN EXCLUDED.sync_hlc > {t}.sync_hlc THEN EXCLUDED.{c} ELSE {t}.{c} END"
        ),
        SyncFieldSemantic::OrBool => format!(
            "CASE WHEN COALESCE({t}.{c}, 0) <> 0 OR COALESCE(EXCLUDED.{c}, 0) <> 0 THEN 1 ELSE 0 END"
        ),
        SyncFieldSemantic::MaxValue => format!(
            "GREATEST(COALESCE({t}.{c}, ''), COALESCE(EXCLUDED.{c}, ''))"
        ),
        // Counters merge exclusively through Inc facts; a Set never touches
        // them (the remote column default seeds zero on insert).
        SyncFieldSemantic::CounterDelta => format!("{t}.{c}"),
        SyncFieldSemantic::ElementSet => format!(
            "CASE WHEN EXCLUDED.sync_hlc > {t}.sync_hlc THEN EXCLUDED.{c} ELSE {t}.{c} END"
        ),
    }
}

/// Full-row upsert with per-field merge expressions; G-Set rows without
/// overrides are insert-only (`DO NOTHING`).
fn set_upsert_sql(schema: &RemoteTableSchema, row_count: usize) -> String {
    let table = quoted(&schema.table);
    // Counter columns are merged exclusively through Inc facts: omit them so
    // the column default (0) applies instead of an explicit NULL.
    let mut columns: Vec<String> = schema
        .columns
        .iter()
        .filter(|column| !is_counter_column(schema, &column.name))
        .map(|column| quoted(&column.name))
        .collect();
    columns.push("sync_hlc".into());
    columns.push("sync_device".into());
    let width = columns.len();
    let mut values = Vec::with_capacity(row_count);
    for row in 0..row_count {
        let placeholders = (0..width)
            .map(|index| format!("${}", row * width + index + 1))
            .collect::<Vec<_>>()
            .join(", ");
        values.push(format!("({placeholders})"));
    }
    let conflict_keys = schema
        .key_columns
        .iter()
        .map(|key| quoted(key))
        .collect::<Vec<_>>()
        .join(", ");
    let is_pure_fact =
        schema.row_semantic == SyncRowSemantic::GSet && schema.field_semantics.is_empty();
    let action = if is_pure_fact {
        "DO NOTHING".to_string()
    } else {
        let mut assignments: Vec<String> = schema
            .columns
            .iter()
            .map(|column| {
                let expression = if schema.row_semantic == SyncRowSemantic::GSet
                    && field_semantic(schema, &column.name).is_none()
                {
                    // Untouched G-Set fields keep their value.
                    format!("{table}.{}", quoted(&column.name))
                } else {
                    merge_expression(schema, &column.name, &table)
                };
                format!("{} = {}", quoted(&column.name), expression)
            })
            .collect();
        assignments.push(format!(
            "sync_hlc = GREATEST({table}.sync_hlc, EXCLUDED.sync_hlc)"
        ));
        assignments.push(format!(
            "sync_device = CASE WHEN EXCLUDED.sync_hlc > {table}.sync_hlc THEN EXCLUDED.sync_device ELSE {table}.sync_device END"
        ));
        format!("DO UPDATE SET {}", assignments.join(", "))
    };
    format!(
        "INSERT INTO {table} ({}) VALUES {} ON CONFLICT ({conflict_keys}) {action}",
        columns.join(", "),
        values.join(", ")
    )
}

fn key_predicate(schema: &RemoteTableSchema, first_param: usize) -> (String, usize) {
    let mut predicates = Vec::new();
    let mut next = first_param;
    for key in &schema.key_columns {
        predicates.push(format!("{} IS NOT DISTINCT FROM ${}", quoted(key), next));
        next += 1;
    }
    (predicates.join(" AND "), next)
}

fn is_counter_column(schema: &RemoteTableSchema, column: &str) -> bool {
    schema.field_semantics.iter().any(|(field, semantic)| {
        field == column && matches!(semantic, SyncFieldSemantic::CounterDelta)
    })
}

fn delete_sql(schema: &RemoteTableSchema) -> String {
    let (predicate, next) = key_predicate(schema, 1);
    format!(
        "DELETE FROM {} WHERE {predicate} AND sync_hlc < ${}",
        quoted(&schema.table),
        next
    )
}

fn inc_sql(schema: &RemoteTableSchema, field: &str) -> SyncStoreResult<String> {
    let (predicate, next) = key_predicate(schema, 1);
    Ok(format!(
        "UPDATE {} SET {} = COALESCE({}, 0) + ${}, sync_hlc = GREATEST(sync_hlc, ${}) WHERE {predicate}",
        quoted(&schema.table),
        quoted(field),
        quoted(field),
        next,
        next + 1
    ))
}

fn element_add_sql(schema: &RemoteTableSchema, field: &str) -> SyncStoreResult<String> {
    let (predicate, next) = key_predicate(schema, 1);
    Ok(format!(
        "UPDATE {t} SET {f} = (
            SELECT COALESCE(jsonb_agg(DISTINCT element), '[]'::jsonb)
            FROM jsonb_array_elements(COALESCE({t}.{f}, '[]'::jsonb) || to_jsonb(${n}::text)) AS element
        ) WHERE {predicate}",
        t = quoted(&schema.table),
        f = quoted(field),
        n = next
    ))
}

fn element_remove_sql(schema: &RemoteTableSchema, field: &str) -> SyncStoreResult<String> {
    let (predicate, next) = key_predicate(schema, 1);
    Ok(format!(
        "UPDATE {t} SET {f} = (
            SELECT COALESCE(jsonb_agg(element), '[]'::jsonb)
            FROM jsonb_array_elements(COALESCE({t}.{f}, '[]'::jsonb)) AS element
            WHERE element <> to_jsonb(${n}::text)
        ) WHERE {predicate}",
        t = quoted(&schema.table),
        f = quoted(field),
        n = next
    ))
}

// ───────────────────────────── param binding ────────────────────────────────

#[derive(Debug)]
enum PgValue {
    Null,
    Text(String),
    Int(i64),
    Real(f64),
    Json(Value),
}

impl ToSql for PgValue {
    fn to_sql(
        &self,
        _ty: &tokio_postgres::types::Type,
        out: &mut tokio_postgres::types::private::BytesMut,
    ) -> Result<tokio_postgres::types::IsNull, Box<dyn std::error::Error + Sync + Send>> {
        match self {
            PgValue::Null => Ok(tokio_postgres::types::IsNull::Yes),
            PgValue::Text(value) => {
                <String as ToSql>::to_sql(value, &tokio_postgres::types::Type::TEXT, out)
            }
            PgValue::Int(value) => {
                <i64 as ToSql>::to_sql(value, &tokio_postgres::types::Type::INT8, out)
            }
            PgValue::Real(value) => {
                <f64 as ToSql>::to_sql(value, &tokio_postgres::types::Type::FLOAT8, out)
            }
            PgValue::Json(value) => {
                <Value as ToSql>::to_sql(value, &tokio_postgres::types::Type::JSONB, out)
            }
        }
        .map_err(Into::into)
    }

    fn accepts(_ty: &tokio_postgres::types::Type) -> bool {
        true
    }

    tokio_postgres::types::to_sql_checked!();
}

fn bind_value(target: RemoteColumnType, value: &Value) -> Box<dyn ToSql + Sync + Send> {
    let converted = match (target, value) {
        // Fallbacks: bind whatever remains as its JSON text form.
        (_, Value::Null) => PgValue::Null,
        (RemoteColumnType::Text, Value::String(text)) => PgValue::Text(text.clone()),
        (RemoteColumnType::Text, Value::Number(number)) => PgValue::Text(number.to_string()),
        (RemoteColumnType::Text, Value::Bool(flag)) => PgValue::Text(flag.to_string()),
        (RemoteColumnType::Text, _) => PgValue::Null,
        (RemoteColumnType::BigInt, Value::Number(number)) => match number.as_i64() {
            Some(int) => PgValue::Int(int),
            None => PgValue::Int(number.as_f64().unwrap_or(0.0) as i64),
        },
        (RemoteColumnType::BigInt, Value::String(text)) => match text.parse::<i64>() {
            Ok(int) => PgValue::Int(int),
            Err(_) => PgValue::Null,
        },
        (RemoteColumnType::Double, Value::Number(number)) => {
            PgValue::Real(number.as_f64().unwrap_or(0.0))
        }
        (RemoteColumnType::Jsonb, value) => PgValue::Json(value.clone()),
        _ => PgValue::Null,
    };
    Box::new(converted)
}

// ─────────────────────────────── trait impl ─────────────────────────────────

#[async_trait]
impl RemoteSyncStore for PostgresSyncStore {
    async fn test_connection(&self) -> SyncStoreResult<SyncConnectionTestResult> {
        let started = std::time::Instant::now();
        let result = self
            .with_client(|client| async move {
                let row = client
                    .query_one("SELECT version()", &[])
                    .await
                    .map_err(|error| SyncStoreError::Other(error.to_string()))?;
                let server_version: String =
                    row.try_get(0).unwrap_or_else(|_| "PostgreSQL".to_string());
                Ok((
                    SyncConnectionTestResult {
                        ok: true,
                        server_version,
                        latency_ms: 0,
                        error: None,
                    },
                    client,
                ))
            })
            .await;
        match result {
            Ok(mut test) => {
                test.latency_ms = started.elapsed().as_millis() as u64;
                Ok(test)
            }
            Err(error) => Ok(SyncConnectionTestResult {
                ok: false,
                server_version: String::new(),
                latency_ms: started.elapsed().as_millis() as u64,
                error: Some(error.to_string()),
            }),
        }
    }

    async fn server_meta_get(&self, key: &str) -> SyncStoreResult<Option<String>> {
        self.with_client(|client| async move {
            let row = client
                .query_opt("SELECT value FROM _sync_meta WHERE key = $1", &[&key])
                .await
                .map_err(pg_error)?;
            Ok((row.and_then(|row| row.try_get(0).ok()), client))
        })
        .await
    }

    async fn server_meta_set(&self, key: &str, value: &str) -> SyncStoreResult<()> {
        let value = value.to_string();
        self.with_client(move |client| async move {
            ensure_protocol_tables(&client).await?;
            client
                .execute(
                    "INSERT INTO _sync_meta (key, value) VALUES ($1, $2)
                     ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
                    &[&key, &value],
                )
                .await
                .map_err(pg_error)?;
            Ok(((), client))
        })
        .await
    }

    async fn ensure_schema(
        &self,
        tables: &[RemoteTableSchema],
        supported_version: i64,
    ) -> SyncStoreResult<i64> {
        let tables = tables.to_vec();
        let closure_tables = tables.clone();
        self.with_client(move |client| async move {
            let row = client
                .query_opt(
                    "SELECT value FROM _sync_meta WHERE key = $1",
                    &[&META_PROTOCOL_VERSION],
                )
                .await
                .map_err(pg_error)?;
            let current: Option<String> = row.and_then(|row| row.try_get(0).ok());
            let current_version = current
                .and_then(|text| text.parse::<i64>().ok())
                .unwrap_or(0);
            if current_version > supported_version {
                return Err(SyncStoreError::RemoteSchemaTooNew {
                    remote: current_version,
                    supported: supported_version,
                });
            }
            for table in &closure_tables {
                client
                    .execute(&create_table_sql(table), &[])
                    .await
                    .map_err(pg_error)?;
                for sql in patch_columns_sql(table) {
                    client.execute(&sql, &[]).await.map_err(pg_error)?;
                }
            }
            if current_version < supported_version {
                let value = supported_version.to_string();
                client
                    .execute(
                        "INSERT INTO _sync_meta (key, value) VALUES ($1, $2)
                         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
                        &[&META_PROTOCOL_VERSION, &value],
                    )
                    .await
                    .map_err(pg_error)?;
            }
            Ok((supported_version, client))
        })
        .await?;
        self.remember_schemas(&tables);
        Ok(supported_version)
    }

    async fn remote_is_empty(&self) -> SyncStoreResult<bool> {
        self.with_client(|client| async move {
            let has_ops = client
                .query_opt("SELECT 1 FROM sync_ops LIMIT 1", &[])
                .await
                .map_err(pg_error)?
                .is_some();
            if has_ops {
                return Ok((false, client));
            }
            let provisioned = client
                .query_opt(
                    "SELECT value FROM _sync_meta WHERE key = $1",
                    &[&META_PROVISIONED_AT],
                )
                .await
                .map_err(pg_error)?
                .is_some();
            Ok((!provisioned, client))
        })
        .await
    }

    async fn latest_seq(&self) -> SyncStoreResult<i64> {
        self.with_client(|client| async move {
            let row = client
                .query_one("SELECT COALESCE(MAX(server_seq), 0) FROM sync_ops", &[])
                .await
                .map_err(pg_error)?;
            Ok((row.try_get(0).unwrap_or(0), client))
        })
        .await
    }

    async fn first_retained_seq(&self) -> SyncStoreResult<i64> {
        self.with_client(|client| async move {
            let row = client
                .query_one("SELECT COALESCE(MIN(server_seq), 0) FROM sync_ops", &[])
                .await
                .map_err(pg_error)?;
            Ok((row.try_get(0).unwrap_or(0), client))
        })
        .await
    }

    async fn push_ops(&self, ops: &[SyncOpRecord]) -> SyncStoreResult<()> {
        if ops.is_empty() {
            return Ok(());
        }
        // PostgreSQL resolves lock cycles between clients by aborting one
        // transaction; pushes are idempotent, so retrying is safe. Concurrent
        // first-time uploads from two devices collide regularly, so allow a
        // few more attempts with a growing backoff.
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            match self.push_ops_once(ops).await {
                Ok(()) => return Ok(()),
                Err(error) if attempt < 8 && is_retryable(&error) => {
                    tracing::warn!(attempt, error = %error, "push conflicted; retrying");
                    // Quadratic backoff plus jitter so colliding clients
                    // decorrelate instead of retrying in lockstep.
                    let base = 150 * u64::from(attempt) * u64::from(attempt);
                    let jitter = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.subsec_millis() as u64)
                        .unwrap_or(0)
                        % 250;
                    tokio::time::sleep(std::time::Duration::from_millis(base + jitter)).await;
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn fetch_ops(&self, cursor: i64, limit: i64) -> SyncStoreResult<Vec<PulledOp>> {
        self.with_client(move |client| async move {
            let rows = client
                .query(
                    "SELECT server_seq, op_id, table_name, entity_key, op, payload::text, hlc, device
                     FROM sync_ops WHERE server_seq > $1 ORDER BY server_seq LIMIT $2",
                    &[&cursor, &limit],
                )
                .await
                .map_err(pg_error)?;
            let mut pulled = Vec::with_capacity(rows.len());
            for row in rows {
                let server_seq: i64 = row.try_get(0).map_err(pg_error)?;
                let op_id: String = row.try_get(1).map_err(pg_error)?;
                let table: String = row.try_get(2).map_err(pg_error)?;
                let entity_key_text: String = row.try_get(3).map_err(pg_error)?;
                let op_code: String = row.try_get(4).map_err(pg_error)?;
                let payload_text: Option<String> = row.try_get(5).map_err(pg_error)?;
                let hlc: String = row.try_get(6).map_err(pg_error)?;
                let device: String = row.try_get(7).map_err(pg_error)?;
                pulled.push(PulledOp {
                    server_seq,
                    op: SyncOpRecord {
                        op_id,
                        table,
                        entity_key: serde_json::from_str(&entity_key_text)
                            .unwrap_or_default(),
                        kind: parse_kind(&op_code),
                        payload: payload_text
                            .and_then(|text| serde_json::from_str(&text).ok()),
                        hlc,
                        device,
                    },
                });
            }
            Ok((pulled, client))
        })
        .await
    }

    async fn fetch_materialized(
        &self,
        table: &str,
        limit: i64,
        offset: i64,
    ) -> SyncStoreResult<Vec<MaterializedRow>> {
        let schema = self.schema_for(table)?;
        let table_name = table.to_string();
        self.with_client(move |client| async move {
            let order = schema
                .key_columns
                .iter()
                .map(|key| quoted(key))
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "SELECT * FROM {} ORDER BY {order} LIMIT $1 OFFSET $2",
                quoted(&table_name)
            );
            let statement = client.prepare(&sql).await.map_err(pg_error)?;
            let rows = client
                .query(&statement, &[&limit, &offset])
                .await
                .map_err(pg_error)?;
            let mut materialized = Vec::with_capacity(rows.len());
            for row in rows {
                let mut sync_hlc = String::new();
                let mut sync_device = String::new();
                let mut columns = Map::new();
                for (index, column) in statement.columns().iter().enumerate() {
                    let name = column.name();
                    let value = pg_row_value(&row, index, column.type_())?;
                    match name {
                        "sync_hlc" => sync_hlc = value.as_str().unwrap_or_default().to_string(),
                        "sync_device" => {
                            sync_device = value.as_str().unwrap_or_default().to_string()
                        }
                        _ => {
                            columns.insert(name.to_string(), value);
                        }
                    }
                }
                materialized.push(MaterializedRow {
                    sync_hlc,
                    sync_device,
                    columns,
                });
            }
            Ok((materialized, client))
        })
        .await
    }

    async fn devices_upsert(&self, record: &SyncDeviceRecord) -> SyncStoreResult<()> {
        let record = record.clone();
        self.with_client(move |client| async move {
            // client_addr is captured by the server from the connection
            // itself, so a device cannot misreport its own address.
            client
                .execute(
                    "INSERT INTO _sync_devices (device_id, app_version, device_name, client_addr, last_seen_at)
                     VALUES ($1, $2, $3, inet_client_addr()::text, now())
                     ON CONFLICT (device_id) DO UPDATE SET app_version = EXCLUDED.app_version,
                       device_name = EXCLUDED.device_name, client_addr = EXCLUDED.client_addr,
                       last_seen_at = now()",
                    &[&record.device_id, &record.app_version, &record.device_name],
                )
                .await
                .map_err(pg_error)?;
            Ok(((), client))
        })
        .await
    }

    async fn devices_list(&self) -> SyncStoreResult<Vec<SyncDeviceRecord>> {
        self.with_client(|client| async move {
            let rows = client
                .query(
                    "SELECT device_id, app_version, device_name, client_addr,
                            last_seen_at::text,
                            EXTRACT(EPOCH FROM (now() - last_seen_at))::bigint AS seen_seconds_ago
                     FROM _sync_devices ORDER BY last_seen_at DESC",
                    &[],
                )
                .await
                .map_err(pg_error)?;
            Ok((
                rows.into_iter()
                    .map(|row| SyncDeviceRecord {
                        device_id: row.try_get(0).unwrap_or_default(),
                        app_version: row.try_get(1).unwrap_or_default(),
                        device_name: row.try_get(2).unwrap_or_default(),
                        ip_addr: row.try_get(3).unwrap_or_default(),
                        last_seen_at: row.try_get(4).ok(),
                        seen_seconds_ago: row.try_get(5).unwrap_or_default(),
                        ..SyncDeviceRecord::default()
                    })
                    .collect(),
                client,
            ))
        })
        .await
    }

    async fn ops_gc(&self, retain_days: i64) -> SyncStoreResult<u64> {
        // make_interval's named args are int4; an i64 bind fails client-side
        // type checks ("error serializing parameter 0").
        let retain_days = i32::try_from(retain_days).unwrap_or(i32::MAX);
        self.with_client(move |client| async move {
            let removed = client
                .execute(
                    "DELETE FROM sync_ops WHERE created_at < now() - make_interval(days => $1)",
                    &[&retain_days],
                )
                .await
                .map_err(pg_error)?;
            Ok((removed as u64, client))
        })
        .await
    }
}

const PROTOCOL_TABLE_DDL: [&str; 6] = [
    "CREATE TABLE IF NOT EXISTS _sync_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL DEFAULT '')",
    "CREATE TABLE IF NOT EXISTS sync_ops (
        server_seq BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
        op_id TEXT NOT NULL UNIQUE,
        table_name TEXT NOT NULL,
        entity_key TEXT NOT NULL DEFAULT '[]',
        op TEXT NOT NULL,
        payload JSONB,
        hlc TEXT NOT NULL DEFAULT '',
        device TEXT NOT NULL DEFAULT '',
        created_at TIMESTAMPTZ NOT NULL DEFAULT now()
    )",
    "CREATE INDEX IF NOT EXISTS sync_ops_created_at_idx ON sync_ops (created_at)",
    "CREATE TABLE IF NOT EXISTS _sync_devices (
        device_id TEXT PRIMARY KEY,
        app_version TEXT NOT NULL DEFAULT '',
        device_name TEXT NOT NULL DEFAULT '',
        client_addr TEXT NOT NULL DEFAULT '',
        last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now()
    )",
    "ALTER TABLE _sync_devices ADD COLUMN IF NOT EXISTS device_name TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE _sync_devices ADD COLUMN IF NOT EXISTS client_addr TEXT NOT NULL DEFAULT ''",
];

async fn ensure_protocol_tables(
    client: &impl tokio_postgres::GenericClient,
) -> SyncStoreResult<()> {
    for sql in PROTOCOL_TABLE_DDL {
        client.execute(sql, &[]).await.map_err(pg_error)?;
    }
    Ok(())
}

async fn materialize_op(
    client: &impl tokio_postgres::GenericClient,
    op: &SyncOpRecord,
    schema: &RemoteTableSchema,
) -> SyncStoreResult<()> {
    let key_params: Vec<Box<dyn ToSql + Sync + Send>> = schema
        .key_columns
        .iter()
        .zip(op.entity_key.iter())
        .map(|(key_column, value)| {
            let column_type = schema
                .columns
                .iter()
                .find(|column| &column.name == key_column)
                .map(|column| column.column_type)
                .unwrap_or(RemoteColumnType::Text);
            bind_value(column_type, value)
        })
        .collect();
    match op.kind {
        SyncOpKind::Delete => {
            let sql = delete_sql(schema);
            let mut params = key_params;
            params.push(Box::new(op.hlc.clone()));
            let refs = params
                .iter()
                .map(|p| &**p as &(dyn ToSql + Sync))
                .collect::<Vec<_>>();
            client.execute(&sql, &refs).await.map_err(pg_error)?;
        }
        SyncOpKind::Set => {
            let Some(payload) = op.payload.as_ref() else {
                return Ok(());
            };
            let sql = set_upsert_sql(schema, 1);
            let mut params: Vec<Box<dyn ToSql + Sync + Send>> = Vec::new();
            for column in &schema.columns {
                if is_counter_column(schema, &column.name) {
                    continue;
                }
                let value = payload.get(&column.name).cloned().unwrap_or(Value::Null);
                params.push(bind_value(column.column_type, &value));
            }
            params.push(Box::new(op.hlc.clone()));
            params.push(Box::new(op.device.clone()));
            let refs = params
                .iter()
                .map(|p| &**p as &(dyn ToSql + Sync))
                .collect::<Vec<_>>();
            client.execute(&sql, &refs).await.map_err(pg_error)?;
        }
        SyncOpKind::Inc => {
            let Some(payload) = op.payload.as_ref() else {
                return Ok(());
            };
            let field = payload
                .get("field")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let delta = payload.get("delta").and_then(Value::as_i64).unwrap_or(0);
            let sql = inc_sql(schema, field)?;
            let mut params = key_params;
            params.push(Box::new(delta));
            params.push(Box::new(op.hlc.clone()));
            let refs = params
                .iter()
                .map(|p| &**p as &(dyn ToSql + Sync))
                .collect::<Vec<_>>();
            client.execute(&sql, &refs).await.map_err(pg_error)?;
        }
        SyncOpKind::SetAdd | SyncOpKind::SetRemove => {
            let Some(payload) = op.payload.as_ref() else {
                return Ok(());
            };
            let field = payload
                .get("field")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let element = payload
                .get("element")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let sql = if op.kind == SyncOpKind::SetAdd {
                element_add_sql(schema, field)?
            } else {
                element_remove_sql(schema, field)?
            };
            let mut params = key_params;
            params.push(Box::new(element));
            let refs = params
                .iter()
                .map(|p| &**p as &(dyn ToSql + Sync))
                .collect::<Vec<_>>();
            client.execute(&sql, &refs).await.map_err(pg_error)?;
        }
    }
    Ok(())
}

fn pg_row_value(
    row: &tokio_postgres::Row,
    index: usize,
    ty: &tokio_postgres::types::Type,
) -> SyncStoreResult<Value> {
    let value = match *ty {
        tokio_postgres::types::Type::INT2 => row
            .try_get::<_, Option<i16>>(index)
            .map_err(pg_error)?
            .map(|v| Value::from(v as i64)),
        tokio_postgres::types::Type::INT4 => row
            .try_get::<_, Option<i32>>(index)
            .map_err(pg_error)?
            .map(|v| Value::from(v as i64)),
        tokio_postgres::types::Type::INT8 => row
            .try_get::<_, Option<i64>>(index)
            .map_err(pg_error)?
            .map(Value::from),
        tokio_postgres::types::Type::FLOAT4 => row
            .try_get::<_, Option<f32>>(index)
            .map_err(pg_error)?
            .map(|v| Value::from(v as f64)),
        tokio_postgres::types::Type::FLOAT8 => row
            .try_get::<_, Option<f64>>(index)
            .map_err(pg_error)?
            .map(|v| {
                serde_json::Number::from_f64(v)
                    .map(Value::Number)
                    .unwrap_or(Value::Null)
            }),
        tokio_postgres::types::Type::JSONB | tokio_postgres::types::Type::JSON => {
            row.try_get::<_, Option<Value>>(index).map_err(pg_error)?
        }
        tokio_postgres::types::Type::BOOL => row
            .try_get::<_, Option<bool>>(index)
            .map_err(pg_error)?
            .map(|v| Value::from(v)),
        _ => row
            .try_get::<_, Option<String>>(index)
            .map_err(pg_error)?
            .map(Value::String),
    };
    Ok(value.unwrap_or(Value::Null))
}

fn kind_code(kind: SyncOpKind) -> &'static str {
    match kind {
        SyncOpKind::Set => "set",
        SyncOpKind::Delete => "del",
        SyncOpKind::Inc => "inc",
        SyncOpKind::SetAdd => "add",
        SyncOpKind::SetRemove => "rm",
    }
}

fn parse_kind(code: &str) -> SyncOpKind {
    match code {
        "del" => SyncOpKind::Delete,
        "inc" => SyncOpKind::Inc,
        "add" => SyncOpKind::SetAdd,
        "rm" => SyncOpKind::SetRemove,
        _ => SyncOpKind::Set,
    }
}

impl PostgresSyncStore {
    async fn push_ops_once(&self, ops: &[SyncOpRecord]) -> SyncStoreResult<()> {
        // One deterministic order the op-log insert and the
        // materialization: concurrent multi-row inserts into the op log's
        // unique index deadlock on index-page locks when two clients write
        // overlapping ranges in different orders. Sorting by (table, key,
        // op_id) makes every client take the same lock sequence; op_id keeps
        // a Set ahead of its Inc facts.
        let mut ops = ops.to_vec();
        ops.sort_by(|a, b| {
            (
                a.table.as_str(),
                entity_key_text(&a.entity_key).as_str(),
                a.op_id.as_str(),
            )
                .cmp(&(
                    b.table.as_str(),
                    entity_key_text(&b.entity_key).as_str(),
                    b.op_id.as_str(),
                ))
        });
        // Resolve materialization schemas up front. A table missing here
        // means ops would land in the log without ever being materialized —
        // fail loudly instead of dropping data silently.
        let schemas = self.schemas.read().unwrap().clone();
        for table in ops
            .iter()
            .map(|op| op.table.clone())
            .collect::<HashSet<String>>()
        {
            if !schemas.contains_key(&table) {
                return Err(SyncStoreError::Other(format!(
                    "materialization schema for table {table} was not ensured; refusing to log ops without materializing them"
                )));
            }
        }
        self.with_client(move |mut client| async move {
            let transaction = client.transaction().await.map_err(pg_error)?;
            // Serialize pushes across clients: one writer at a time removes
            // every interleaving that can form a lock cycle. The lock is
            // transaction-scoped and released automatically on commit.
            transaction
                .execute("SELECT pg_advisory_xact_lock(940817)", &[])
                .await
                .map_err(pg_error)?;
            ensure_protocol_tables(&transaction).await?;

            // 1. Append ops; only freshly inserted ones need materialization.
            let mut inserted_ids = HashSet::new();
            for chunk in ops.chunks(100) {
                let mut sql = String::from(
                    "INSERT INTO sync_ops (op_id, table_name, entity_key, op, payload, hlc, device) VALUES ",
                );
                let mut params: Vec<Box<dyn ToSql + Sync + Send>> = Vec::new();
                let mut separator = "";
                for op in chunk.iter() {
                    let base = params.len();
                    let placeholders: Vec<String> = (0..7)
                        .map(|offset| format!("${}", base + offset + 1))
                        .collect();
                    sql.push_str(separator);
                    sql.push_str(&format!("({})", placeholders.join(", ")));
                    separator = ", ";
                    params.push(Box::new(op.op_id.clone()));
                    params.push(Box::new(op.table.clone()));
                    params.push(Box::new(
                        serde_json::to_string(&op.entity_key).unwrap_or_else(|_| "[]".into()),
                    ));
                    params.push(Box::new(kind_code(op.kind).to_string()));
                    params.push(Box::new(
                        op.payload.clone().map(Value::Object).unwrap_or(Value::Null),
                    ));
                    params.push(Box::new(op.hlc.clone()));
                    params.push(Box::new(op.device.clone()));
                }
                sql.push_str(" ON CONFLICT (op_id) DO NOTHING RETURNING op_id");
                let refs = params
                    .iter()
                    .map(|p| &**p as &(dyn ToSql + Sync))
                    .collect::<Vec<_>>();
                let rows = transaction.query(&sql, &refs).await.map_err(pg_error)?;
                for row in rows {
                    if let Ok(op_id) = row.try_get::<_, String>(0) {
                        inserted_ids.insert(op_id);
                    }
                }
            }

            // 2. Materialize freshly appended ops through lattice merges.
            //    `ops` is already in the deterministic global order from the
            //    top of this function. Runs of Set ops for one table are
            //    materialized as a single multi-row upsert — bootstrap
            //    uploads are Set-heavy, and batching them is an order of
            //    magnitude faster than per-op statements.
            let mut set_batch: Vec<&SyncOpRecord> = Vec::new();
            for op in ops.iter().filter(|op| inserted_ids.contains(&op.op_id)) {
                if op.kind == SyncOpKind::Set {
                    // Start a new statement whenever the table changes OR the
                    // natural key repeats: PostgreSQL rejects a multi-row
                    // upsert that would affect the same row twice (21000),
                    // and source tables without a local unique index (feed
                    // tables) legitimately contain duplicate natural keys.
                    // Ops are sorted, so equal keys are adjacent — sequential
                    // statements preserve exact last-write-wins semantics.
                    let key_repeats = set_batch
                        .last()
                        .is_some_and(|last| {
                            last.table == op.table
                                && entity_key_text(&last.entity_key)
                                    == entity_key_text(&op.entity_key)
                        });
                    if key_repeats || (!set_batch.is_empty() && set_batch[0].table != op.table) {
                        flush_set_batch(&transaction, &schemas, &mut set_batch).await?;
                    }
                    set_batch.push(op);
                    if set_batch.len() >= 500 {
                        flush_set_batch(&transaction, &schemas, &mut set_batch).await?;
                    }
                    continue;
                }
                flush_set_batch(&transaction, &schemas, &mut set_batch).await?;
                let schema = schemas.get(&op.table).expect("checked above");
                materialize_op(&transaction, op, schema).await?;
            }
            flush_set_batch(&transaction, &schemas, &mut set_batch).await?;

            transaction.commit().await.map_err(pg_error)?;
            Ok(((), client))
        })
        .await
    }
}

#[cfg(test)]
mod pg_smoke {
    use super::*;
    use std::str::FromStr;
    use vrcx_0_application_sync::RemoteColumnDef;

    /// Full roundtrip against a real server; runs only when
    /// VRCX_PG_TEST_DSN is set, and cleans up everything it created so a
    /// subsequent first-enable still bootstraps from a clean state.
    #[test]
    fn postgres_roundtrip_smoke() {
        let Ok(dsn) = std::env::var("VRCX_PG_TEST_DSN") else {
            return;
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async move {
            let store = PostgresSyncStore::new(&PostgresSyncStoreConfig {
                dsn: dsn.clone(),
                tls_verify: false,
                allow_plaintext: true,
            })
            .expect("store");

            let probe = store.test_connection().await.expect("probe");
            assert!(probe.ok, "connection failed: {:?}", probe.error);
            println!("server: {} ({} ms)", probe.server_version, probe.latency_ms);

            let schema = vec![RemoteTableSchema {
                table: "smoke_test_rows".into(),
                columns: vec![
                    RemoteColumnDef {
                        name: "id".into(),
                        column_type: RemoteColumnType::Text,
                    },
                    RemoteColumnDef {
                        name: "name".into(),
                        column_type: RemoteColumnType::Text,
                    },
                    RemoteColumnDef {
                        name: "tally".into(),
                        column_type: RemoteColumnType::BigInt,
                    },
                ],
                key_columns: vec!["id".into()],
                row_semantic: SyncRowSemantic::Lww,
                field_semantics: vec![("tally".to_string(), SyncFieldSemantic::CounterDelta)],
            }];
            let version = store.ensure_schema(&schema, 1).await.expect("schema");
            assert_eq!(version, 1);

            let key = vec![Value::String("row-1".into())];
            let mut payload = Map::new();
            payload.insert("id".to_string(), Value::String("row-1".into()));
            payload.insert("name".to_string(), Value::String("alpha".into()));
            let set_op = SyncOpRecord {
                op_id: "smoke-1-set".into(),
                table: "smoke_test_rows".into(),
                entity_key: key.clone(),
                kind: SyncOpKind::Set,
                payload: Some(payload),
                hlc: "00000000000000001-00000000-smoketest".into(),
                device: "smoketest".into(),
            };
            let mut inc_payload = Map::new();
            inc_payload.insert("field".to_string(), Value::String("tally".into()));
            inc_payload.insert("delta".to_string(), Value::from(5i64));
            let inc_op = SyncOpRecord {
                op_id: "smoke-2-inc".into(),
                table: "smoke_test_rows".into(),
                entity_key: key,
                kind: SyncOpKind::Inc,
                payload: Some(inc_payload),
                hlc: "00000000000000002-00000000-smoketest".into(),
                device: "smoketest".into(),
            };
            store
                .push_ops(&[set_op.clone(), inc_op])
                .await
                .expect("push");

            // Re-push is idempotent by op id.
            store.push_ops(&[set_op.clone()]).await.expect("repush");

            let pulled = store.fetch_ops(0, 10).await.expect("fetch ops");
            assert_eq!(pulled.len(), 2, "op log contains exactly the two ops");
            // The engine's derive contract stamps Set ops to sort before
            // their Inc facts (same key, op_id order); the ids above honor
            // it, so the set precedes the inc in the log.
            let pushed_ids: Vec<&str> = pulled.iter().map(|item| item.op.op_id.as_str()).collect();
            assert_eq!(pushed_ids, vec!["smoke-1-set", "smoke-2-inc"]);

            let materialized = store
                .fetch_materialized("smoke_test_rows", 10, 0)
                .await
                .expect("materialized");
            assert_eq!(materialized.len(), 1);
            assert_eq!(
                materialized[0].columns.get("tally"),
                Some(&Value::from(5i64)),
                "counter merged through Inc facts"
            );
            assert_eq!(
                materialized[0].columns.get("name"),
                Some(&Value::String("alpha".into()))
            );

            // Device presence roundtrip: name stored, address captured by the
            // server, heartbeat age computable without client clocks.
            store
                .devices_upsert(&SyncDeviceRecord {
                    device_id: "smoketest-device".into(),
                    app_version: "0.0.0-smoke".into(),
                    device_name: "smoke-host".into(),
                    ..SyncDeviceRecord::default()
                })
                .await
                .expect("devices upsert");
            let devices = store.devices_list().await.expect("devices list");
            let device = devices
                .iter()
                .find(|device| device.device_id == "smoketest-device")
                .expect("upserted device listed");
            assert_eq!(device.device_name, "smoke-host");
            assert_eq!(device.app_version, "0.0.0-smoke");
            assert!(
                device.seen_seconds_ago <= 5,
                "heartbeat age is server-computed and fresh, got {}",
                device.seen_seconds_ago
            );
            assert!(
                device.last_seen_at.is_some(),
                "last seen timestamp returned"
            );
            println!("device addr captured by server: {:?}", device.ip_addr);

            // Cleanup so the app's real bootstrap starts from a clean slate.
            let mut raw_config = tokio_postgres::Config::from_str(&dsn).unwrap();
            raw_config.ssl_mode(tokio_postgres::config::SslMode::Prefer);
            let (client, connection) = raw_config
                .connect(make_tls_connector(false).unwrap())
                .await
                .expect("raw connect");
            tokio::spawn(async move {
                let _ = connection.await;
            });
            for sql in [
                "DROP TABLE IF EXISTS smoke_test_rows",
                "DELETE FROM sync_ops",
                "DELETE FROM _sync_meta",
                "DELETE FROM _sync_devices",
            ] {
                client.execute(sql, &[]).await.expect("cleanup");
            }
            println!("smoke roundtrip OK; remote cleaned");
        });
    }
}

#[cfg(test)]
mod pg_diag {
    use super::*;
    use std::str::FromStr;
    use vrcx_0_application_sync::RemoteColumnDef;

    async fn diag_client(dsn: &str) -> Client {
        let mut config = tokio_postgres::Config::from_str(dsn).unwrap();
        config.ssl_mode(tokio_postgres::config::SslMode::Prefer);
        let (client, connection) = config
            .connect(make_tls_connector(false).unwrap())
            .await
            .expect("connect");
        tokio::spawn(async move {
            let _ = connection.await;
        });
        client
    }

    /// Read-only: who is connected, and any lock blockers.
    #[test]
    fn list_connections() {
        let Ok(dsn) = std::env::var("VRCX_PG_TEST_DSN") else {
            return;
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let client = diag_client(&dsn).await;
            let rows = client
                .query(
                    "SELECT pid, state, coalesce(client_addr::text,''), (now()-xact_start)::text, left(coalesce(query,''),80)
                     FROM pg_stat_activity
                     WHERE datname = current_database() AND pid <> pg_backend_pid()",
                    &[],
                )
                .await
                .expect("stat");
            for row in &rows {
                let pid: i32 = row.get(0);
                let state: String = row.get(1);
                let addr: String = row.get(2);
                let xact_age: String = row.get(3);
                let query: String = row.get(4);
                println!("pid={pid} state={state} addr={addr} xact_age={} q={query}", xact_age);
            }
            println!("total backends: {}", rows.len());
        });
    }

    /// Two connections pushing overlapping rows in adversarial key orders
    /// concurrently — the exact shape that deadlocked real clients. With
    /// deterministic (table, key) materialization order plus lock-timeout
    /// retries, every push must succeed and counters must sum exactly.
    #[test]
    fn concurrent_pushes_with_inverted_key_orders_converge() {
        let Ok(dsn) = std::env::var("VRCX_PG_TEST_DSN") else {
            return;
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let store_a = PostgresSyncStore::new(&PostgresSyncStoreConfig {
                dsn: dsn.clone(),
                tls_verify: false,
                allow_plaintext: true,
            })
            .unwrap();
            let store_b = PostgresSyncStore::new(&PostgresSyncStoreConfig {
                dsn: dsn.clone(),
                tls_verify: false,
                allow_plaintext: true,
            })
            .unwrap();

            let schema = vec![RemoteTableSchema {
                table: "smoke_concurrency_rows".into(),
                columns: vec![
                    RemoteColumnDef {
                        name: "id".into(),
                        column_type: RemoteColumnType::Text,
                    },
                    RemoteColumnDef {
                        name: "val".into(),
                        column_type: RemoteColumnType::Text,
                    },
                    RemoteColumnDef {
                        name: "tally".into(),
                        column_type: RemoteColumnType::BigInt,
                    },
                ],
                key_columns: vec!["id".into()],
                row_semantic: SyncRowSemantic::Lww,
                field_semantics: vec![("tally".to_string(), SyncFieldSemantic::CounterDelta)],
            }];
            store_a.ensure_schema(&schema, 1).await.expect("schema");
            store_b.ensure_schema(&schema, 1).await.expect("schema b");

            // Adversarial: A pushes keys ascending, B pushes the same keys
            // descending, both at the same time, several rounds.
            for round in 0..10u32 {
                let keys: Vec<u32> = (0..24).collect();
                let make_ops = |prefix: char, order: Vec<u32>| {
                    order
                        .into_iter()
                        .map(|k| {
                            let id = format!("row-{k}");
                            let mut payload = Map::new();
                            payload.insert("id".to_string(), Value::String(id.clone()));
                            payload
                                .insert("val".to_string(), Value::String(format!("{prefix}{k}")));
                            SyncOpRecord {
                                op_id: format!("conc-{round}-{prefix}-{k}"),
                                table: "smoke_concurrency_rows".into(),
                                entity_key: vec![Value::String(id)],
                                kind: SyncOpKind::Set,
                                payload: Some(payload),
                                hlc: format!(
                                    "{:017}-00000000-conc{prefix}{round}",
                                    1_000_000_000u64 + u64::from(round)
                                ),
                                device: format!("conc-{prefix}"),
                            }
                        })
                        .collect::<Vec<_>>()
                };
                let ops_a = make_ops('a', keys.clone());
                let mut reversed = keys.clone();
                reversed.reverse();
                let ops_b = make_ops('b', reversed);
                let (ra, rb) = tokio::join!(store_a.push_ops(&ops_a), store_b.push_ops(&ops_b));
                ra.expect("push a");
                rb.expect("push b");
            }

            // Counter deltas from both sides sum exactly.
            for round in 0..10u32 {
                let inc = |device: char, k: u32| SyncOpRecord {
                    op_id: format!("conc-inc-{round}-{device}-{k}"),
                    table: "smoke_concurrency_rows".into(),
                    entity_key: vec![Value::String(format!("row-{k}"))],
                    kind: SyncOpKind::Inc,
                    payload: Some(Map::from_iter([
                        ("field".to_string(), Value::String("tally".into())),
                        ("delta".to_string(), Value::from(1i64)),
                    ])),
                    hlc: format!(
                        "{:017}-00000000-conci{device}{round}",
                        2_000_000_000u64 + u64::from(round)
                    ),
                    device: format!("conc-{device}"),
                };
                let mut ops_a = Vec::new();
                let mut ops_b = Vec::new();
                for k in (0..24u32).rev() {
                    ops_a.push(inc('a', k));
                }
                for k in 0..24u32 {
                    ops_b.push(inc('b', k));
                }
                let (ra, rb) = tokio::join!(store_a.push_ops(&ops_a), store_b.push_ops(&ops_b));
                ra.expect("inc a");
                rb.expect("inc b");
            }

            let rows = store_a
                .fetch_materialized("smoke_concurrency_rows", 100, 0)
                .await
                .expect("materialized");
            assert_eq!(rows.len(), 24, "all keys exist");
            for row in &rows {
                assert_eq!(
                    row.columns.get("tally"),
                    Some(&Value::from(20i64)),
                    "10 rounds x 2 devices of +1 each; got {:?}",
                    row.columns.get("tally")
                );
            }
            println!("concurrency roundtrip OK: 24 rows, tallies exact");

            // Cleanup only our own artifacts.
            let client = diag_client(&dsn).await;
            for sql in [
                "DROP TABLE IF EXISTS smoke_concurrency_rows",
                "DELETE FROM sync_ops WHERE op_id LIKE 'conc-%'",
            ] {
                client.execute(sql, &[]).await.expect("cleanup");
            }
        });
    }

    /// Duplicate natural keys inside one push must not trip PostgreSQL's
    /// "ON CONFLICT DO UPDATE cannot affect row a second time" — the exact
    /// feed_gps failure.
    #[test]
    fn duplicate_key_sets_in_one_push_apply_sequentially() {
        let Ok(dsn) = std::env::var("VRCX_PG_TEST_DSN") else {
            return;
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let store = PostgresSyncStore::new(&PostgresSyncStoreConfig {
                dsn: dsn.clone(),
                tls_verify: false,
                allow_plaintext: true,
            })
            .unwrap();
            let schema = vec![RemoteTableSchema {
                table: "smoke_dupkey_rows".into(),
                columns: vec![
                    RemoteColumnDef {
                        name: "id".into(),
                        column_type: RemoteColumnType::Text,
                    },
                    RemoteColumnDef {
                        name: "val".into(),
                        column_type: RemoteColumnType::Text,
                    },
                ],
                key_columns: vec!["id".into()],
                row_semantic: SyncRowSemantic::Lww,
                field_semantics: vec![],
            }];
            store.ensure_schema(&schema, 1).await.expect("schema");
            let set = |id: &str, val: &str, op_id: &str, hlc: &str| SyncOpRecord {
                op_id: op_id.into(),
                table: "smoke_dupkey_rows".into(),
                entity_key: vec![Value::String(id.into())],
                kind: SyncOpKind::Set,
                payload: Some(Map::from_iter([
                    ("id".to_string(), Value::String(id.into())),
                    ("val".to_string(), Value::String(val.into())),
                ])),
                hlc: hlc.into(),
                device: "dupkey".into(),
            };
            // One push containing three ops for the same key plus a unique
            // one — previously a single batched statement, now split.
            let ops = vec![
                set("k1", "v1", "d1", "00000000000000001-00000000-dupkey"),
                set("k1", "v2", "d2", "00000000000000002-00000000-dupkey"),
                set("k1", "v3", "d3", "00000000000000003-00000000-dupkey"),
                set("k2", "w1", "d4", "00000000000000004-00000000-dupkey"),
            ];
            store
                .push_ops(&ops)
                .await
                .expect("push with duplicate keys");
            let rows = store
                .fetch_materialized("smoke_dupkey_rows", 10, 0)
                .await
                .expect("read");
            let value_of = |id: &str| {
                rows.iter()
                    .find(|row| row.columns.get("id") == Some(&Value::String(id.into())))
                    .and_then(|row| row.columns.get("val").cloned())
            };
            assert_eq!(
                value_of("k1"),
                Some(Value::String("v3".into())),
                "last write wins"
            );
            assert_eq!(value_of("k2"), Some(Value::String("w1".into())));
            println!("duplicate-key push OK");
            let client = diag_client(&dsn).await;
            for sql in [
                "DROP TABLE IF EXISTS smoke_dupkey_rows",
                "DELETE FROM sync_ops WHERE op_id LIKE 'd%' AND device = 'dupkey'",
            ] {
                client.execute(sql, &[]).await.expect("cleanup");
            }
        });
    }

    /// Destructive, explicit (VRCX_PG_RESET=1): wipe everything sync-related
    /// and stamp a fresh epoch so every client re-bootstraps cleanly.
    #[test]
    fn remote_reset() {
        let Ok(dsn) = std::env::var("VRCX_PG_TEST_DSN") else {
            return;
        };
        if std::env::var("VRCX_PG_RESET").ok().as_deref() != Some("1") {
            return;
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let client = diag_client(&dsn).await;
            let tables = client
                .query(
                    "SELECT tablename FROM pg_tables WHERE schemaname = 'public'",
                    &[],
                )
                .await
                .expect("list tables");
            let names: Vec<String> = tables
                .into_iter()
                .map(|row| row.get::<_, String>(0))
                .filter(|name| {
                    !matches!(name.as_str(), "_sync_meta" | "sync_ops" | "_sync_devices")
                })
                .collect();
            for name in &names {
                client
                    .execute(&format!("DROP TABLE IF EXISTS \"{name}\" CASCADE"), &[])
                    .await
                    .expect("drop");
            }
            for sql in [
                "TRUNCATE sync_ops",
                "TRUNCATE _sync_meta",
                "TRUNCATE _sync_devices",
            ] {
                client.execute(sql, &[]).await.expect("truncate");
            }
            let epoch = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            client
                .execute(
                    "INSERT INTO _sync_meta (key, value) VALUES ('sync.epoch', $1)",
                    &[&epoch.to_string()],
                )
                .await
                .expect("epoch");
            println!("dropped {} tables; epoch={epoch}", names.len());
        });
    }

    /// Read-only: op-log probe for one table's ops.
    #[test]
    fn feed_ops_probe() {
        let Ok(dsn) = std::env::var("VRCX_PG_TEST_DSN") else {
            return;
        };
        let table = std::env::var("VRCX_PG_WATCH_TABLE").expect("VRCX_PG_WATCH_TABLE");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let client = diag_client(&dsn).await;
            let row = client
                .query_one(
                    "SELECT COUNT(*), COALESCE(MAX(created_at)::text,'none'), COALESCE(MIN(created_at)::text,'none')
                     FROM sync_ops WHERE table_name = $1",
                    &[&table],
                )
                .await
                .expect("probe");
            let count: i64 = row.get(0);
            let max_at: String = row.get(1);
            let min_at: String = row.get(2);
            println!("{table} ops={count} first={min_at} last={max_at}");
            let row = client
                .query_one("SELECT COUNT(*), COALESCE(MAX(created_at)::text,'none') FROM sync_ops", &[])
                .await
                .expect("total");
            let total: i64 = row.get(0);
            let total_at: String = row.get(1);
            println!("all ops={total} last={total_at}");
            let devices = client
                .query("SELECT device_id, last_seen_at::text FROM _sync_devices", &[])
                .await
                .expect("devices");
            for d in devices {
                let id: String = d.get(0);
                let seen: String = d.get(1);
                println!("device {id} last_seen={seen}");
            }
        });
    }

    /// Read-only: sample a table's count twice to measure live throughput.
    #[test]
    fn feed_watch() {
        let Ok(dsn) = std::env::var("VRCX_PG_TEST_DSN") else {
            return;
        };
        let table =
            std::env::var("VRCX_PG_WATCH_TABLE").unwrap_or_else(|_| "gamelog_location".into());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let client = diag_client(&dsn).await;
            let count = |t: &str| {
                let t = t.to_string();
                let client = &client;
                async move {
                    client
                        .query_one(&format!("SELECT COUNT(*) FROM \"{t}\""), &[])
                        .await
                        .map(|r| r.get::<_, i64>(0))
                        .unwrap_or(-1)
                }
            };
            let first = count(&table).await;
            std::thread::sleep(std::time::Duration::from_secs(30));
            let second = count(&table).await;
            let rate = (second - first) / 30;
            println!("{table}: {first} -> {second} (+{}/s)", rate.max(0));
        });
    }

    /// Read-only: row counts to confirm the bootstrap actually landed.
    #[test]
    fn server_counts() {
        let Ok(dsn) = std::env::var("VRCX_PG_TEST_DSN") else {
            return;
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let client = diag_client(&dsn).await;
            for table in ["sync_ops", "gamelog_location", "_sync_devices"] {
                let count: i64 = client
                    .query_one(&format!("SELECT COUNT(*) FROM {table}"), &[])
                    .await
                    .map(|r| r.get(0))
                    .unwrap_or(-1);
                println!("{table}={count}");
            }
        });
    }

    /// Heal: terminate backends whose transaction has been open for over a
    /// minute (stuck holders that deadlock everyone else). Explicitly
    /// requested via VRCX_PG_HEAL=1.
    #[test]
    fn terminate_stuck_transactions() {
        let Ok(dsn) = std::env::var("VRCX_PG_TEST_DSN") else {
            return;
        };
        if std::env::var("VRCX_PG_HEAL").ok().as_deref() != Some("1") {
            return;
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let client = diag_client(&dsn).await;
            let rows = client
                .query(
                    "SELECT pid FROM pg_stat_activity
                     WHERE datname = current_database()
                       AND pid <> pg_backend_pid()
                       AND xact_start IS NOT NULL
                       AND now() - xact_start > interval '60 seconds'",
                    &[],
                )
                .await
                .expect("select stuck");
            for row in &rows {
                let pid: i32 = row.get(0);
                let terminated: bool = client
                    .query_one("SELECT pg_terminate_backend($1)", &[&pid])
                    .await
                    .map(|r| r.get(0))
                    .unwrap_or(false);
                println!("terminated pid={pid} ok={terminated}");
            }
            println!("stuck backends found: {}", rows.len());
        });
    }
}

async fn flush_set_batch(
    transaction: &tokio_postgres::Transaction<'_>,
    schemas: &HashMap<String, Arc<RemoteTableSchema>>,
    batch: &mut Vec<&SyncOpRecord>,
) -> SyncStoreResult<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let schema = schemas.get(&batch[0].table).expect("checked above").clone();
    let sql = set_upsert_sql(&schema, batch.len());
    let mut params: Vec<Box<dyn ToSql + Sync + Send>> = Vec::new();
    for op in batch.iter() {
        let payload = op.payload.as_ref().expect("Set ops carry payloads");
        for column in &schema.columns {
            if is_counter_column(&schema, &column.name) {
                continue;
            }
            params.push(bind_value(
                column.column_type,
                payload.get(&column.name).unwrap_or(&Value::Null),
            ));
        }
        params.push(Box::new(op.hlc.clone()));
        params.push(Box::new(op.device.clone()));
    }
    let refs = params
        .iter()
        .map(|p| &**p as &(dyn ToSql + Sync))
        .collect::<Vec<_>>();
    transaction.execute(&sql, &refs).await.map_err(pg_error)?;
    batch.clear();
    Ok(())
}

fn entity_key_text(key: &[Value]) -> String {
    serde_json::to_string(key).unwrap_or_default()
}

fn is_retryable(error: &SyncStoreError) -> bool {
    let text = error.to_string();
    text.contains("deadlock detected")
        || text.contains("could not serialize access")
        || text.contains("duplicate key value violates unique constraint")
        || text.contains("due to lock timeout")
        || text.contains("terminating connection due to idle-in-transaction timeout")
}

fn pg_error(error: tokio_postgres::Error) -> SyncStoreError {
    let mut message = format!("PostgreSQL error: {error}");
    if let Some(db) = error.as_db_error() {
        message.push_str(&format!(
            " — {} (code {}{})",
            db.message(),
            db.code().code(),
            db.detail()
                .map(|detail| format!(", detail: {detail}"))
                .unwrap_or_default()
        ));
    }
    // Client-side failures (serialization, conversion) hide the real reason
    // in the source chain; surface it.
    let mut source = std::error::Error::source(&error);
    while let Some(error) = source {
        message.push_str(&format!(": {error}"));
        source = error.source();
    }
    SyncStoreError::Other(message)
}
