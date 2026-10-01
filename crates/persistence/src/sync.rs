//! Local capture and materialization for remote database sync.
//!
//! Capture is fully trigger-based: three `AFTER INSERT/UPDATE/DELETE` triggers
//! per synced table append change facts into `_sync_outbox` inside the same
//! transaction as the business write, so a crash can never strand a committed
//! row without its fact. Triggers stamp write-time HLC strings (decimal
//! milliseconds + logical zero + device id) directly in SQL, which keeps
//! last-write-wins ordering anchored to when a change actually happened
//! rather than when it was pushed.
//!
//! Pull application runs in one write transaction: ops merge into business
//! tables through per-field lattice semantics, the pull cursor advances, and
//! the capture rows produced by this very transaction (the echo) are deleted
//! by sequence watermark before commit. The single-writer design guarantees
//! nothing else wrote in between, so the watermark is exact.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use serde_json::{Map, Value};
use vrcx_0_contracts::{
    register_sync_table_owned, sync_table_descriptor, SyncFieldSemantic, SyncHlc, SyncOpKind,
    SyncOpRecord, SyncRowSemantic, SyncTableDescriptor, SYNC_EXCLUDED_CONFIG_KEYS,
};

use crate::database::DatabaseService;
use crate::Error;

pub const META_DEVICE_ID: &str = "sync.deviceId";
pub const META_PULL_CURSOR: &str = "sync.pullCursor";
pub const META_LAST_PUSH_AT: &str = "sync.lastPushAt";
pub const META_LAST_PULL_AT: &str = "sync.lastPullAt";
pub const META_BOOTSTRAP_STATE: &str = "sync.bootstrapState";
pub const META_LAST_OP_GC_AT: &str = "sync.lastOpGcAt";

/// Trigger-side HLC stamp: decimal epoch milliseconds, zero-padded to the
/// same width as `SyncHlc::encode`, plus zero logical + device. The width
/// matters — lexicographic comparison against engine-generated stamps is the
/// LWW arbitration.
const TRIGGER_HLC_SQL: &str =
    "printf('%017d', CAST(strftime('%s','now') AS INTEGER) * 1000 + CAST(substr(strftime('%f','now'),-3) AS INTEGER)) || '-00000000-'";

#[derive(Clone, Debug)]
pub struct TablePlan {
    pub descriptor: &'static SyncTableDescriptor,
    pub table: String,
    pub columns: Vec<String>,
    /// Columns that cross devices (rowid aliases excluded).
    pub payload_columns: Vec<String>,
}

struct CaptureInner {
    device: String,
    hlc: Mutex<SyncHlc>,
    plans: RwLock<HashMap<String, TablePlan>>,
}

/// Shared handle to the installed capture state; also the process-wide HLC.
#[derive(Clone)]
pub struct SyncCaptureHandle {
    inner: Arc<CaptureInner>,
}

impl SyncCaptureHandle {
    /// Stamp a new local event (used for delete-op stamping at push time).
    pub fn tick(&self) -> String {
        let mut hlc = self.inner.hlc.lock().unwrap();
        hlc.tick(now_ms());
        hlc.encode()
    }

    /// Merge a remote timestamp observed via pull.
    pub fn observe(&self, other: &SyncHlc) {
        let mut hlc = self.inner.hlc.lock().unwrap();
        hlc.observe(other, now_ms());
    }

    pub fn device_id(&self) -> &str {
        &self.inner.device
    }

    pub fn plan_for(&self, table: &str) -> Option<TablePlan> {
        self.inner.plans.read().unwrap().get(table).cloned()
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

// ────────────────────────── internal tables & meta ──────────────────────────

pub fn ensure_sync_tables(db: &DatabaseService) -> Result<(), Error> {
    db.ensure_schema_once("sync-internal", || {
        for sql in [
            "CREATE TABLE IF NOT EXISTS _sync_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL DEFAULT '')",
            "CREATE TABLE IF NOT EXISTS _sync_outbox (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                op_id TEXT NOT NULL DEFAULT '',
                table_name TEXT NOT NULL,
                entity_key TEXT NOT NULL,
                op TEXT NOT NULL,
                payload TEXT,
                hlc TEXT NOT NULL DEFAULT '',
                device TEXT NOT NULL DEFAULT '',
                stamped_ops TEXT
            )",
            "CREATE INDEX IF NOT EXISTS _sync_outbox_table_key_idx ON _sync_outbox (table_name, entity_key, hlc)",
            "CREATE TABLE IF NOT EXISTS _sync_row_version (
                table_name TEXT NOT NULL,
                entity_key TEXT NOT NULL,
                hlc TEXT NOT NULL DEFAULT '',
                snapshot TEXT,
                PRIMARY KEY (table_name, entity_key)
            )",
        ] {
            db.execute_non_query(sql, &Default::default())?;
        }
        Ok(())
    })
}

pub fn sync_meta_get(db: &DatabaseService, key: &str) -> Result<Option<String>, Error> {
    ensure_sync_tables(db)?;
    let rows = db.execute(
        "SELECT value FROM _sync_meta WHERE key = @key LIMIT 1",
        &params(&[("key", Value::String(key.to_string()))]),
    )?;
    Ok(rows
        .into_iter()
        .next()
        .and_then(|row| row.first().cloned())
        .and_then(|v| v.as_str().map(str::to_string)))
}

pub fn sync_meta_set(db: &DatabaseService, key: &str, value: &str) -> Result<(), Error> {
    ensure_sync_tables(db)?;
    db.execute_non_query(
        "INSERT INTO _sync_meta (key, value) VALUES (@key, @value)
         ON CONFLICT(key) DO UPDATE SET value = @value",
        &params(&[
            ("key", Value::String(key.to_string())),
            ("value", Value::String(value.to_string())),
        ]),
    )?;
    Ok(())
}

pub fn sync_meta_remove(db: &DatabaseService, key: &str) -> Result<(), Error> {
    ensure_sync_tables(db)?;
    db.execute_non_query(
        "DELETE FROM _sync_meta WHERE key = @key",
        &params(&[("key", Value::String(key.to_string()))]),
    )?;
    Ok(())
}

/// Stable per-database device identity; the tiebreaker in HLC ordering.
pub fn sync_device_id(db: &DatabaseService) -> Result<String, Error> {
    if let Some(existing) = sync_meta_get(db, META_DEVICE_ID)? {
        if !existing.is_empty() {
            return Ok(existing);
        }
    }
    let device = uuid::Uuid::new_v4().simple().to_string();
    sync_meta_set(db, META_DEVICE_ID, &device)?;
    Ok(device)
}

fn params(entries: &[(&str, Value)]) -> HashMap<String, Value> {
    entries
        .iter()
        .map(|(k, v)| (format!("@{k}"), v.clone()))
        .collect()
}

// ───────────────────────────── capture triggers ─────────────────────────────

fn sql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// `WHEN` guard for the configs table so credential keys never leave the device.
fn configs_when_clause(old: bool) -> String {
    let row = if old { "OLD" } else { "NEW" };
    let mut excluded = SYNC_EXCLUDED_CONFIG_KEYS
        .iter()
        .map(|k| sql_literal(k))
        .collect::<Vec<_>>();
    excluded.sort();
    format!(
        "WHEN {row}.\"key\" NOT IN ({}) AND {row}.\"value\" NOT LIKE 'enc1:%'",
        excluded.join(", ")
    )
}

fn json_array_expr(prefix: &str, columns: &[&str]) -> String {
    let items = columns
        .iter()
        .map(|c| format!("{prefix}.\"{c}\""))
        .collect::<Vec<_>>();
    format!("json_array({})", items.join(", "))
}

fn json_object_expr(prefix: &str, columns: &[String]) -> String {
    let items = columns
        .iter()
        .map(|c| format!("'{}', {prefix}.\"{c}\"", c.replace('\'', "''")))
        .collect::<Vec<_>>();
    format!("json_object({})", items.join(", "))
}

fn trigger_sqls(table: &str, plan: &TablePlan, device: &str) -> Vec<String> {
    let device_lit = sql_literal(device);
    let key_columns: Vec<&str> = plan.descriptor.key_columns.to_vec();
    let key_expr_new = json_array_expr("NEW", &key_columns);
    let key_expr_old = json_array_expr("OLD", &key_columns);
    let payload_expr_new = json_object_expr("NEW", &plan.payload_columns);
    let table_lit = sql_literal(table);
    let is_configs = table == "configs";

    let mut sqls = Vec::with_capacity(3);
    for (action, event, key_expr) in [
        ("ins", "INSERT", &key_expr_new),
        ("upd", "UPDATE", &key_expr_new),
    ] {
        let when = if is_configs {
            configs_when_clause(false)
        } else {
            String::new()
        };
        sqls.push(format!(
            "CREATE TRIGGER IF NOT EXISTS \"_sync_{action}_{table}\" AFTER {event} ON \"{table}\" {when} BEGIN
                INSERT INTO _sync_outbox (op_id, table_name, entity_key, op, payload, hlc, device)
                VALUES ('', {table_lit}, {key_expr}, 'set', {payload_expr_new}, {TRIGGER_HLC_SQL} || {device_lit}, {device_lit});
            END"
        ));
    }
    let delete_when = if is_configs {
        configs_when_clause(true)
    } else {
        String::new()
    };
    sqls.push(format!(
        "CREATE TRIGGER IF NOT EXISTS \"_sync_del_{table}\" AFTER DELETE ON \"{table}\" {delete_when} BEGIN
            INSERT INTO _sync_outbox (op_id, table_name, entity_key, op, payload, hlc, device)
            VALUES ('', {table_lit}, {key_expr_old}, 'del', NULL, {TRIGGER_HLC_SQL} || {device_lit}, {device_lit});
        END"
    ));
    sqls
}

fn drop_trigger_sqls(table: &str) -> Vec<String> {
    ["ins", "upd", "del"]
        .iter()
        .map(|action| format!("DROP TRIGGER IF EXISTS \"_sync_{action}_{table}\""))
        .collect()
}

fn table_exists(db: &DatabaseService, table: &str) -> Result<bool, Error> {
    let rows = db.execute(
        "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = @name LIMIT 1",
        &params(&[("name", Value::String(table.to_string()))]),
    )?;
    Ok(!rows.is_empty())
}

