//! Remote sync engine lifecycle on the runtime host.
//!
//! The engine exists only while the user enabled sync and provided a
//! connection string. Enabling/disabling from the settings UI re-runs
//! [`RemoteSyncHost::restart_from_config`]; startup calls
//! [`RemoteSyncHost::start_auto`] after data services start.

use std::sync::{Arc, RwLock};

use vrcx_0_application_core::{RuntimeBackgroundJobs, TaskSupervisor};
use vrcx_0_application_sync::{
    RemoteSyncEngine, CONFIG_ALLOW_PLAINTEXT, CONFIG_DATABASE, CONFIG_ENABLED, CONFIG_HOST,
    CONFIG_PASSWORD, CONFIG_PORT, CONFIG_TLS_VERIFY, CONFIG_USER,
};
use vrcx_0_outbound_adapters::{PostgresSyncStore, PostgresSyncStoreConfig};
use vrcx_0_persistence::config::{get_bool, get_string};
use vrcx_0_persistence::DatabaseService;

/// Owns the sync engine; cheap to share into tasks and Tauri commands.
pub struct RemoteSyncHost {
    db: Arc<DatabaseService>,
    background_jobs: RuntimeBackgroundJobs,
    tasks: TaskSupervisor,
    app_version: String,
    engine: RwLock<Option<Arc<RemoteSyncEngine>>>,
}

impl RemoteSyncHost {
    pub fn new(
        db: Arc<DatabaseService>,
        background_jobs: RuntimeBackgroundJobs,
        tasks: TaskSupervisor,
        app_version: String,
    ) -> Self {
        Self {
            db,
            background_jobs,
            tasks,
            app_version,
            engine: RwLock::new(None),
        }
    }

    pub fn current(&self) -> Option<Arc<RemoteSyncEngine>> {
        self.engine.read().ok().and_then(|guard| guard.clone())
    }

    /// Persisted sync settings (password included; keep internal).
    pub fn settings(&self) -> RemoteSyncSettings {
        RemoteSyncSettings {
            enabled: get_bool(&self.db, CONFIG_ENABLED, false).unwrap_or(false),
            host: get_string(&self.db, CONFIG_HOST, "").unwrap_or_default(),
            port: get_string(&self.db, CONFIG_PORT, "")
                .ok()
                .and_then(|raw| raw.trim().parse::<i64>().ok())
                .filter(|port| *port > 0 && *port < 65536)
                .unwrap_or(5432),
            user: get_string(&self.db, CONFIG_USER, "").unwrap_or_default(),
            password: get_string(&self.db, CONFIG_PASSWORD, "").unwrap_or_default(),
            database: get_string(&self.db, CONFIG_DATABASE, "").unwrap_or_default(),
            tls_verify: get_bool(&self.db, CONFIG_TLS_VERIFY, false).unwrap_or(false),
            allow_plaintext: get_bool(&self.db, CONFIG_ALLOW_PLAINTEXT, true).unwrap_or(true),
        }
    }

    /// Sanitized fields for the settings UI (no password value).
    pub fn connection_fields(&self) -> vrcx_0_contracts::SyncConnectionFields {
        let settings = self.settings();
        let read_i64 = |key: &str, default: i64| {
            get_string(&self.db, key, "")
                .ok()
                .and_then(|raw| raw.trim().parse::<i64>().ok())
                .filter(|value| *value >= 0)
                .unwrap_or(default)
        };
        vrcx_0_contracts::SyncConnectionFields {
            host: settings.host,
            port: settings.port,
            user: settings.user,
            database: settings.database,
            tls_verify: settings.tls_verify,
            allow_plaintext: settings.allow_plaintext,
            has_password: !settings.password.is_empty(),
            interval_seconds: read_i64(
                vrcx_0_application_sync::CONFIG_INTERVAL_SEC,
                vrcx_0_application_sync::DEFAULT_INTERVAL_SEC as i64,
            ),
        }
    }

    /// (Re)build the engine from the persisted settings. Stops and uninstalls
    /// capture when sync is disabled or unconfigured.
    pub async fn restart_from_config(&self) -> Result<Option<Arc<RemoteSyncEngine>>> {
        let settings = self.settings();
        if let Ok(mut guard) = self.engine.write() {
            if let Some(existing) = guard.take() {
                let _ = existing.shutdown();
            }
        }
        if !settings.enabled || !settings.is_configured() {
            return Ok(None);
        }
        let store = Arc::new(
            PostgresSyncStore::new(&PostgresSyncStoreConfig {
                dsn: settings.dsn(),
                tls_verify: settings.tls_verify,
                allow_plaintext: settings.allow_plaintext,
            })
            .map_err(|error| Error::Custom(error.to_string()))?,
        );
        let engine = RemoteSyncEngine::new(
            Arc::clone(&self.db),
            store,
            self.background_jobs.clone(),
            self.app_version.clone(),
        )
        .map_err(|error| Error::Custom(error.to_string()))?;
        engine
            .mark_bootstrap_pending()
            .map_err(|error| Error::Custom(format!("Failed to arm sync bootstrap: {error}")))?;
        engine.start_loop(self.tasks.clone());
        if let Ok(mut guard) = self.engine.write() {
            *guard = Some(Arc::clone(&engine));
        }
        engine.trigger_now();
        Ok(Some(engine))
    }

    /// Run one full sync cycle now and return the fresh status (the
    /// "sync now" button's backing call). Self-heals when the engine died or
    /// lost a startup race: a configured+enabled database gets the engine
    /// rebuilt here instead of surfacing an error dialog.
    pub async fn run_cycle_now(&self) -> Result<vrcx_0_contracts::SyncStatusSnapshot> {
        if self.current().is_none() {
            let settings = self.settings();
            if settings.enabled && settings.is_configured() {
                tracing::info!("sync engine missing; rebuilding from stored settings");
                self.restart_from_config()
                    .await?
                    .ok_or_else(|| Error::Custom(
                        "Remote sync engine could not be started from the saved settings; re-open the Data Sync settings and enable it again."
                            .to_string(),
                    ))?;
            } else {
                return Err(Error::Custom(
                    "Remote sync is not running: fill in the connection fields and turn on the enable switch in Settings → Data Sync first."
                        .to_string(),
                ));
            }
        }
        let engine = self.current().expect("engine ensured above");
        engine
            .run_cycle_and_status()
            .await
            .map_err(|error| Error::Custom(error.to_string()))
    }

    /// Stop the engine and uninstall local capture.
    pub fn stop(&self) -> Result<()> {
        if let Ok(mut guard) = self.engine.write() {
            if let Some(existing) = guard.take() {
                existing.shutdown().map_err(|error| {
                    Error::Custom(format!("Failed to stop remote sync: {error}"))
                })?;
            }
        }
        Ok(())
    }

    /// Called once after data services start; a no-op unless the user has
    /// sync enabled.
    pub fn start_auto(self: &Arc<Self>) {
        let settings = self.settings();
        if !settings.enabled || !settings.is_configured() {
            return;
        }
        let host = Arc::clone(self);
        self.tasks.spawn(async move {
            if let Err(error) = host.restart_from_config().await {
                tracing::warn!(error = %error, "failed to start remote sync");
            }
        });
    }
}

impl RemoteSyncHost {
    /// Status snapshot for the UI; works with or without a running engine.
    pub async fn status(&self) -> vrcx_0_contracts::SyncStatusSnapshot {
        if let Some(engine) = self.current() {
            return engine.status().await;
        }
        let settings = self.settings();
        vrcx_0_contracts::SyncStatusSnapshot {
            enabled: settings.enabled && settings.is_configured(),
            configured: settings.is_configured(),
            phase: "disabled".to_string(),
            device_id: self.device_id().unwrap_or_default(),
            last_push_at: None,
            last_pull_at: None,
            pending_outbox: 0,
            pull_cursor: 0,
            last_error: None,
            remote_schema_version: 0,
            remote_devices: Vec::new(),
            last_pushed_ops: 0,
            last_pulled_ops: 0,
            last_cycle_at: None,
        }
    }

    pub fn device_id(&self) -> Result<String> {
        vrcx_0_persistence::sync::sync_device_id(&self.db).map_err(Error::from)
    }

    /// One-off connection probe for the settings dialog. An empty password
    /// falls back to the stored one so the UI never needs to resend secrets.
    pub async fn test_connection(
        &self,
        host: &str,
        port: i64,
        user: &str,
        password: Option<&str>,
        database: &str,
        tls_verify: bool,
        allow_plaintext: bool,
    ) -> vrcx_0_contracts::SyncConnectionTestResult {
        let stored = self.settings();
        let settings = RemoteSyncSettings {
            host: host.trim().to_string(),
            port: if (1..65536).contains(&port) {
                port
            } else {
                5432
            },
            user: user.trim().to_string(),
            password: match password.map(str::trim) {
                Some(text) if !text.is_empty() => text.to_string(),
                _ => stored.password,
            },
            database: database.trim().to_string(),
            tls_verify,
            allow_plaintext,
            ..stored
        };
        if settings.host.is_empty() || settings.database.is_empty() {
            return vrcx_0_contracts::SyncConnectionTestResult {
                ok: false,
                server_version: String::new(),
                latency_ms: 0,
                error: Some("Host and database are required.".to_string()),
            };
        }
        let store = match PostgresSyncStore::new(&PostgresSyncStoreConfig {
            dsn: settings.dsn(),
            tls_verify,
            allow_plaintext,
        }) {
            Ok(store) => Arc::new(store),
            Err(error) => {
                return vrcx_0_contracts::SyncConnectionTestResult {
                    ok: false,
                    server_version: String::new(),
                    latency_ms: 0,
                    error: Some(error.to_string()),
                }
            }
        };
        let store: Arc<dyn vrcx_0_application_sync::RemoteSyncStore> = store;
        let mut result = vrcx_0_application_sync::test_store_connection(&store).await;
        if !result.ok && !allow_plaintext {
            if let Some(error) = result.error.as_deref() {
                if error.contains("TLS handshake") {
                    result.error = Some(format!(
                        "{error} — the server has no TLS; enable 'allow unencrypted connection' for trusted LAN servers"
                    ));
                }
            }
        }
        result
    }
}