/// Business columns whose values cross devices. Excludes rowid-alias columns
/// (`id INTEGER PRIMARY KEY`): the natural key identifies the row, and local
/// rowids are private to each database.
pub fn table_payload_column_types(
    db: &DatabaseService,
    table: &str,
) -> Result<Vec<(String, String)>, Error> {
    let rows = db.execute(
        &format!("PRAGMA table_info(\"{table}\")"),
        &Default::default(),
    )?;
    let pk_columns = rows
        .iter()
        .filter(|row| row.get(5).and_then(Value::as_i64).unwrap_or(0) > 0)
        .count();
    Ok(rows
        .iter()
        .filter(|row| {
            let declared = row
                .get(2)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_uppercase();
            let is_pk = row.get(5).and_then(Value::as_i64).unwrap_or(0) > 0;
            // A single INTEGER primary key is a rowid alias.
            !(is_pk && pk_columns == 1 && declared == "INTEGER")
        })
        .filter_map(|row| {
            let name = row.get(1).and_then(Value::as_str).map(str::to_string)?;
            let declared = row
                .get(2)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Some((name, declared))
        })
        .collect())
}

fn table_columns(db: &DatabaseService, table: &str) -> Result<Vec<String>, Error> {
    let rows = db.execute(
        &format!("PRAGMA table_info(\"{table}\")"),
        &Default::default(),
    )?;
    Ok(rows
        .iter()
        .filter_map(|row| row.get(1).and_then(Value::as_str).map(str::to_string))
        .collect())
}

/// Enumerate physical tables present locally that match the sync catalog.
pub fn synced_local_tables(db: &DatabaseService) -> Result<Vec<TablePlan>, Error> {
    let rows = db.execute(
        "SELECT name FROM sqlite_schema WHERE type = 'table'",
        &Default::default(),
    )?;
    let mut plans = Vec::new();
    for name in rows
        .into_iter()
        .filter_map(|row| row.first().and_then(Value::as_str).map(str::to_string))
    {
        if let Some(descriptor) = sync_table_descriptor(&name) {
            let columns = table_columns(db, &name)?;
            let payload_columns = table_payload_column_types(db, &name)?
                .into_iter()
                .map(|(column, _)| column)
                .collect();
            plans.push(TablePlan {
                descriptor,
                table: name,
                columns,
                payload_columns,
            });
        }
    }
    Ok(plans)
}

/// Register one table for capture: build its plan and install triggers.
/// Idempotent; also recreates triggers after a device id change.
pub fn register_sync_table(
    db: &DatabaseService,
    table: &str,
    device: &str,
) -> Result<Option<TablePlan>, Error> {
    let Some(descriptor) = sync_table_descriptor(table) else {
        return Ok(None);
    };
    if !table_exists(db, table)? {
        return Ok(None);
    }
    let columns = table_columns(db, table)?;
    let payload_columns = table_payload_column_types(db, table)?
        .into_iter()
        .map(|(column, _)| column)
        .collect();
    let plan = TablePlan {
        descriptor,
        table: table.to_string(),
        columns,
        payload_columns,
    };
    for sql in drop_trigger_sqls(table) {
        db.execute_non_query(&sql, &Default::default())?;
    }
    for sql in trigger_sqls(table, &plan, device) {
        db.execute_non_query(&sql, &Default::default())?;
    }
    Ok(Some(plan))
}

/// Per-user tables are created lazily at login; when pulling ops for a table
/// this device has never instantiated, create it before applying.
fn ensure_synced_table_exists(db: &DatabaseService, table: &str) -> Result<(), Error> {
    if table_exists(db, table)? {
        return Ok(());
    }
    let Some(descriptor) = sync_table_descriptor(table) else {
        return Ok(());
    };
    if descriptor.per_user {
        let prefix = table
            .strip_suffix(descriptor.template)
            .unwrap_or_default()
            .to_string();
        if !prefix.is_empty() {
            crate::database::schema::ensure_user_store_tables(db, &prefix)?;
        }
    }
    Ok(())
}

// ───────────────────── schema evolution (derive/drift) ─────────────────────

/// Tables that must never sync even though they are not in the static
/// catalog: credentials, re-derivable caches, and per-device read models.
const DERIVATION_EXCLUDED_TABLES: [&str; 2] = ["cookies", "favorite_print"];
const DERIVATION_EXCLUDED_PREFIXES: [&str; 4] = ["cache_", "screenshot_", "_sync_", "sqlite_"];
const DERIVATION_EXCLUDED_SUFFIXES: [&str; 5] = [
    "_activity_sessions_v2",
    "_activity_bucket_cache_v2",
    "_activity_page_cache",
    "_activity_sync_state_v2",
    "_avatar_history",
];

fn is_derivation_excluded(table: &str) -> bool {
    DERIVATION_EXCLUDED_TABLES.contains(&table)
        || DERIVATION_EXCLUDED_PREFIXES
            .iter()
            .any(|prefix| table.starts_with(prefix))
        || DERIVATION_EXCLUDED_SUFFIXES
            .iter()
            .any(|suffix| table.ends_with(suffix))
}

/// Detect the natural key: declared PRIMARY KEY (minus rowid aliases),
/// then the first UNIQUE index, then every payload column (content
/// addressing for append-only facts without any declared key).
fn derive_key_columns(
    db: &DatabaseService,
    table: &str,
    payload_columns: &[(String, String)],
) -> Result<Vec<String>, Error> {
    let info = db.execute(
        &format!("PRAGMA table_info(\"{table}\")"),
        &Default::default(),
    )?;
    let mut pk_columns: Vec<(i64, String)> = info
        .iter()
        .filter_map(|row| {
            let position = row.get(5).and_then(Value::as_i64).unwrap_or(0);
            let name = row.get(1).and_then(Value::as_str).map(str::to_string)?;
            Some((position, name))
        })
        .filter(|(position, _)| *position > 0)
        .collect();
    pk_columns.sort();
    let declared_type = |name: &str| {
        info.iter()
            .find(|row| row.get(1).and_then(Value::as_str) == Some(name))
            .and_then(|row| row.get(2).and_then(Value::as_str))
            .unwrap_or_default()
            .to_ascii_uppercase()
    };
    // A single INTEGER primary key is a rowid alias (device-local); skip it.
    let pk_usable = !(pk_columns.len() == 1
        && pk_columns[0].1 == "id"
        && declared_type(&pk_columns[0].1) == "INTEGER");
    if !pk_columns.is_empty() && pk_usable {
        return Ok(pk_columns.into_iter().map(|(_, name)| name).collect());
    }

    let indexes = db.execute(
        &format!("PRAGMA index_list(\"{table}\")"),
        &Default::default(),
    )?;
    let mut unique_indexes: Vec<(i64, String)> = indexes
        .iter()
        .filter_map(|row| {
            let unique = row.get(2).and_then(Value::as_i64).unwrap_or(0) == 1;
            let name = row.get(1).and_then(Value::as_str).map(str::to_string)?;
            let seq = row.first().and_then(Value::as_i64).unwrap_or(0);
            Some((seq, name)).filter(|_| unique)
        })
        .collect();
    unique_indexes.sort();
    for (_, index_name) in unique_indexes {
        let columns = db.execute(
            &format!("PRAGMA index_info(\"{index_name}\")"),
            &Default::default(),
        )?;
        let names: Vec<String> = columns
            .iter()
            .filter_map(|row| row.get(2).and_then(Value::as_str).map(str::to_string))
            .collect();
        if !names.is_empty()
            && names
                .iter()
                .all(|name| payload_columns.iter().any(|(column, _)| column == name))
        {
            return Ok(names);
        }
    }

    Ok(payload_columns
        .iter()
        .map(|(column, _)| column.clone())
        .collect())
}

/// Derive a descriptor for a table that is neither cataloged nor registered,
/// then register it. New app tables therefore sync automatically with safe
/// defaults (LWW rows; boolean OR only for the well-known `seen`/`expired`
/// flag names) instead of being silently missed.
pub fn derive_and_register_sync_table(
    db: &DatabaseService,
    table: &str,
    per_user_prefixes: &[String],
) -> Result<Option<&'static SyncTableDescriptor>, Error> {
    if is_derivation_excluded(table) {
        return Ok(None);
    }
    let payload_columns = table_payload_column_types(db, table)?;
    if payload_columns.is_empty() {
        return Ok(None);
    }
    let key_columns = derive_key_columns(db, table, &payload_columns)?;

    // Per-user detection: strip a known user prefix to get the template.
    let (per_user, template) = per_user_prefixes
        .iter()
        .find_map(|prefix| {
            table
                .strip_prefix(prefix.as_str())
                .filter(|rest| rest.starts_with('_'))
                .map(|rest| (true, rest.to_string()))
        })
        .unwrap_or((false, table.to_string()));

    let field_semantics = payload_columns
        .iter()
        .map(|(column, _)| column.as_str())
        .filter(|column| matches!(*column, "seen" | "expired"))
        .map(|column| (column.to_string(), SyncFieldSemantic::OrBool))
        .collect::<Vec<_>>();

    register_sync_table_owned(
        template,
        per_user,
        key_columns,
        SyncRowSemantic::Lww,
        field_semantics,
    );
    Ok(sync_table_descriptor(table))
}

/// User prefixes observed on already-known per-user tables; used to classify
/// unknown tables that share the prefix.
fn observed_per_user_prefixes(db: &DatabaseService) -> Result<Vec<String>, Error> {
    let tables = db.execute(
        "SELECT name FROM sqlite_schema WHERE type = 'table'",
        &Default::default(),
    )?;
    let mut prefixes = Vec::new();
    for table in tables
        .into_iter()
        .filter_map(|row| row.first().and_then(Value::as_str).map(str::to_string))
    {
        for descriptor in vrcx_0_contracts::SYNC_TABLE_CATALOG {
            if !descriptor.per_user {
                continue;
            }
            if let Some(prefix) = table.strip_suffix(descriptor.template) {
                if vrcx_0_contracts::looks_like_user_prefix(prefix)
                    && !prefixes.contains(&prefix.to_string())
                {
                    prefixes.push(prefix.to_string());
                }
            }
        }
    }
    Ok(prefixes)
}

// ─────────────────────────── capture install/teardown ───────────────────────

pub fn install_capture(db: &DatabaseService) -> Result<SyncCaptureHandle, Error> {
    ensure_sync_tables(db)?;
    let device = sync_device_id(db)?;
    let inner = CaptureInner {
        device: device.clone(),
        hlc: Mutex::new(SyncHlc::new(now_ms(), device.clone())),
        plans: RwLock::new(HashMap::new()),
    };
    let handle = SyncCaptureHandle {
        inner: Arc::new(inner),
    };
    refresh_capture(db, &handle)?;
    Ok(handle)
}

/// (Re)scan local tables and keep capture in sync with schema evolution:
/// - new tables (per-user at login, or shipped by an app update) are derived
///   or matched against the catalog and start capturing;
/// - column additions rebuild the capture triggers so payloads carry the new
///   column (the remote schema follows via the engine's schema-hash check).
pub fn refresh_capture(db: &DatabaseService, handle: &SyncCaptureHandle) -> Result<(), Error> {
    let tables = db.execute(
        "SELECT name FROM sqlite_schema WHERE type = 'table'",
        &Default::default(),
    )?;
    let names = tables
        .into_iter()
        .filter_map(|row| row.first().and_then(Value::as_str).map(str::to_string))
        .filter(|name| crate::database::schema::safe_identifier(name, "Table name").is_ok())
        .collect::<Vec<_>>();

    {
        // Existing plans first: detect column drift before adding tables.
        let plans = handle.inner.plans.read().unwrap();
        let drifted = names
            .iter()
            .filter(|table| plans.contains_key(table.as_str()))
            .filter_map(|table| {
                let plan = plans.get(table.as_str())?;
                let current = table_payload_column_types(db, table).ok()?;
                let current_names: Vec<String> =
                    current.into_iter().map(|(column, _)| column).collect();
                (current_names != plan.payload_columns).then(|| table.clone())
            })
            .collect::<Vec<String>>();
        drop(plans);
        for table in drifted {
            let Some(descriptor) = sync_table_descriptor(&table) else {
                continue;
            };
            let columns = table_columns(db, &table)?;
            let payload_columns = table_payload_column_types(db, &table)?
                .into_iter()
                .map(|(column, _)| column)
                .collect();
            for sql in drop_trigger_sqls(&table) {
                db.execute_non_query(&sql, &Default::default())?;
            }
            let plan = TablePlan {
                descriptor,
                table: table.clone(),
                columns,
                payload_columns,
            };
            for sql in trigger_sqls(&table, &plan, &handle.inner.device) {
                db.execute_non_query(&sql, &Default::default())?;
            }
            handle.inner.plans.write().unwrap().insert(table, plan);
        }
    }

    // New tables: catalog match, explicit registration, or derivation.
    let prefixes = observed_per_user_prefixes(db)?;
    for table in names {
        if handle.inner.plans.read().unwrap().contains_key(&table) {
            continue;
        }
        if sync_table_descriptor(&table).is_none() {
            let _ = derive_and_register_sync_table(db, &table, &prefixes)?;
        }
        let registered = register_sync_table(db, &table, &handle.inner.device)?;
        if let Some(registered) = registered {
            handle
                .inner
                .plans
                .write()
                .unwrap()
                .insert(registered.table.clone(), registered);
        }
    }
    Ok(())
}

pub fn uninstall_capture(db: &DatabaseService, handle: &SyncCaptureHandle) -> Result<(), Error> {
    let tables: Vec<String> = handle.inner.plans.read().unwrap().keys().cloned().collect();
    for table in tables {
        for sql in drop_trigger_sqls(&table) {
            db.execute_non_query(&sql, &Default::default())?;
        }
    }
    handle.inner.plans.write().unwrap().clear();
    Ok(())
}

// ─────────────────────────────── push staging ───────────────────────────────

pub struct OutboxBatch {
    pub ops: Vec<SyncOpRecord>,
    pub max_seq: i64,
}

pub fn outbox_pending_count(db: &DatabaseService) -> Result<i64, Error> {
    ensure_sync_tables(db)?;
    let rows = db.execute("SELECT COUNT(*) FROM _sync_outbox", &Default::default())?;
    Ok(rows
        .first()
        .and_then(|row| row.first())
        .and_then(Value::as_i64)
        .unwrap_or(0))
}

fn entity_key_text(key: &[Value]) -> String {
    serde_json::to_string(key).unwrap_or_else(|_| "[]".into())
}

/// Stage up to `limit` pending facts for pushing.
///
/// Delete facts get their HLC and op id stamped here (trigger stamps are
/// unavailable for deletes because the row is gone). Tables carrying
/// `CounterDelta`/`ElementSet` fields are translated into `Inc`/`SetAdd`/
/// `SetRemove` ops by diffing the row against the last synced snapshot, so
/// concurrent increments on several devices sum instead of overwriting.
/// Derived ops are serialized back into the outbox row (`stamped_ops`), which
/// makes retries byte-identical and therefore exactly-once at the remote.
pub fn outbox_take(
    db: &DatabaseService,
    handle: &SyncCaptureHandle,
    limit: i64,
) -> Result<Option<OutboxBatch>, Error> {
    ensure_sync_tables(db)?;
    refresh_capture(db, handle)?;
    db.write_transaction(move |tx| {
        let rows = tx.execute(
            "SELECT seq, table_name, entity_key, op, payload, hlc, device, stamped_ops
             FROM _sync_outbox ORDER BY seq LIMIT @limit",
            &params(&[("limit", Value::from(limit))]),
        )?;
        if rows.is_empty() {
            return Ok(None);
        }

        #[derive(Default)]
        struct Staged {
            seq: i64,
            table: String,
            entity_text: String,
            ops: Vec<SyncOpRecord>,
        }

        let mut staged: Vec<Staged> = Vec::new();
        for row in &rows {
            let seq = row[0].as_i64().unwrap_or(0);
            let table = row[1].as_str().unwrap_or_default().to_string();
            let entity_text = row[2].as_str().unwrap_or_default().to_string();
            let op = row[3].as_str().unwrap_or_default().to_string();
            let hlc = row[5].as_str().unwrap_or_default().to_string();
            let device = row[6].as_str().unwrap_or_default().to_string();
            let stamped = row[7].as_str().map(str::to_string);

            if table.is_empty() || sync_table_descriptor(&table).is_none() {
                continue;
            }

            if let Some(text) = stamped {
                if let Ok(ops) = serde_json::from_str::<Vec<SyncOpRecord>>(&text) {
                    staged.push(Staged {
                        seq,
                        table,
                        entity_text,
                        ops,
                    });
                    continue;
                }
            }

            // Advance the row watermark when staging a local fact: after the
            // op is pushed and trimmed, the watermark is what keeps older
            // pulled ops from overwriting this device's newer local write.
            let advance_watermark = |tx: &crate::database::DatabaseWriteTransaction<'_>,
                                     hlc: &str|
             -> Result<(), Error> {
                tx.execute_non_query(
                    "INSERT INTO _sync_row_version (table_name, entity_key, hlc, snapshot)
                     VALUES (@t, @k, @h, '')
                     ON CONFLICT(table_name, entity_key) DO UPDATE SET
                        hlc = CASE WHEN @h > hlc THEN @h ELSE hlc END",
                    &params(&[
                        ("t", Value::String(table.clone())),
                        ("k", Value::String(entity_text.clone())),
                        ("h", Value::String(hlc.to_string())),
                    ]),
                )?;
                Ok(())
            };

            let entity_key: Vec<Value> = serde_json::from_str(&entity_text).unwrap_or_default();
            let payload: Option<Map<String, Value>> = row[4]
                .as_str()
                .and_then(|text| serde_json::from_str(text).ok());

            let ops = if op == "del" {
                let stamp = handle.tick();
                advance_watermark(tx, &stamp)?;
                vec![SyncOpRecord {
                    op_id: format!("{stamp}/{seq}"),
                    table: table.clone(),
                    entity_key,
                    kind: SyncOpKind::Delete,
                    payload: None,
                    hlc: stamp,
                    device: device.clone(),
                }]
            } else {
                if !hlc.is_empty() {
                    advance_watermark(tx, &hlc)?;
                }
                derive_set_ops(
                    tx,
                    seq,
                    &table,
                    entity_text.clone(),
                    entity_key,
                    payload,
                    &hlc,
                    &device,
                )?
            };

            staged.push(Staged {
                seq,
                table,
                entity_text,
                ops,
            });
        }

        // Compact superseded full-row Set ops for the same key: the payload is
        // a complete row snapshot, so only the newest one carries information.
        // Never compact across a Delete/Inc/element op for the same key.
        let mut compacted: Vec<Staged> = Vec::with_capacity(staged.len());
        for item in staged {
            if item.ops.len() == 1 && item.ops[0].kind == SyncOpKind::Set {
                if let Some(index) = compacted.iter().rposition(|prev| {
                    prev.table == item.table
                        && prev.entity_text == item.entity_text
                        && prev.ops.len() == 1
                        && prev.ops[0].kind == SyncOpKind::Set
                }) {
                    // Only fold when no other op for this key came in between.
                    let blocked = compacted[index + 1..]
                        .iter()
                        .any(|mid| mid.table == item.table && mid.entity_text == item.entity_text);
                    if !blocked {
                        compacted.remove(index);
                    }
                }
            }
            compacted.push(item);
        }

        // Persist stamps and snapshots, then flatten.
        let mut ops = Vec::new();
        let mut max_seq = 0;
        for item in &compacted {
            max_seq = max_seq.max(item.seq);
            let encoded = serde_json::to_string(&item.ops).unwrap_or_else(|_| "[]".into());
            tx.execute_non_query(
                "UPDATE _sync_outbox SET stamped_ops = @ops WHERE seq = @seq",
                &params(&[
                    ("ops", Value::String(encoded)),
                    ("seq", Value::from(item.seq)),
                ]),
            )?;
            ops.extend(item.ops.iter().cloned());
        }
        Ok(Some(OutboxBatch { ops, max_seq }))
    })
}