#[derive(Clone, Debug)]
pub struct RemoteSyncSettings {
    pub enabled: bool,
    pub host: String,
    pub port: i64,
    pub user: String,
    pub password: String,
    pub database: String,
    pub tls_verify: bool,
    pub allow_plaintext: bool,
}

impl RemoteSyncSettings {
    pub fn is_configured(&self) -> bool {
        !self.host.trim().is_empty()
            && !self.user.trim().is_empty()
            && !self.password.is_empty()
            && !self.database.trim().is_empty()
    }

    /// Compose the libpq URL; identity components are percent-encoded.
    /// Plaintext is only possible through the explicit allow flag.
    pub fn dsn(&self) -> String {
        let ssl_mode = if self.allow_plaintext {
            "prefer"
        } else {
            "require"
        };
        format!(
            "postgresql://{}:{}@{}:{}/{}?sslmode={ssl_mode}",
            percent_encode(&self.user),
            percent_encode(&self.password),
            self.host.trim(),
            self.port,
            percent_encode(&self.database),
        )
    }
}

fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

impl crate::state::RuntimeHostState {
    pub fn remote_sync(&self) -> &Arc<RemoteSyncHost> {
        &self.remote_sync_host
    }
}

type Error = crate::Error;
type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod pg_integration {
    use super::*;

    /// End-to-end "test connection" exactly as the settings UI drives it:
    /// configs written with the same keys the command layer writes, read
    /// back through `settings()`, DSN composed, real server probed. Runs
    /// only when VRCX_PG_TEST=1 with VRCX_PG_TEST_* fields.
    #[test]
    fn test_connection_follows_the_app_path_against_a_real_server() {
        if std::env::var("VRCX_PG_TEST").ok().as_deref() != Some("1") {
            return;
        }
        let host = std::env::var("VRCX_PG_TEST_HOST").expect("VRCX_PG_TEST_HOST");
        let port: i64 = std::env::var("VRCX_PG_TEST_PORT")
            .unwrap_or_else(|_| "5432".into())
            .parse()
            .expect("port");
        let user = std::env::var("VRCX_PG_TEST_USER").expect("VRCX_PG_TEST_USER");
        let password = std::env::var("VRCX_PG_TEST_PASSWORD").expect("VRCX_PG_TEST_PASSWORD");
        let database = std::env::var("VRCX_PG_TEST_DATABASE").expect("VRCX_PG_TEST_DATABASE");

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("vrcx-0-sync-it-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Arc::new(DatabaseService::new(&dir.join("VRCX-0.sqlite3")).unwrap());

        // Same keys the Tauri command writes.
        vrcx_0_persistence::config::set_string(&db, CONFIG_HOST, &host).unwrap();
        vrcx_0_persistence::config::set_string(&db, CONFIG_PORT, &port.to_string()).unwrap();
        vrcx_0_persistence::config::set_string(&db, CONFIG_USER, &user).unwrap();
        vrcx_0_persistence::config::set_string(&db, CONFIG_PASSWORD, &password).unwrap();
        vrcx_0_persistence::config::set_string(&db, CONFIG_DATABASE, &database).unwrap();
        vrcx_0_persistence::config::set_bool(&db, CONFIG_TLS_VERIFY, false).unwrap();
        vrcx_0_persistence::config::set_bool(&db, CONFIG_ALLOW_PLAINTEXT, true).unwrap();

        let host_state = RemoteSyncHost::new(
            Arc::clone(&db),
            RuntimeBackgroundJobs::new(),
            TaskSupervisor::default(),
            "integration-test".into(),
        );
        let settings = host_state.settings();
        assert!(
            settings.is_configured(),
            "settings round-trip via config keys"
        );
        assert!(settings.dsn().starts_with("postgresql://"));
        assert!(settings.dsn().contains("sslmode=prefer"));

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = runtime
            .block_on(host_state.test_connection(&host, port, &user, None, &database, false, true));
        assert!(result.ok, "app-path test failed: {:?}", result.error);
        println!(
            "app-path OK: {} ({} ms)",
            result.server_version, result.latency_ms
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