fn derive_set_ops(
    tx: &crate::database::DatabaseWriteTransaction<'_>,
    seq: i64,
    table: &str,
    entity_text: String,
    entity_key: Vec<Value>,
    payload: Option<Map<String, Value>>,
    hlc: &str,
    device: &str,
) -> Result<Vec<SyncOpRecord>, Error> {
    let Some(payload) = payload else {
        return Ok(Vec::new());
    };
    let descriptor = sync_table_descriptor(table).expect("checked above");
    let has_special = descriptor.field_semantics.iter().any(|(_, s)| {
        matches!(
            s,
            SyncFieldSemantic::CounterDelta | SyncFieldSemantic::ElementSet
        )
    });

    let base = SyncOpRecord {
        op_id: format!("{hlc}/{seq}"),
        table: table.to_string(),
        entity_key: entity_key.clone(),
        kind: SyncOpKind::Set,
        payload: Some(payload.clone()),
        hlc: hlc.to_string(),
        device: device.to_string(),
    };

    let mut ops = Vec::new();
    if !has_special {
        ops.push(base);
        return Ok(ops);
    }

    // Diff against the last synced snapshot for counter/element fields.
    let snapshot_row = tx.execute(
        "SELECT snapshot FROM _sync_row_version WHERE table_name = @t AND entity_key = @k LIMIT 1",
        &params(&[
            ("t", Value::String(table.to_string())),
            ("k", Value::String(entity_text.clone())),
        ]),
    )?;
    let snapshot: Option<Map<String, Value>> = snapshot_row
        .first()
        .and_then(|row| row.first())
        .and_then(Value::as_str)
        .and_then(|text| serde_json::from_str(text).ok());

    let record_snapshot = |tx: &crate::database::DatabaseWriteTransaction<'_>,
                           snapshot: &Map<String, Value>|
     -> Result<(), Error> {
        tx.execute_non_query(
            "INSERT INTO _sync_row_version (table_name, entity_key, hlc, snapshot)
             VALUES (@t, @k, @h, @s)
             ON CONFLICT(table_name, entity_key) DO UPDATE SET
                hlc = CASE WHEN @h > hlc THEN @h ELSE hlc END,
                snapshot = @s",
            &params(&[
                ("t", Value::String(table.to_string())),
                ("k", Value::String(entity_text.clone())),
                ("h", Value::String(hlc.to_string())),
                (
                    "s",
                    Value::String(serde_json::to_string(snapshot).unwrap_or_default()),
                ),
            ]),
        )?;
        Ok(())
    };

    let Some(snapshot) = snapshot else {
        // First sync of this row. Counter fields travel as increment facts,
        // never absolute values: independently seeded histories then sum
        // exactly instead of racing two absolute snapshots against each other.
        // The Set precedes its increments so replicas that never saw the row
        // create it before applying deltas.
        let mut set_payload = payload.clone();
        let mut counter_facts = Vec::new();
        for (field, semantic) in descriptor.field_semantics {
            if *semantic != SyncFieldSemantic::CounterDelta {
                continue;
            }
            if let Some(value) = set_payload.remove(*field) {
                let current = value.as_i64().unwrap_or(0);
                if current > 0 {
                    counter_facts.push(SyncOpRecord {
                        op_id: format!("{hlc}/{seq}/first{current}"),
                        table: table.to_string(),
                        entity_key: entity_key.clone(),
                        kind: SyncOpKind::Inc,
                        payload: Some(Map::from_iter([
                            ("field".to_string(), Value::String(field.to_string())),
                            ("delta".to_string(), Value::from(current)),
                        ])),
                        hlc: hlc.to_string(),
                        device: device.to_string(),
                    });
                }
            }
        }
        ops.push(SyncOpRecord {
            op_id: format!("{hlc}/{seq}"),
            table: table.to_string(),
            entity_key: entity_key.clone(),
            kind: SyncOpKind::Set,
            payload: Some(set_payload),
            hlc: hlc.to_string(),
            device: device.to_string(),
        });
        ops.extend(counter_facts);
        record_snapshot(tx, &payload)?;
        return Ok(ops);
    };

    let mut special_ops: Vec<SyncOpRecord> = Vec::new();
    let mut plain_columns_changed = false;
    let special_fields: Vec<&str> = descriptor
        .field_semantics
        .iter()
        .filter(|(_, s)| {
            matches!(
                s,
                SyncFieldSemantic::CounterDelta | SyncFieldSemantic::ElementSet
            )
        })
        .map(|(f, _)| *f)
        .collect();

    for (field, value) in &payload {
        if !special_fields.contains(&field.as_str()) {
            if snapshot.get(field) != Some(value) {
                plain_columns_changed = true;
            }
            continue;
        }
        let old = snapshot.get(field);
        match descriptor
            .field_semantics
            .iter()
            .find(|(f, _)| f == field)
            .map(|(_, s)| s)
        {
            Some(SyncFieldSemantic::CounterDelta) => {
                let current = value.as_i64().unwrap_or(0);
                let previous = old.and_then(Value::as_i64).unwrap_or(0);
                let delta = current - previous;
                if delta > 0 {
                    special_ops.push(SyncOpRecord {
                        op_id: format!("{hlc}/{seq}/inc{delta}"),
                        table: table.to_string(),
                        entity_key: base.entity_key.clone(),
                        kind: SyncOpKind::Inc,
                        payload: Some(Map::from_iter([
                            ("field".to_string(), Value::String(field.clone())),
                            ("delta".to_string(), Value::from(delta)),
                        ])),
                        hlc: hlc.to_string(),
                        device: device.to_string(),
                    });
                } else if delta < 0 {
                    plain_columns_changed = true; // counter reset: fall back to LWW snapshot
                }
            }
            Some(SyncFieldSemantic::ElementSet) => {
                let current_list = value.as_array().cloned().unwrap_or_default();
                let previous_list = old.and_then(Value::as_array).cloned().unwrap_or_default();
                for element in &current_list {
                    if !previous_list.contains(element) {
                        special_ops.push(SyncOpRecord {
                            op_id: format!("{hlc}/{seq}/add"),
                            table: table.to_string(),
                            entity_key: base.entity_key.clone(),
                            kind: SyncOpKind::SetAdd,
                            payload: Some(Map::from_iter([
                                ("field".to_string(), Value::String(field.clone())),
                                ("element".to_string(), element.clone()),
                            ])),
                            hlc: hlc.to_string(),
                            device: device.to_string(),
                        });
                    }
                }
                for element in &previous_list {
                    if !current_list.contains(element) {
                        special_ops.push(SyncOpRecord {
                            op_id: format!("{hlc}/{seq}/rm"),
                            table: table.to_string(),
                            entity_key: base.entity_key.clone(),
                            kind: SyncOpKind::SetRemove,
                            payload: Some(Map::from_iter([
                                ("field".to_string(), Value::String(field.clone())),
                                ("element".to_string(), element.clone()),
                            ])),
                            hlc: hlc.to_string(),
                            device: device.to_string(),
                        });
                    }
                }
            }
            _ => {
                if snapshot.get(field) != Some(value) {
                    plain_columns_changed = true;
                }
            }
        }
    }

    if plain_columns_changed || special_ops.is_empty() {
        // Non-special columns changed (or nothing special moved): push only
        // the full snapshot. The snapshot already includes any counter
        // movement, so emitting deltas too would double-count; the recorded
        // snapshot makes the next take compute fresh deltas from here.
        ops.push(base);
    } else {
        // Only counter/element fields moved: deltas carry the change.
        ops.extend(special_ops);
    }
    record_snapshot(tx, &payload)?;
    Ok(ops)
}

/// Drop pushed facts after the remote acknowledged the batch.
pub fn outbox_trim_pushed(db: &DatabaseService, max_seq: i64) -> Result<(), Error> {
    db.write_transaction(move |tx| {
        tx.execute_non_query(
            "DELETE FROM _sync_outbox WHERE seq <= @max",
            &params(&[("max", Value::from(max_seq))]),
        )?;
        tx.execute_non_query(
            "INSERT INTO _sync_meta (key, value) VALUES (@k, @v)
             ON CONFLICT(key) DO UPDATE SET value = @v",
            &params(&[
                ("k", Value::String(META_LAST_PUSH_AT.to_string())),
                ("v", Value::String(now_iso())),
            ]),
        )?;
        Ok(())
    })
}

// ────────────────────────────── pull application ────────────────────────────

#[derive(Clone, Copy, Debug, Default)]
pub struct ApplyStats {
    pub applied: u64,
    pub skipped_self: u64,
    pub skipped_stale: u64,
    pub skipped_pending: u64,
    pub skipped_missing: u64,
}

/// Apply one batch of pulled ops (in remote arrival order) and advance the
/// pull cursor atomically. The transaction deletes its own echo (capture rows
/// with seq above the pre-transaction watermark) before commit.
pub fn apply_pulled_ops(
    db: &DatabaseService,
    device_id: &str,
    ops: &[SyncOpRecord],
    new_cursor: i64,
) -> Result<ApplyStats, Error> {
    // Ensure tables referenced by the batch exist (per-user lazily created).
    let mut tables: Vec<&str> = Vec::new();
    for op in ops {
        if !tables.contains(&op.table.as_str()) {
            tables.push(&op.table);
        }
    }
    let mut plans: HashMap<String, Vec<String>> = HashMap::new();
    for table in tables {
        ensure_synced_table_exists(db, table)?;
        let columns = table_columns(db, table)?;
        plans.insert(table.to_string(), columns);
    }

    db.write_transaction(move |tx| {
        let watermark_rows = tx.execute(
            "SELECT COALESCE(MAX(seq), 0) FROM _sync_outbox",
            &Default::default(),
        )?;
        let watermark = watermark_rows
            .first()
            .and_then(|row| row.first())
            .and_then(Value::as_i64)
            .unwrap_or(0);

        let mut stats = ApplyStats::default();
        for op in ops {
            if op.device == device_id {
                stats.skipped_self += 1;
                continue;
            }
            let Some(columns) = plans.get(&op.table).cloned() else {
                stats.skipped_missing += 1;
                continue;
            };
            let Some(descriptor) = sync_table_descriptor(&op.table) else {
                stats.skipped_missing += 1;
                continue;
            };
            let entity_text = entity_key_text(&op.entity_key);

            let applied = match op.kind {
                SyncOpKind::Delete => apply_delete(
                    tx,
                    descriptor,
                    &op.table,
                    &columns,
                    &op.entity_key,
                    &entity_text,
                    &op.hlc,
                )?,
                SyncOpKind::Set => {
                    apply_set(tx, descriptor, &op.table, &columns, op, &entity_text)?
                }
                SyncOpKind::Inc => {
                    apply_inc(tx, descriptor, &op.table, &columns, op, &entity_text)?
                }
                SyncOpKind::SetAdd | SyncOpKind::SetRemove => {
                    apply_element(tx, &op.table, &columns, op, &entity_text)?
                }
            };
            match applied {
                ApplyOutcome::Applied => stats.applied += 1,
                ApplyOutcome::SkippedPending => stats.skipped_pending += 1,
                ApplyOutcome::SkippedStale => stats.skipped_stale += 1,
                ApplyOutcome::SkippedMissing => stats.skipped_missing += 1,
            }
        }

        tx.execute_non_query(
            "DELETE FROM _sync_outbox WHERE seq > @watermark",
            &params(&[("watermark", Value::from(watermark))]),
        )?;
        tx.execute_non_query(
            "INSERT INTO _sync_meta (key, value) VALUES (@k, @v)
             ON CONFLICT(key) DO UPDATE SET value = @v",
            &params(&[
                ("k", Value::String(META_PULL_CURSOR.to_string())),
                ("v", Value::String(new_cursor.to_string())),
            ]),
        )?;
        tx.execute_non_query(
            "INSERT INTO _sync_meta (key, value) VALUES (@k, @v)
             ON CONFLICT(key) DO UPDATE SET value = @v",
            &params(&[
                ("k", Value::String(META_LAST_PULL_AT.to_string())),
                ("v", Value::String(now_iso())),
            ]),
        )?;
        Ok(stats)
    })
}

enum ApplyOutcome {
    Applied,
    SkippedPending,
    SkippedStale,
    SkippedMissing,
}

/// A local pending fact with a newer-or-equal HLC wins without applying.
fn has_newer_pending(
    tx: &crate::database::DatabaseWriteTransaction<'_>,
    table: &str,
    entity_text: &str,
    op_hlc: &str,
) -> Result<bool, Error> {
    let rows = tx.execute(
        "SELECT 1 FROM _sync_outbox
         WHERE table_name = @t AND entity_key = @k AND hlc >= @h AND op IN ('set', 'del') LIMIT 1",
        &params(&[
            ("t", Value::String(table.to_string())),
            ("k", Value::String(entity_text.to_string())),
            ("h", Value::String(op_hlc.to_string())),
        ]),
    )?;
    Ok(!rows.is_empty())
}

fn row_version(
    tx: &crate::database::DatabaseWriteTransaction<'_>,
    table: &str,
    entity_text: &str,
) -> Result<Option<String>, Error> {
    let rows = tx.execute(
        "SELECT hlc FROM _sync_row_version WHERE table_name = @t AND entity_key = @k LIMIT 1",
        &params(&[
            ("t", Value::String(table.to_string())),
            ("k", Value::String(entity_text.to_string())),
        ]),
    )?;
    Ok(rows
        .first()
        .and_then(|row| row.first())
        .and_then(Value::as_str)
        .map(str::to_string))
}

fn set_row_version(
    tx: &crate::database::DatabaseWriteTransaction<'_>,
    table: &str,
    entity_text: &str,
    hlc: &str,
    snapshot: Option<&Map<String, Value>>,
) -> Result<(), Error> {
    let snapshot_text = snapshot
        .map(|s| serde_json::to_string(s).unwrap_or_default())
        .unwrap_or_default();
    // The HLC watermark only ever advances: counter and element ops push it
    // forward too, which prevents an older full-row Set from regressing a
    // counter that already merged deltas from several devices.
    tx.execute_non_query(
        "INSERT INTO _sync_row_version (table_name, entity_key, hlc, snapshot)
         VALUES (@t, @k, @h, @s)
         ON CONFLICT(table_name, entity_key) DO UPDATE SET
            hlc = CASE WHEN @h > hlc THEN @h ELSE hlc END,
            snapshot = COALESCE(NULLIF(@s, ''), snapshot)",
        &params(&[
            ("t", Value::String(table.to_string())),
            ("k", Value::String(entity_text.to_string())),
            ("h", Value::String(hlc.to_string())),
            ("s", Value::String(snapshot_text)),
        ]),
    )?;
    Ok(())
}

fn find_rowid(
    tx: &crate::database::DatabaseWriteTransaction<'_>,
    descriptor: &SyncTableDescriptor,
    table: &str,
    entity_key: &[Value],
) -> Result<Option<i64>, Error> {
    let mut clauses = Vec::new();
    let mut args: HashMap<String, Value> = HashMap::new();
    for (index, column) in descriptor.key_columns.iter().enumerate() {
        clauses.push(format!("\"{column}\" IS @k{index}"));
        args.insert(
            format!("@k{index}"),
            entity_key.get(index).cloned().unwrap_or(Value::Null),
        );
    }
    let rows = tx.execute(
        &format!(
            "SELECT rowid FROM \"{table}\" WHERE {} LIMIT 1",
            clauses.join(" AND ")
        ),
        &args,
    )?;
    Ok(rows
        .first()
        .and_then(|row| row.first())
        .and_then(Value::as_i64))
}

fn apply_delete(
    tx: &crate::database::DatabaseWriteTransaction<'_>,
    descriptor: &SyncTableDescriptor,
    table: &str,
    _columns: &[String],
    entity_key: &[Value],
    entity_text: &str,
    op_hlc: &str,
) -> Result<ApplyOutcome, Error> {
    if has_newer_pending(tx, table, entity_text, op_hlc)? {
        return Ok(ApplyOutcome::SkippedPending);
    }
    if let Some(version) = row_version(tx, table, entity_text)? {
        if version.as_str() >= op_hlc {
            return Ok(ApplyOutcome::SkippedStale);
        }
    }
    let mut clauses = Vec::new();
    let mut args: Vec<(String, Value)> = Vec::new();
    for (index, column) in descriptor.key_columns.iter().enumerate() {
        clauses.push(format!("\"{column}\" IS @k{index}"));
        args.push((
            format!("k{index}"),
            entity_key.get(index).cloned().unwrap_or(Value::Null),
        ));
    }
    let arg_map: HashMap<String, Value> = args
        .iter()
        .map(|(k, v)| (format!("@{k}"), v.clone()))
        .collect();
    tx.execute_non_query(
        &format!("DELETE FROM \"{table}\" WHERE {}", clauses.join(" AND ")),
        &arg_map,
    )?;
    set_row_version(tx, table, entity_text, op_hlc, None)?;
    Ok(ApplyOutcome::Applied)
}

fn apply_set(
    tx: &crate::database::DatabaseWriteTransaction<'_>,
    descriptor: &SyncTableDescriptor,
    table: &str,
    columns: &[String],
    op: &SyncOpRecord,
    entity_text: &str,
) -> Result<ApplyOutcome, Error> {
    let Some(payload) = op.payload.as_ref() else {
        return Ok(ApplyOutcome::SkippedMissing);
    };
    if has_newer_pending(tx, table, entity_text, &op.hlc)? {
        return Ok(ApplyOutcome::SkippedPending);
    }
    if let Some(version) = row_version(tx, table, entity_text)? {
        if version >= op.hlc {
            return Ok(ApplyOutcome::SkippedStale);
        }
    }

    let existing = find_rowid(tx, descriptor, table, &op.entity_key)?;
    // Counter fields merge exclusively through Inc facts.
    let payload_columns: Vec<&String> = columns
        .iter()
        .filter(|column| payload.contains_key(column.as_str()))
        .filter(|column| {
            descriptor.field_semantics.iter().all(|(field, semantic)| {
                !(*field == column.as_str() && *semantic == SyncFieldSemantic::CounterDelta)
            })
        })
        .collect();

    if let Some(rowid) = existing {
        if descriptor.row_semantic == SyncRowSemantic::GSet && descriptor.field_semantics.is_empty()
        {
            // Pure fact: an identical natural key is the same fact.
            set_row_version(tx, table, entity_text, &op.hlc, Some(payload))?;
            return Ok(ApplyOutcome::SkippedStale);
        }
        // Merge fields: G-Set tables only take explicitly overridden fields
        // (e.g. location.time), LWW rows take every payload column.
        let update_columns: Vec<&String> = if descriptor.row_semantic == SyncRowSemantic::GSet {
            payload_columns
                .into_iter()
                .filter(|column| {
                    descriptor
                        .field_semantics
                        .iter()
                        .any(|(field, _)| *field == column.as_str())
                })
                .collect()
        } else {
            payload_columns
        };
        if !update_columns.is_empty() {
            let assignments = update_columns
                .iter()
                .map(|column| format!("\"{column}\" = @c_{column}"))
                .collect::<Vec<_>>();
            let arg_map: HashMap<String, Value> = update_columns
                .iter()
                .map(|column| {
                    (
                        format!("@c_{column}"),
                        payload.get(column.as_str()).cloned().unwrap_or(Value::Null),
                    )
                })
                .collect();
            tx.execute_non_query(
                &format!(
                    "UPDATE \"{table}\" SET {} WHERE rowid = @rowid",
                    assignments.join(", ")
                ),
                &{
                    let mut map = arg_map;
                    map.insert("@rowid".to_string(), Value::from(rowid));
                    map
                },
            )?;
        }
    } else {
        let col_names = payload_columns
            .iter()
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>();
        let placeholders = payload_columns
            .iter()
            .map(|c| format!("@v_{c}"))
            .collect::<Vec<_>>();
        let arg_map: HashMap<String, Value> = payload_columns
            .iter()
            .map(|column| {
                (
                    format!("@v_{column}"),
                    payload.get(column.as_str()).cloned().unwrap_or(Value::Null),
                )
            })
            .collect();
        tx.execute_non_query(
            &format!(
                "INSERT INTO \"{table}\" ({}) VALUES ({})",
                col_names.join(", "),
                placeholders.join(", ")
            ),
            &arg_map,
        )?;
    }
    let has_special_fields = !descriptor.field_semantics.is_empty();
    if has_special_fields {
        if let Some(rowid) =
            existing.or_else(|| find_rowid(tx, descriptor, table, &op.entity_key).unwrap_or(None))
        {
            let col_list = columns
                .iter()
                .map(|column| format!("\"{column}\""))
                .collect::<Vec<_>>()
                .join(", ");
            let rows = tx.execute(
                &format!("SELECT {col_list} FROM \"{table}\" WHERE rowid = @rowid"),
                &params(&[("rowid", Value::from(rowid))]),
            )?;
            if let Some(row) = rows.first() {
                let mut merged = payload.clone();
                for (column, value) in columns.iter().zip(row.iter()) {
                    if let Some(semantic_field) = descriptor
                        .field_semantics
                        .iter()
                        .find(|(field, _)| *field == column.as_str())
                    {
                        merged.insert(column.clone(), value.clone());
                        let _ = semantic_field;
                    }
                }
                set_row_version(tx, table, entity_text, &op.hlc, Some(&merged))?;
                return Ok(ApplyOutcome::Applied);
            }
        }
    }
    set_row_version(tx, table, entity_text, &op.hlc, Some(payload))?;
    Ok(ApplyOutcome::Applied)
}

fn apply_inc(
    tx: &crate::database::DatabaseWriteTransaction<'_>,
    descriptor: &SyncTableDescriptor,
    table: &str,
    columns: &[String],
    op: &SyncOpRecord,
    entity_text: &str,
) -> Result<ApplyOutcome, Error> {
    let Some(payload) = op.payload.as_ref() else {
        return Ok(ApplyOutcome::SkippedMissing);
    };
    let Some(field) = payload.get("field").and_then(Value::as_str) else {
        return Ok(ApplyOutcome::SkippedMissing);
    };
    let Some(delta) = payload.get("delta").and_then(Value::as_i64) else {
        return Ok(ApplyOutcome::SkippedMissing);
    };
    if !columns.iter().any(|c| c == field) {
        return Ok(ApplyOutcome::SkippedMissing);
    }
    let Some(rowid) = find_rowid(tx, descriptor, table, &op.entity_key)? else {
        return Ok(ApplyOutcome::SkippedMissing);
    };
    tx.execute_non_query(
        &format!(
            "UPDATE \"{table}\" SET \"{field}\" = COALESCE(\"{field}\", 0) + @delta WHERE rowid = @rowid"
        ),
        &params(&[
            ("delta", Value::from(delta)),
            ("rowid", Value::from(rowid)),
        ]),
    )?;
    update_snapshot_field(tx, table, entity_text, field, |current| {
        let base = current.as_i64().unwrap_or(0);
        Value::from(base + delta)
    })?;
    set_row_version(tx, table, entity_text, &op.hlc, None)?;
    Ok(ApplyOutcome::Applied)
}

fn apply_element(
    tx: &crate::database::DatabaseWriteTransaction<'_>,
    table: &str,
    columns: &[String],
    op: &SyncOpRecord,
    entity_text: &str,
) -> Result<ApplyOutcome, Error> {
    let Some(payload) = op.payload.as_ref() else {
        return Ok(ApplyOutcome::SkippedMissing);
    };
    let Some(field) = payload.get("field").and_then(Value::as_str) else {
        return Ok(ApplyOutcome::SkippedMissing);
    };
    let Some(element) = payload.get("element") else {
        return Ok(ApplyOutcome::SkippedMissing);
    };
    if !columns.iter().any(|c| c == field) {
        return Ok(ApplyOutcome::SkippedMissing);
    }
    let Some(descriptor) = sync_table_descriptor(table) else {
        return Ok(ApplyOutcome::SkippedMissing);
    };
    let Some(rowid) = find_rowid(tx, descriptor, table, &op.entity_key)? else {
        return Ok(ApplyOutcome::SkippedMissing);
    };
    let rows = tx.execute(
        &format!("SELECT \"{field}\" FROM \"{table}\" WHERE rowid = @rowid"),
        &params(&[("rowid", Value::from(rowid))]),
    )?;
    let current_text = rows
        .first()
        .and_then(|row| row.first())
        .and_then(Value::as_str)
        .unwrap_or("[]");
    let mut list: Vec<Value> = serde_json::from_str(current_text).unwrap_or_default();
    match op.kind {
        SyncOpKind::SetAdd => {
            if !list.contains(element) {
                list.push(element.clone());
            }
        }
        SyncOpKind::SetRemove => {
            list.retain(|item| item != element);
        }
        _ => unreachable!("element ops only"),
    }
    tx.execute_non_query(
        &format!("UPDATE \"{table}\" SET \"{field}\" = @value WHERE rowid = @rowid"),
        &params(&[
            (
                "value",
                Value::String(serde_json::to_string(&list).unwrap_or_default()),
            ),
            ("rowid", Value::from(rowid)),
        ]),
    )?;
    update_snapshot_field(tx, table, entity_text, field, |_| {
        Value::Array(list.clone())
    })?;
    set_row_version(tx, table, entity_text, &op.hlc, None)?;
    Ok(ApplyOutcome::Applied)
}

fn update_snapshot_field(
    tx: &crate::database::DatabaseWriteTransaction<'_>,
    table: &str,
    entity_text: &str,
    field: &str,
    transform: impl FnOnce(&Value) -> Value,
) -> Result<(), Error> {
    let rows = tx.execute(
        "SELECT snapshot FROM _sync_row_version WHERE table_name = @t AND entity_key = @k LIMIT 1",
        &params(&[
            ("t", Value::String(table.to_string())),
            ("k", Value::String(entity_text.to_string())),
        ]),
    )?;
    let Some(snapshot_text) = rows
        .first()
        .and_then(|row| row.first())
        .and_then(Value::as_str)
    else {
        return Ok(());
    };
    let mut snapshot: Map<String, Value> = serde_json::from_str(snapshot_text).unwrap_or_default();
    let updated = transform(snapshot.get(field).unwrap_or(&Value::Null));
    snapshot.insert(field.to_string(), updated);
    tx.execute_non_query(
        "UPDATE _sync_row_version SET snapshot = @s WHERE table_name = @t AND entity_key = @k",
        &params(&[
            (
                "s",
                Value::String(serde_json::to_string(&snapshot).unwrap_or_default()),
            ),
            ("t", Value::String(table.to_string())),
            ("k", Value::String(entity_text.to_string())),
        ]),
    )?;
    Ok(())
}

// ─────────────────────────── bootstrap read helpers ─────────────────────────

pub struct TableChunk {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub next_rowid: i64,
    pub total_rows: i64,
}

/// Read one rowid-ordered chunk of a synced table for full bootstrap push.
pub fn read_table_chunk(
    db: &DatabaseService,
    table: &str,
    after_rowid: i64,
    limit: i64,
    created_after: Option<&str>,
) -> Result<Option<TableChunk>, Error> {
    if sync_table_descriptor(table).is_none() || !table_exists(db, table)? {
        return Ok(None);
    }
    let columns: Vec<String> = table_payload_column_types(db, table)?
        .into_iter()
        .map(|(column, _)| column)
        .collect();
    if columns.is_empty() {
        return Ok(None);
    }
    let column_list = columns
        .iter()
        .map(|column| format!("\"{column}\""))
        .collect::<Vec<_>>()
        .join(", ");
    // Optional recent-history window; ignored for tables without created_at.
    let since = created_after.filter(|_| columns.iter().any(|column| column == "created_at"));
    let window_sql = since
        .map(|_| " AND (created_at IS NULL OR created_at >= @since)")
        .unwrap_or("");
    let mut args: Vec<(&str, Value)> = Vec::new();
    let push_arg = |name: &'static str, value: Value, args: &mut Vec<(&'static str, Value)>| {
        args.push((name, value));
    };
    push_arg("after", Value::from(after_rowid), &mut args);
    push_arg("limit", Value::from(limit), &mut args);
    if let Some(since) = since {
        push_arg("since", Value::String(since.to_string()), &mut args);
    }
    let arg_map = |names: &[&str]| -> HashMap<String, Value> {
        names
            .iter()
            .filter_map(|name| {
                args.iter()
                    .find(|(arg, _)| arg == name)
                    .map(|(_, value)| (format!("@{name}"), value.clone()))
            })
            .collect()
    };
    let total_rows = db
        .execute(
            &format!("SELECT COUNT(*) FROM \"{table}\" WHERE 1=1{window_sql}"),
            &arg_map(&["since"]),
        )?
        .first()
        .and_then(|row| row.first())
        .and_then(Value::as_i64)
        .unwrap_or(0);
    // rowid leads the select purely for pagination; payload rows drop it.
    let rows = db.execute(
        &format!(
            "SELECT rowid, {column_list} FROM \"{table}\" WHERE rowid > @after{window_sql} ORDER BY rowid LIMIT @limit"
        ),
        &arg_map(&["after", "limit", "since"]),
    )?;
    let next_rowid = rows
        .iter()
        .filter_map(|row| row.first().and_then(Value::as_i64))
        .max()
        .unwrap_or(after_rowid);
    let rows = rows.iter().map(|row| row[1..].to_vec()).collect();
    Ok(Some(TableChunk {
        columns,
        rows,
        next_rowid,
        total_rows,
    }))
}

/// Build the bootstrap ops for one row. Counter fields split into increment
/// facts (exact sums across independently seeded devices); the snapshot is
/// seeded so later takes diff from the pushed value.
pub fn bootstrap_set_ops(
    db: &DatabaseService,
    plan: &TablePlan,
    device: &str,
    hlc: &str,
    seq: u64,
    columns: &[String],
    row: &[Value],
) -> Result<Vec<SyncOpRecord>, Error> {
    let mut payload = Map::new();
    for (column, value) in columns.iter().zip(row.iter()) {
        payload.insert(column.clone(), value.clone());
    }
    let entity_key: Vec<Value> = plan
        .descriptor
        .key_columns
        .iter()
        .filter_map(|column| payload.get(*column).cloned())
        .collect();
    let entity_text = serde_json::to_string(&entity_key).unwrap_or_else(|_| "[]".into());

    let mut counter_facts = Vec::new();
    let mut set_payload = payload.clone();
    for (field, semantic) in plan.descriptor.field_semantics {
        if *semantic != SyncFieldSemantic::CounterDelta {
            continue;
        }
        if let Some(value) = set_payload.remove(*field) {
            let current = value.as_i64().unwrap_or(0);
            if current > 0 {
                counter_facts.push(SyncOpRecord {
                    op_id: format!("{hlc}/{seq}/boot{current}"),
                    table: plan.table.clone(),
                    entity_key: entity_key.clone(),
                    kind: SyncOpKind::Inc,
                    payload: Some(Map::from_iter([
                        ("field".to_string(), Value::String(field.to_string())),
                        ("delta".to_string(), Value::from(current)),
                    ])),
                    hlc: hlc.to_string(),
                    device: device.to_string(),
                });
            }
        }
    }
    let mut ops = vec![SyncOpRecord {
        op_id: format!("{hlc}/{seq}"),
        table: plan.table.clone(),
        entity_key: entity_key.clone(),
        kind: SyncOpKind::Set,
        payload: Some(set_payload),
        hlc: hlc.to_string(),
        device: device.to_string(),
    }];
    ops.extend(counter_facts);

    if !plan.descriptor.field_semantics.is_empty() {
        db.write_transaction(|tx| {
            tx.execute_non_query(
                "INSERT INTO _sync_row_version (table_name, entity_key, hlc, snapshot)
                 VALUES (@t, @k, @h, @s)
                 ON CONFLICT(table_name, entity_key) DO UPDATE SET
                    hlc = CASE WHEN @h > hlc THEN @h ELSE hlc END,
                    snapshot = @s",
                &params(&[
                    ("t", Value::String(plan.table.clone())),
                    ("k", Value::String(entity_text)),
                    ("h", Value::String(hlc.to_string())),
                    (
                        "s",
                        Value::String(serde_json::to_string(&payload).unwrap_or_default()),
                    ),
                ]),
            )?;
            Ok(())
        })?;
    }
    Ok(ops)
}

/// Cursor position for bootstrap resumption, keyed by table.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct BootstrapState {
    pub phase: String,
    pub table_index: usize,
    pub last_rowid: i64,
    pub tables: Vec<String>,
    /// Total rows across tables at bootstrap start, for progress display.
    #[serde(default)]
    pub rows_total: i64,
    /// Row count per table, aligned with `tables`, for per-table progress.
    #[serde(default)]
    pub table_rows: Vec<i64>,
}

/// Row count of one synced table, for bootstrap progress totals.
pub fn table_row_count(db: &DatabaseService, table: &str) -> Result<i64, Error> {
    let rows = db.execute(
        &format!("SELECT COUNT(*) FROM \"{table}\""),
        &Default::default(),
    )?;
    Ok(rows
        .first()
        .and_then(|row| row.first())
        .and_then(Value::as_i64)
        .unwrap_or(0))
}

pub fn bootstrap_state_get(db: &DatabaseService) -> Result<Option<BootstrapState>, Error> {
    match sync_meta_get(db, META_BOOTSTRAP_STATE)? {
        Some(text) => Ok(serde_json::from_str(&text).ok()),
        None => Ok(None),
    }
}

pub fn bootstrap_state_set(db: &DatabaseService, state: &BootstrapState) -> Result<(), Error> {
    sync_meta_set(
        db,
        META_BOOTSTRAP_STATE,
        &serde_json::to_string(state).unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db(name: &str) -> DatabaseService {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("vrcx-0-sync-{name}-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        DatabaseService::new(&dir.join("VRCX-0.sqlite3")).unwrap()
    }

    fn setup_gamelog(db: &DatabaseService) {
        crate::game_log::ensure_game_log_tables(db).unwrap();
    }

    #[test]
    fn capture_records_insert_update_and_delete_facts() {
        let db = test_db("capture-basic");
        setup_gamelog(&db);
        let handle = install_capture(&db).unwrap();

        db.execute_non_query(
            "INSERT INTO gamelog_join_leave (created_at, type, display_name, location, user_id, time, owner_id)
             VALUES ('2026-01-01T00:00:00Z', 'OnPlayerJoined', 'Alice', 'loc1', 'usr_a', 0, 1)",
            &Default::default(),
        )
        .unwrap();
        let pending = outbox_pending_count(&db).unwrap();
        assert_eq!(pending, 1);

        // Same natural key again: INSERT OR IGNORE must not capture.
        db.execute_non_query(
            "INSERT OR IGNORE INTO gamelog_join_leave (created_at, type, display_name, location, user_id, time, owner_id)
             VALUES ('2026-01-01T00:00:00Z', 'OnPlayerJoined', 'Alice', 'loc1', 'usr_a', 5, 1)",
            &Default::default(),
        )
        .unwrap();
        assert_eq!(outbox_pending_count(&db).unwrap(), 1);

        // UPDATE captures a fresh fact.
        db.execute_non_query(
            "UPDATE gamelog_location SET time = 10 WHERE 0", // table missing rows; use join_leave update instead
            &Default::default(),
        )
        .ok();
        db.execute_non_query(
            "UPDATE gamelog_join_leave SET time = 9 WHERE display_name = 'Alice'",
            &Default::default(),
        )
        .unwrap();
        assert_eq!(outbox_pending_count(&db).unwrap(), 2);

        // DELETE captures a del fact with a provisional stamp.
        db.execute_non_query(
            "DELETE FROM gamelog_join_leave WHERE display_name = 'Alice'",
            &Default::default(),
        )
        .unwrap();
        assert_eq!(outbox_pending_count(&db).unwrap(), 3);

        let batch = outbox_take(&db, &handle, 10).unwrap().unwrap();
        // insert + update compact into one full-row Set; delete follows.
        assert_eq!(batch.ops.len(), 2);
        assert!(matches!(batch.ops[0].kind, SyncOpKind::Set));
        assert!(matches!(batch.ops[1].kind, SyncOpKind::Delete));
        assert!(
            !batch.ops[1].hlc.is_empty(),
            "del ops must be stamped at take"
        );
    }

    #[test]
    fn set_op_take_compaction_keeps_last_snapshot_per_key() {
        let db = test_db("take-compaction");
        setup_gamelog(&db);
        let handle = install_capture(&db).unwrap();

        for time in [1, 2, 3] {
            db.execute_non_query(
                "INSERT OR IGNORE INTO gamelog_location (created_at, location, world_id, world_name, time, group_name, owner_id)
                 VALUES ('2026-01-01T00:00:00Z', 'w1', 'wr1', 'World', 1, 'g', 0)",
                &Default::default(),
            )
            .unwrap();
            db.execute_non_query(
                "UPDATE gamelog_location SET time = @t WHERE created_at = '2026-01-01T00:00:00Z' AND location = 'w1'",
                &params(&[("t", Value::from(time))]),
            )
            .unwrap();
        }
        // 3 inserts (2 ignored) + 3 updates = 4 facts, compacted to 1 Set.
        let batch = outbox_take(&db, &handle, 10).unwrap().unwrap();
        assert_eq!(batch.ops.len(), 1);
        assert_eq!(
            batch.ops[0].payload.as_ref().unwrap().get("time"),
            Some(&Value::from(3))
        );
    }

    #[test]
    fn pull_apply_is_idempotent_and_echo_free() {
        let db = test_db("pull-apply");
        setup_gamelog(&db);
        let handle = install_capture(&db).unwrap();
        let device = handle.device_id().to_string();

        let op = SyncOpRecord {
            op_id: "x1".into(),
            table: "gamelog_join_leave".into(),
            entity_key: vec![
                Value::String("2026-02-01T00:00:00Z".into()),
                Value::String("OnPlayerJoined".into()),
                Value::String("Bob".into()),
            ],
            kind: SyncOpKind::Set,
            payload: Some(Map::from_iter([
                (
                    "created_at".to_string(),
                    Value::String("2026-02-01T00:00:00Z".into()),
                ),
                ("type".to_string(), Value::String("OnPlayerJoined".into())),
                ("display_name".to_string(), Value::String("Bob".into())),
                ("user_id".to_string(), Value::String("usr_bob".into())),
            ])),
            hlc: format!("{}-00000000-remote1", "00000000000001000"),
            device: "remote1".into(),
        };

        let stats = apply_pulled_ops(&db, &device, std::slice::from_ref(&op), 1).unwrap();
        assert_eq!(stats.applied, 1);
        // Echo: applying must not enqueue a push-back fact.
        assert_eq!(outbox_pending_count(&db).unwrap(), 0);

        // Re-delivery of the same op is stale.
        let stats = apply_pulled_ops(&db, &device, std::slice::from_ref(&op), 2).unwrap();
        assert_eq!(stats.applied, 0);
        assert_eq!(stats.skipped_stale + stats.skipped_pending, 1);

        // Newer remote op wins; older local pending op is protected.
        let count = db
            .execute(
                "SELECT COUNT(*) FROM gamelog_join_leave",
                &Default::default(),
            )
            .unwrap()
            .first()
            .unwrap()
            .first()
            .and_then(Value::as_i64)
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn local_pending_fact_beats_pulled_older_op() {
        let db = test_db("pending-wins");
        setup_gamelog(&db);
        crate::database::schema::ensure_global_store_tables(&db).unwrap();
        let handle = install_capture(&db).unwrap();
        let device = handle.device_id().to_string();

        // Local write of the same fact the remote will also send.
        db.execute_non_query(
            "INSERT INTO memos (user_id, edited_at, memo) VALUES ('usr_x', '2026-03-01', 'local newer')",
            &Default::default(),
        )
        .unwrap();

        let pulled = SyncOpRecord {
            op_id: "x1".into(),
            table: "memos".into(),
            entity_key: vec![Value::String("usr_x".into())],
            kind: SyncOpKind::Set,
            payload: Some(Map::from_iter([
                ("user_id".to_string(), Value::String("usr_x".into())),
                ("edited_at".to_string(), Value::String("2026-03-01".into())),
                ("memo".to_string(), Value::String("remote older".into())),
            ])),
            hlc: "00000000000000001-00000000-remote1".into(),
            device: "remote1".into(),
        };
        let stats = apply_pulled_ops(&db, &device, std::slice::from_ref(&pulled), 1).unwrap();
        assert_eq!(stats.applied, 0);
        let memo = db
            .execute("SELECT memo FROM memos", &Default::default())
            .unwrap()
            .first()
            .unwrap()
            .first()
            .and_then(Value::as_str)
            .unwrap()
            .to_string();
        assert_eq!(memo, "local newer");
    }

    #[test]
    fn new_tables_derive_descriptors_and_capture_after_refresh() {
        let db = test_db("derive-new-table");
        setup_gamelog(&db);
        let handle = install_capture(&db).unwrap();

        // A table shipped after install: TEXT primary key, mutable columns.
        db.execute_non_query(
            "CREATE TABLE playlist_items (id TEXT PRIMARY KEY, title TEXT, updated_at TEXT)",
            &Default::default(),
        )
        .unwrap();
        db.execute_non_query(
            "INSERT INTO playlist_items (id, title, updated_at) VALUES ('pl1', 'First', '2026-06-01')",
            &Default::default(),
        )
        .unwrap();
        // Not captured before the scan knows the table…
        let before = outbox_pending_count(&db).unwrap();
        // …and an excluded cache table stays out entirely.
        db.execute_non_query(
            "CREATE TABLE cache_future_thing (id TEXT PRIMARY KEY, blob TEXT)",
            &Default::default(),
        )
        .unwrap();
        db.execute_non_query(
            "INSERT INTO cache_future_thing (id, blob) VALUES ('c1', 'x')",
            &Default::default(),
        )
        .unwrap();
        refresh_capture(&db, &handle).unwrap();

        db.execute_non_query(
            "INSERT INTO playlist_items (id, title, updated_at) VALUES ('pl2', 'Second', '2026-06-02')",
            &Default::default(),
        )
        .unwrap();
        db.execute_non_query(
            "INSERT INTO cache_future_thing (id, blob) VALUES ('c2', 'y')",
            &Default::default(),
        )
        .unwrap();
        let batch = outbox_take(&db, &handle, 20).unwrap().unwrap();
        let playlist_sets: Vec<_> = batch
            .ops
            .iter()
            .filter(|op| op.table == "playlist_items")
            .collect();
        assert!(!playlist_sets.is_empty(), "derived table captures changes");
        assert!(
            batch.ops.iter().all(|op| op.table != "cache_future_thing"),
            "cache tables are never captured"
        );
        let _ = before;
        let derived = vrcx_0_contracts::sync_table_descriptor("playlist_items").unwrap();
        assert_eq!(
            derived.key_columns,
            &["id"],
            "TEXT primary key is the derived natural key"
        );
    }

    #[test]
    fn column_addition_rebuilds_triggers_so_payloads_carry_new_columns() {
        let db = test_db("derive-column-add");
        setup_gamelog(&db);
        let handle = install_capture(&db).unwrap();

        db.execute_non_query(
            "CREATE TABLE widget_notes (id TEXT PRIMARY KEY, body TEXT)",
            &Default::default(),
        )
        .unwrap();
        refresh_capture(&db, &handle).unwrap();
        db.execute_non_query(
            "INSERT INTO widget_notes (id, body) VALUES ('w1', 'hello')",
            &Default::default(),
        )
        .unwrap();
        let first = outbox_take(&db, &handle, 10).unwrap().unwrap();
        assert!(first.ops[0]
            .payload
            .as_ref()
            .unwrap()
            .get("note_color")
            .is_none());
        outbox_trim_pushed(&db, first.max_seq).unwrap();

        // App update adds a column; the drift check must rebuild triggers.
        db.execute_non_query(
            "ALTER TABLE widget_notes ADD COLUMN note_color TEXT NOT NULL DEFAULT ''",
            &Default::default(),
        )
        .unwrap();
        refresh_capture(&db, &handle).unwrap();
        db.execute_non_query(
            "UPDATE widget_notes SET note_color = 'red' WHERE id = 'w1'",
            &Default::default(),
        )
        .unwrap();
        let second = outbox_take(&db, &handle, 10).unwrap().unwrap();
        assert_eq!(
            second.ops[0].payload.as_ref().unwrap().get("note_color"),
            Some(&Value::String("red".into())),
            "payload includes the new column after refresh"
        );
    }

    #[test]
    fn read_table_chunk_pagination_covers_every_row_exactly_once() {
        let db = test_db("chunk-pagination");
        setup_gamelog(&db);
        db.execute_non_query(
            "CREATE TABLE widget_notes (id TEXT PRIMARY KEY, body TEXT)",
            &Default::default(),
        )
        .unwrap();
        // Register the table so read_table_chunk accepts it, then fill it.
        // TEXT-leading columns are exactly the shape that broke pagination:
        // the cursor must come from rowid, never from the first payload
        // column.
        let handle = install_capture(&db).unwrap();
        refresh_capture(&db, &handle).unwrap();
        for index in 0..1237 {
            db.execute_non_query(
                "INSERT INTO widget_notes (id, body) VALUES (@id, @body)",
                &params(&[
                    ("id", Value::String(format!("w{index}"))),
                    ("body", Value::String("text".into())),
                ]),
            )
            .unwrap();
        }
        let mut after = 0i64;
        let mut visited = 0usize;
        let mut seen_ids = std::collections::HashSet::new();
        for _ in 0..30 {
            let chunk = read_table_chunk(&db, "widget_notes", after, 100, None)
                .unwrap()
                .expect("chunk");
            assert!(chunk.next_rowid > after, "cursor must advance past {after}");
            for row in &chunk.rows {
                let id = row[0].as_str().unwrap_or_default().to_string();
                assert!(seen_ids.insert(id), "row revisited — pagination looped");
                visited += 1;
            }
            after = chunk.next_rowid;
            if chunk.rows.len() < 100 {
                break;
            }
        }
        assert_eq!(visited, 1237, "every row uploaded exactly once");
    }

    #[test]
    fn configs_credentials_are_never_captured() {
        let db = test_db("configs-filter");
        db.execute_non_query(
            "CREATE TABLE configs (key TEXT PRIMARY KEY, value TEXT)",
            &Default::default(),
        )
        .unwrap();
        let handle = install_capture(&db).unwrap();

        db.execute_non_query(
            "INSERT INTO configs (key, value) VALUES ('config:theme', 'dark')",
            &Default::default(),
        )
        .unwrap();
        db.execute_non_query(
            "INSERT INTO configs (key, value) VALUES ('config:vrcx_webhookurl', 'https://secret')",
            &Default::default(),
        )
        .unwrap();
        db.execute_non_query(
            "INSERT INTO configs (key, value) VALUES ('config:mystery', 'enc1:AAAA')",
            &Default::default(),
        )
        .unwrap();
        let batch = outbox_take(&db, &handle, 10).unwrap().unwrap();
        assert_eq!(batch.ops.len(), 1);
        assert_eq!(
            batch.ops[0].payload.as_ref().unwrap().get("key"),
            Some(&Value::String("config:theme".into()))
        );
    }

    #[test]
    fn counter_delta_take_and_apply_converge() {
        let db = test_db("counter-delta");
        db.execute_non_query(
            "CREATE TABLE browse_history (owner_user_id TEXT NOT NULL, entity_kind TEXT NOT NULL, entity_id TEXT NOT NULL, first_viewed_at TEXT, last_viewed_at TEXT, view_count INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (owner_user_id, entity_kind, entity_id))",
            &Default::default(),
        )
        .unwrap();
        let handle = install_capture(&db).unwrap();

        // First take splits the counter into an increment fact and a Set that
        // carries no counter value.
        db.execute_non_query(
            "INSERT INTO browse_history (owner_user_id, entity_kind, entity_id, first_viewed_at, last_viewed_at, view_count)
             VALUES ('usr_o', 'world', 'wr1', '2026-01-01', '2026-01-01', 1)",
            &Default::default(),
        )
        .unwrap();
        let first = outbox_take(&db, &handle, 10).unwrap().unwrap();
        assert_eq!(first.ops.len(), 2);
        let inc = first
            .ops
            .iter()
            .find(|op| matches!(op.kind, SyncOpKind::Inc))
            .expect("first take emits an increment fact");
        assert_eq!(
            inc.payload.as_ref().unwrap().get("delta"),
            Some(&Value::from(1))
        );
        let set = first
            .ops
            .iter()
            .find(|op| matches!(op.kind, SyncOpKind::Set))
            .expect("first take emits a Set");
        assert!(
            set.payload.as_ref().unwrap().get("view_count").is_none(),
            "Sets never carry counter values"
        );
        outbox_trim_pushed(&db, first.max_seq).unwrap();

        // Subsequent increments become Inc deltas.
        for _ in 0..3 {
            db.execute_non_query(
                "UPDATE browse_history SET view_count = view_count + 1, last_viewed_at = '2026-01-02'
                 WHERE entity_id = 'wr1'",
                &Default::default(),
            )
            .unwrap();
        }
        let second = outbox_take(&db, &handle, 10).unwrap().unwrap();
        // The first update also changed last_viewed_at (plain column), so it
        // derives a full Set snapshot; the remaining increments derive Inc
        // deltas. Set(vc=2) + Inc(1) + Inc(1) sums to the same total on every
        // replica, so this is the intended shape.
        assert_eq!(second.ops.len(), 3);
        assert!(matches!(second.ops[0].kind, SyncOpKind::Set));
        assert_eq!(
            second
                .ops
                .iter()
                .filter(|op| matches!(op.kind, SyncOpKind::Inc))
                .count(),
            2
        );
        outbox_trim_pushed(&db, second.max_seq).unwrap();

        // Without plain-column changes, pure increments emit Inc only.
        db.execute_non_query(
            "UPDATE browse_history SET view_count = view_count + 2 WHERE entity_id = 'wr1'",
            &Default::default(),
        )
        .unwrap();
        let third = outbox_take(&db, &handle, 10).unwrap().unwrap();
        assert_eq!(third.ops.len(), 1);
        assert!(matches!(third.ops[0].kind, SyncOpKind::Inc));
        assert_eq!(
            third.ops[0].payload.as_ref().unwrap().get("delta"),
            Some(&Value::from(2))
        );

        // Applying the same delta on a second replica twice is exact.
        let db2 = test_db("counter-delta-replica");
        db2.execute_non_query(
            "CREATE TABLE browse_history (owner_user_id TEXT NOT NULL, entity_kind TEXT NOT NULL, entity_id TEXT NOT NULL, first_viewed_at TEXT, last_viewed_at TEXT, view_count INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (owner_user_id, entity_kind, entity_id))",
            &Default::default(),
        )
        .unwrap();
        let handle2 = install_capture(&db2).unwrap();
        let seed = SyncOpRecord {
            op_id: "s0".into(),
            table: "browse_history".into(),
            entity_key: vec![
                Value::String("usr_o".into()),
                Value::String("world".into()),
                Value::String("wr1".into()),
            ],
            kind: SyncOpKind::Set,
            payload: Some(Map::from_iter([
                ("owner_user_id".to_string(), Value::String("usr_o".into())),
                ("entity_kind".to_string(), Value::String("world".into())),
                ("entity_id".to_string(), Value::String("wr1".into())),
            ])),
            hlc: "00000000000000001-00000000-origin".into(),
            device: "origin".into(),
        };
        let seed_inc = SyncOpRecord {
            op_id: "s0i".into(),
            table: "browse_history".into(),
            entity_key: seed.entity_key.clone(),
            kind: SyncOpKind::Inc,
            payload: Some(Map::from_iter([
                ("field".to_string(), Value::String("view_count".into())),
                ("delta".to_string(), Value::from(4)),
            ])),
            hlc: "00000000000000001-00000000-origin".into(),
            device: "origin".into(),
        };
        apply_pulled_ops(&db2, handle2.device_id(), &[seed.clone(), seed_inc], 1).unwrap();
        apply_pulled_ops(&db2, handle2.device_id(), &third.ops, 2).unwrap();
        let count = db2
            .execute("SELECT view_count FROM browse_history", &Default::default())
            .unwrap()
            .first()
            .unwrap()
            .first()
            .and_then(Value::as_i64)
            .unwrap();
        assert_eq!(count, 6);
    }
}
