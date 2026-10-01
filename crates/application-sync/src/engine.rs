//! Remote sync engine: orchestrates push, pull, bootstrap and reconciliation
//! against a [`RemoteSyncStore`] port.
//!
//! Cycle shape (star topology, remote is the hub and single serialization
//! point):
//! 1. resume bootstrap if one is in flight,
//! 2. otherwise push staged outbox batches until drained,
//! 3. pull remote facts in arrival order until caught up,
//! 4. reconcile from materialized state if the op log was GC'd past our
//!    cursor, and run the op GC once per day.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Map, Value};
use tokio::sync::Notify;
use vrcx_0_application_core::{
    sleep_until_due_or_stopped, RuntimeBackgroundJobs, TaskSpawnOutcome, TaskSupervisor,
};
use vrcx_0_contracts::{
    sync_table_descriptor, SyncBootstrapProgress, SyncConnectionTestResult, SyncDeviceRecord,
    SyncFieldSemantic, SyncHlc, SyncOpKind, SyncOpRecord, SyncStatusSnapshot, SyncTableProgress,
    SYNC_PROTOCOL_SCHEMA_VERSION,
};
use vrcx_0_persistence::sync::{
    apply_pulled_ops, bootstrap_set_ops, bootstrap_state_get, bootstrap_state_set, install_capture,
    outbox_pending_count, outbox_take, outbox_trim_pushed, read_table_chunk, refresh_capture,
    sync_device_id, sync_meta_get, sync_meta_remove, sync_meta_set, synced_local_tables,
    uninstall_capture, BootstrapState, SyncCaptureHandle, META_BOOTSTRAP_STATE, META_LAST_OP_GC_AT,
    META_LAST_PULL_AT, META_LAST_PUSH_AT, META_PULL_CURSOR,
};
use vrcx_0_persistence::DatabaseService;

use crate::store::{
    MaterializedRow, PulledOp, RemoteColumnDef, RemoteColumnType, RemoteSyncStore,
    RemoteTableSchema, SyncStoreError,
};

pub const CONFIG_ENABLED: &str = "remoteSync.enabled";
pub const CONFIG_HOST: &str = "remoteSync.host";
pub const CONFIG_PORT: &str = "remoteSync.port";
pub const CONFIG_USER: &str = "remoteSync.user";
pub const CONFIG_PASSWORD: &str = "remoteSync.password";
pub const CONFIG_DATABASE: &str = "remoteSync.database";
pub const CONFIG_INTERVAL_SEC: &str = "remoteSync.intervalSec";
pub const CONFIG_RETAIN_DAYS: &str = "remoteSync.retainDays";
pub const CONFIG_TLS_VERIFY: &str = "remoteSync.tlsVerify";
pub const CONFIG_ALLOW_PLAINTEXT: &str = "remoteSync.allowPlaintext";

pub const DEFAULT_INTERVAL_SEC: u64 = 60;
pub const DEFAULT_RETAIN_DAYS: i64 = 30;

const PUSH_BATCH: i64 = 200;
const PULL_BATCH: i64 = 500;
const PUSH_BATCHES_PER_CYCLE: usize = 50;
const PULL_BATCHES_PER_CYCLE: usize = 100;
const BOOTSTRAP_CHUNK: i64 = 500;
const BOOTSTRAP_MATERIALIZED_PAGE: i64 = 1000;
const GC_INTERVAL_HOURS: i64 = 24;
/// Set while a first-time bootstrap should (re)run; cleared on completion.
const META_BOOTSTRAP_PENDING: &str = "sync.pendingBootstrap";
/// Written once after the first successful bootstrap.
const META_BOOTSTRAPPED_AT: &str = "sync.bootstrappedAt";
/// Local copy of the remote sync epoch; a mismatch (someone wiped or
/// rebuilt the remote) restarts the bootstrap automatically.
const META_REMOTE_EPOCH: &str = "sync.remoteEpoch";

pub const JOB_NAME: &str = "remoteSync";

#[derive(Debug, thiserror::Error)]
pub enum SyncEngineError {
    #[error(transparent)]
    Store(#[from] SyncStoreError),
    #[error("local database error: {0}")]
    Local(String),
}

impl From<vrcx_0_persistence::Error> for SyncEngineError {
    fn from(error: vrcx_0_persistence::Error) -> Self {
        Self::Local(error.to_string())
    }
}

pub type EngineResult<T> = std::result::Result<T, SyncEngineError>;

struct EngineState {
    phase: String,
    last_error: Option<String>,
    progress: SyncBootstrapProgress,
    stats: CycleStats,
}

/// What the most recent completed cycle moved, surfaced in the status card.
#[derive(Clone, Debug, Default)]
struct CycleStats {
    pushed: u64,
    pulled: u64,
    finished_at: Option<String>,
}

pub struct RemoteSyncEngine {
    db: Arc<DatabaseService>,
    store: Arc<dyn RemoteSyncStore>,
    background_jobs: RuntimeBackgroundJobs,
    app_version: String,
    handle: SyncCaptureHandle,
    waker: Arc<Notify>,
    state: Mutex<EngineState>,
    ensured_schema_hash: Mutex<Option<u64>>,
    /// Serializes cycles: a manual "sync now" and the interval loop must not
    /// apply pulled ops concurrently — Inc deltas would double-count if two
    /// appliers read the same cursor before either commits.
    cycle_lock: tokio::sync::Mutex<()>,
}

impl RemoteSyncEngine {
    /// Build the engine and install local change capture. Fails if the local
    /// database is unavailable.
    pub fn new(
        db: Arc<DatabaseService>,
        store: Arc<dyn RemoteSyncStore>,
        background_jobs: RuntimeBackgroundJobs,
        app_version: String,
    ) -> EngineResult<Arc<Self>> {
        let handle = install_capture(&db)?;
        Ok(Arc::new(Self {
            db,
            store,
            background_jobs,
            app_version,
            handle,
            waker: Arc::new(Notify::new()),
            ensured_schema_hash: Mutex::new(None),
            cycle_lock: tokio::sync::Mutex::new(()),
            state: Mutex::new(EngineState {
                phase: "idle".into(),
                last_error: None,
                progress: SyncBootstrapProgress::default(),
                stats: CycleStats::default(),
            }),
        }))
    }

    pub fn capture_handle(&self) -> &SyncCaptureHandle {
        &self.handle
    }

    /// Stop capturing; used when the user disables sync.
    pub fn shutdown(&self) -> EngineResult<()> {
        uninstall_capture(&self.db, &self.handle)?;
        Ok(())
    }

    /// Wake the loop for an immediate cycle ("sync now").
    pub fn trigger_now(&self) {
        self.waker.notify_one();
    }

    pub fn bootstrap_progress(&self) -> SyncBootstrapProgress {
        self.state.lock().unwrap().progress.clone()
    }

    pub fn configured_interval(&self) -> u64 {
        vrcx_0_persistence::config::get_string(&self.db, CONFIG_INTERVAL_SEC, "")
            .ok()
            .and_then(|raw| raw.trim().parse().ok())
            .filter(|secs| *secs >= 5)
            .unwrap_or(DEFAULT_INTERVAL_SEC)
    }

    fn set_phase(&self, phase: &str) {
        self.state.lock().unwrap().phase = phase.to_string();
    }

    fn set_error(&self, message: Option<String>) {
        self.state.lock().unwrap().last_error = message;
    }

    fn set_progress(&self, update: impl FnOnce(&mut SyncBootstrapProgress)) {
        let mut state = self.state.lock().unwrap();
        update(&mut state.progress);
    }

    /// Register the background job and spawn the cancellable sync loop.
    pub fn start_loop(self: &Arc<Self>, tasks: TaskSupervisor) -> TaskSpawnOutcome {
        let interval = self.configured_interval();
        self.background_jobs.register_job(
            JOB_NAME,
            "rust-host",
            Some(interval),
            vrcx_0_application_core::RuntimeOperationStatus::Scheduled,
            "Remote database sync loop registered.",
        );
        let engine = Arc::clone(self);
        tasks.spawn_cancellable(move |stop_token| async move {
            let _ = engine.run_one_cycle().await;
            loop {
                let interval = Duration::from_secs(engine.configured_interval());
                tokio::select! {
                    continued = sleep_until_due_or_stopped(interval, &stop_token) => {
                        if !continued {
                            return;
                        }
                    }
                    _ = engine.waker.notified() => {}
                }
                if let Err(error) = engine.run_one_cycle().await {
                    tracing::warn!(error = %error, "remote sync cycle failed");
                }
            }
        })
    }

    pub async fn run_one_cycle(self: &Arc<Self>) -> EngineResult<()> {
        let _guard = self.cycle_lock.lock().await;
        self.background_jobs
            .mark_running(JOB_NAME, "Remote sync cycle running.");
        let result = self.cycle_inner().await;
        match &result {
            Ok((pushed, pulled)) => {
                self.record_cycle_stats(*pushed, *pulled);
                self.set_error(None);
                self.set_phase("idle");
                self.background_jobs
                    .mark_completed(JOB_NAME, "Remote sync cycle completed.");
            }
            Err(error) => {
                let message = error.to_string();
                self.set_error(Some(message.clone()));
                self.set_phase("error");
                self.background_jobs.mark_failed(JOB_NAME, message);
            }
        }
        result.map(|_| ())
    }

    fn record_cycle_stats(&self, pushed: u64, pulled: u64) {
        let mut state = self.state.lock().unwrap();
        state.stats = CycleStats {
            pushed,
            pulled,
            finished_at: Some(now_iso()),
        };
    }

    fn stats_snapshot(&self) -> (u64, u64, Option<String>) {
        let state = self.state.lock().unwrap();
        (
            state.stats.pushed,
            state.stats.pulled,
            state.stats.finished_at.clone(),
        )
    }

    async fn cycle_inner(self: &Arc<Self>) -> EngineResult<(u64, u64)> {
        self.reconcile_remote_epoch().await?;

        // Register presence so other devices can list us.
        let _ = self
            .store
            .devices_upsert(&SyncDeviceRecord {
                device_id: self.handle.device_id().to_string(),
                app_version: self.app_version.clone(),
                last_push_at: Some(now_iso()),
                last_pull_at: Some(now_iso()),
            })
            .await;

        let bootstrap_pending = sync_meta_get(&self.db, META_BOOTSTRAP_PENDING)?
            .map(|value| value == "1")
            .unwrap_or(false);
        if bootstrap_pending || bootstrap_state_get(&self.db)?.is_some() {
            self.run_bootstrap().await?;
            return Ok((0, 0));
        }

        // New per-user tables may have appeared since the last cycle.
        refresh_capture(&self.db, &self.handle)?;
        self.ensure_remote_schema_if_changed().await?;

        let pushed = self.push_until_drained().await?;
        let pulled = self.pull_until_caught_up().await?;
        self.reconcile_if_gap().await?;
        self.maybe_gc_ops().await?;
        Ok((pushed, pulled))
    }

    async fn push_until_drained(self: &Arc<Self>) -> EngineResult<u64> {
        let mut pushed = 0u64;
        for _ in 0..PUSH_BATCHES_PER_CYCLE {
            let batch = outbox_take(&self.db, &self.handle, PUSH_BATCH)?;
            let Some(batch) = batch else {
                return Ok(pushed);
            };
            if batch.ops.is_empty() {
                outbox_trim_pushed(&self.db, batch.max_seq)?;
                continue;
            }
            pushed += batch.ops.len() as u64;
            self.store.push_ops(&batch.ops).await?;
            outbox_trim_pushed(&self.db, batch.max_seq)?;
        }
        Ok(pushed)
    }

    async fn pull_until_caught_up(self: &Arc<Self>) -> EngineResult<u64> {
        let mut applied = 0u64;
        for _ in 0..PULL_BATCHES_PER_CYCLE {
            let cursor = self.pull_cursor()?;
            let pulled = self.store.fetch_ops(cursor, PULL_BATCH).await?;
            if pulled.is_empty() {
                return Ok(applied);
            }
            applied += pulled.len() as u64;
            self.apply_pulled(pulled)?;
        }
        Ok(applied)
    }

    fn apply_pulled(self: &Arc<Self>, pulled: Vec<PulledOp>) -> EngineResult<()> {
        let mut new_cursor = 0i64;
        let mut ops = Vec::with_capacity(pulled.len());
        for item in pulled {
            new_cursor = new_cursor.max(item.server_seq);
            if let Some(hlc) = SyncHlc::decode(&item.op.hlc) {
                self.handle.observe(&hlc);
            }
            ops.push(item.op);
        }
        apply_pulled_ops(&self.db, self.handle.device_id(), &ops, new_cursor)?;
        Ok(())
    }

    fn pull_cursor(&self) -> EngineResult<i64> {
        Ok(sync_meta_get(&self.db, META_PULL_CURSOR)?
            .and_then(|value| value.parse().ok())
            .unwrap_or(0))
    }

    /// If op GC removed facts we never pulled, rebuild from materialized
    /// state (which already merged them) and jump the cursor forward.
    async fn reconcile_if_gap(self: &Arc<Self>) -> EngineResult<()> {
        let cursor = self.pull_cursor()?;
        let first = self.store.first_retained_seq().await?;
        if first == 0 || cursor + 1 >= first {
            return Ok(());
        }
        tracing::info!(
            cursor,
            first_retained = first,
            "sync op log was GC'd past the local cursor; reconciling from materialized state"
        );
        self.set_phase("reconciling");
        self.merge_materialized_state().await?;
        let latest = self.store.latest_seq().await?;
        sync_meta_set(&self.db, META_PULL_CURSOR, &latest.to_string())?;
        Ok(())
    }

    async fn maybe_gc_ops(self: &Arc<Self>) -> EngineResult<()> {
        let last = sync_meta_get(&self.db, META_LAST_OP_GC_AT)?
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(0);
        let now = chrono_now_hours();
        if now - last < GC_INTERVAL_HOURS {
            return Ok(());
        }
        let retain_days = vrcx_0_persistence::config::get_string(
            &self.db,
            CONFIG_RETAIN_DAYS,
            &DEFAULT_RETAIN_DAYS.to_string(),
        )
        .ok()
        .and_then(|raw| raw.trim().parse().ok())
        .filter(|days| *days >= 7)
        .unwrap_or(DEFAULT_RETAIN_DAYS);
        let removed = self.store.ops_gc(retain_days).await?;
        sync_meta_set(&self.db, META_LAST_OP_GC_AT, &now.to_string())?;
        tracing::debug!(removed, "remote sync op GC completed");
        Ok(())
    }

    // ───────────────────────────── bootstrap ─────────────────────────────

    /// When the remote database was wiped or rebuilt (its epoch changed),
    /// discard the local bootstrap state and re-run the first-time merge —
    /// the local database is always the source of truth for a full re-upload.
    async fn reconcile_remote_epoch(&self) -> EngineResult<()> {
        let Ok(Some(remote_epoch)) = self.store.server_meta_get("sync.epoch").await else {
            return Ok(());
        };
        if remote_epoch.is_empty() {
            return Ok(());
        }
        let local_epoch = vrcx_0_persistence::sync::sync_meta_get(&self.db, META_REMOTE_EPOCH)
            .map_err(SyncEngineError::from)?
            .unwrap_or_default();
        if local_epoch != remote_epoch {
            tracing::info!(
                remote = %remote_epoch,
                "remote sync epoch changed; re-arming first-time bootstrap"
            );
            vrcx_0_persistence::sync::sync_meta_remove(&self.db, META_BOOTSTRAP_STATE)
                .map_err(SyncEngineError::from)?;
            vrcx_0_persistence::sync::sync_meta_remove(&self.db, META_BOOTSTRAPPED_AT)
                .map_err(SyncEngineError::from)?;
            sync_meta_set(&self.db, META_REMOTE_EPOCH, &remote_epoch)?;
            self.mark_bootstrap_pending()?;
            self.set_progress(|progress| *progress = SyncBootstrapProgress::default());
        }
        Ok(())
    }

    /// Mark a first-time bootstrap as pending; the loop picks it up on the
    /// next cycle (and `trigger_now` wakes the loop immediately).
    pub fn mark_bootstrap_pending(&self) -> EngineResult<()> {
        let bootstrapped = vrcx_0_persistence::sync::sync_meta_get(&self.db, META_BOOTSTRAPPED_AT)
            .map_err(SyncEngineError::from)?
            .is_some();
        if !bootstrapped {
            sync_meta_set(&self.db, META_BOOTSTRAP_PENDING, "1")?;
        }
        Ok(())
    }

    async fn run_bootstrap(self: &Arc<Self>) -> EngineResult<()> {
        // Resuming after a restart (state already persisted): the store's
        // materialization schema cache is empty in the new process — always
        // ensure before pushing, or pushes correctly refuse to log ops they
        // cannot materialize.
        self.ensure_remote_schema_if_changed().await?;

        let mut state = match bootstrap_state_get(&self.db)? {
            Some(state) => state,
            None => {
                // First entry: ensure the remote schema and decide the phase.
                let schema = remote_schema_snapshot(&self.db)?;
                let remote_version = self
                    .store
                    .ensure_schema(&schema, SYNC_PROTOCOL_SCHEMA_VERSION)
                    .await?;
                sync_meta_set(
                    &self.db,
                    "sync.remoteSchemaVersion",
                    &remote_version.to_string(),
                )?;
                let tables = synced_local_tables(&self.db)?
                    .into_iter()
                    .map(|plan| plan.table)
                    .collect::<Vec<_>>();
                let table_rows = tables
                    .iter()
                    .map(|table| {
                        vrcx_0_persistence::sync::table_row_count(&self.db, table).unwrap_or(0)
                    })
                    .collect::<Vec<_>>();
                let rows_total = table_rows.iter().sum::<i64>();
                BootstrapState {
                    phase: if self.store.remote_is_empty().await? {
                        "push".into()
                    } else {
                        "merge".into()
                    },
                    table_index: 0,
                    last_rowid: 0,
                    tables,
                    rows_total,
                    table_rows,
                }
            }
        };
        self.set_phase("bootstrap");
        // The in-memory progress resets on app restart while the persisted
        // bootstrap state continues — re-arm the live display every entry.
        let table_progress = state
            .tables
            .iter()
            .enumerate()
            .map(|(index, name)| SyncTableProgress {
                name: name.clone(),
                rows_total: state.table_rows.get(index).copied().unwrap_or(0),
                rows_done: 0,
                done: index < state.table_index,
            })
            .collect::<Vec<_>>();
        let tables_before = state.table_index;
        self.set_progress(|progress| {
            progress.running = true;
            progress.rows_total = state.rows_total.max(progress.rows_total as i64);
            progress.tables_total = state.tables.len() as u32;
            progress.tables_done = state.table_index as u32;
            progress.phase = state.phase.clone();
            progress.current_table_rows_total =
                state.table_rows.get(tables_before).copied().unwrap_or(0);
            progress.tables = table_progress;
        });
        match state.phase.as_str() {
            "merge" => {
                // Existing fleet: adopt remote state first, then fall through
                // to the push phase on the next cycle.
                self.set_progress(|progress| progress.phase = "merge".into());
                self.merge_materialized_state().await?;
                let latest = self.store.latest_seq().await?;
                sync_meta_set(&self.db, META_PULL_CURSOR, &latest.to_string())?;
                state.phase = "push".into();
                state.table_index = 0;
                state.last_rowid = 0;
                bootstrap_state_set(&self.db, &state)?;
                self.set_progress(|progress| progress.phase = "push".into());
                return Ok(());
            }
            "push" => {
                self.push_bootstrap_tables(&mut state).await?;
            }
            other => {
                return Err(SyncEngineError::Local(format!(
                    "Unknown bootstrap phase: {other}"
                )));
            }
        }

        // Bootstrap finished: clear the markers and settle with a normal cycle.
        sync_meta_remove(&self.db, META_BOOTSTRAP_STATE)?;
        sync_meta_remove(&self.db, META_BOOTSTRAP_PENDING)?;
        sync_meta_set(&self.db, META_BOOTSTRAPPED_AT, &now_iso())?;
        let _ = self
            .store
            .server_meta_set("sync.provisionedAt", &now_iso())
            .await;
        if let Ok(Some(epoch)) = self.store.server_meta_get("sync.epoch").await {
            if !epoch.is_empty() {
                let _ = sync_meta_set(&self.db, META_REMOTE_EPOCH, &epoch);
            }
        }
        self.set_progress(|progress| {
            progress.running = false;
            progress.phase = "done".into();
        });
        self.set_phase("idle");
        self.push_until_drained().await?;
        self.pull_until_caught_up().await?;
        Ok(())
    }

    async fn push_bootstrap_tables(
        self: &Arc<Self>,
        state: &mut BootstrapState,
    ) -> EngineResult<()> {
        while state.table_index < state.tables.len() {
            let table = state.tables[state.table_index].clone();
            let table_total = state
                .table_rows
                .get(state.table_index)
                .copied()
                .unwrap_or(0);
            self.set_progress(|progress| {
                progress.current_table = table.clone();
                progress.phase = "push".into();
                progress.current_table_rows_total = table_total;
                progress.current_table_rows_done = 0;
                progress.tables_done = state.table_index as u32;
            });
            let plans = synced_local_tables(&self.db)?;
            let Some(plan) = plans.iter().find(|plan| plan.table == table) else {
                state.table_index += 1;
                state.last_rowid = 0;
                continue;
            };
            loop {
                let chunk =
                    read_table_chunk(&self.db, &table, state.last_rowid, BOOTSTRAP_CHUNK, None)?;
                let Some(chunk) = chunk else {
                    break;
                };
                if chunk.rows.is_empty() {
                    break;
                }
                let mut ops = Vec::with_capacity(chunk.rows.len());
                for (index, row) in chunk.rows.iter().enumerate() {
                    let hlc = bootstrap_row_hlc(&chunk.columns, row, &self.handle);
                    let row_ops = bootstrap_set_ops(
                        &self.db,
                        plan,
                        self.handle.device_id(),
                        &hlc,
                        (state.table_index as u64) * 1_000_000 + index as u64,
                        &chunk.columns,
                        row,
                    )?;
                    ops.extend(row_ops);
                }
                if !ops.is_empty() {
                    self.store.push_ops(&ops).await?;
                }
                state.last_rowid = chunk.next_rowid;
                let uploaded = ops.len() as u64;
                self.set_progress(|progress| {
                    progress.rows_done += uploaded;
                    progress.current_table_rows_done += uploaded;
                });
                bootstrap_state_set(&self.db, state)?;
                if chunk.rows.len() < BOOTSTRAP_CHUNK as usize {
                    break;
                }
            }
            state.table_index += 1;
            state.last_rowid = 0;
            let finished = state.table_index;
            self.set_progress(|progress| {
                progress.tables_done += 1;
                progress.current_table_rows_done = 0;
                if let Some(entry) = progress.tables.iter_mut().find(|entry| entry.name == table) {
                    entry.done = true;
                    entry.rows_done = entry.rows_total.max(0) as u64;
                }
                let _ = finished;
            });
            bootstrap_state_set(&self.db, state)?;
        }
        Ok(())
    }

    /// Pull the remote materialized state of every catalog table and merge it
    /// locally through the same lattice merge as ops. Used for first-merge on
    /// a non-empty remote and for post-GC reconciliation.
    async fn merge_materialized_state(self: &Arc<Self>) -> EngineResult<()> {
        let tables = synced_local_tables(&self.db)?
            .into_iter()
            .map(|plan| plan.table)
            .collect::<Vec<_>>();
        for table in tables {
            let Some(descriptor) = sync_table_descriptor(&table) else {
                continue;
            };
            let mut offset = 0i64;
            loop {
                let rows = self
                    .store
                    .fetch_materialized(&table, BOOTSTRAP_MATERIALIZED_PAGE, offset)
                    .await?;
                if rows.is_empty() {
                    break;
                }
                let ops = rows
                    .iter()
                    .filter_map(|row| materialized_set_op(descriptor, &table, row))
                    .collect::<Vec<_>>();
                if !ops.is_empty() {
                    apply_pulled_ops(&self.db, self.handle.device_id(), &ops, self.pull_cursor()?)?;
                }
                offset += rows.len() as i64;
                self.set_progress(|progress| progress.rows_done += rows.len() as u64);
                if (rows.len() as i64) < BOOTSTRAP_MATERIALIZED_PAGE {
                    break;
                }
            }
        }
        Ok(())
    }

    /// Run exactly one cycle and return the fresh status; used by the
    /// "sync now" button so it can show a loading state and a result toast.
    pub async fn run_cycle_and_status(self: &Arc<Self>) -> EngineResult<SyncStatusSnapshot> {
        let result = self.run_one_cycle().await;
        let status = self.status().await;
        result.map(|()| status)
    }

    /// Phase and last error without holding the lock across an await.
    fn phase_snapshot(&self) -> (String, Option<String>) {
        let state = self.state.lock().unwrap();
        (state.phase.clone(), state.last_error.clone())
    }

    /// Re-ensure the remote materialized schema when the local table set
    /// changed (new per-user tables, new columns). The hash avoids DDL
    /// round-trips on every cycle.
    async fn ensure_remote_schema_if_changed(self: &Arc<Self>) -> EngineResult<()> {
        let schema = remote_schema_snapshot(&self.db)?;
        let hash = schema_hash(&schema);
        if *self.ensured_schema_hash.lock().unwrap() == Some(hash) {
            return Ok(());
        }
        self.store
            .ensure_schema(&schema, SYNC_PROTOCOL_SCHEMA_VERSION)
            .await?;
        *self.ensured_schema_hash.lock().unwrap() = Some(hash);
        Ok(())
    }

    pub async fn status(self: &Arc<Self>) -> SyncStatusSnapshot {
        let (phase, last_error) = self.phase_snapshot();

        let pending = outbox_pending_count(&self.db).unwrap_or(0);
        let cursor = self.pull_cursor().unwrap_or(0);
        let configured = !vrcx_0_persistence::config::get_string(&self.db, CONFIG_HOST, "")
            .unwrap_or_default()
            .trim()
            .is_empty()
            && !vrcx_0_persistence::config::get_string(&self.db, CONFIG_DATABASE, "")
                .unwrap_or_default()
                .trim()
                .is_empty();
        let last_push_at = sync_meta_get(&self.db, META_LAST_PUSH_AT).ok().flatten();
        let last_pull_at = sync_meta_get(&self.db, META_LAST_PULL_AT).ok().flatten();
        let remote_schema_version = sync_meta_get(&self.db, "sync.remoteSchemaVersion")
            .ok()
            .flatten()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        let remote_devices = match self.store.devices_list().await {
            Ok(devices) => devices,
            Err(error) => {
                tracing::debug!(error = %error, "failed to list remote sync devices");
                Vec::new()
            }
        };
        let (pushed_ops, pulled_ops, last_cycle_at) = self.stats_snapshot();
        SyncStatusSnapshot {
            enabled: true,
            configured,
            last_pushed_ops: pushed_ops,
            last_pulled_ops: pulled_ops,
            last_cycle_at: last_cycle_at.or(last_pull_at.clone()),
            phase,
            device_id: self.handle.device_id().to_string(),
            last_push_at,
            last_pull_at,
            pending_outbox: pending,
            pull_cursor: cursor,
            last_error,
            remote_schema_version,
            remote_devices,
        }
    }
}

/// Stamp a bootstrap op from the row's own business timestamp when one exists,
/// so first-merge last-write-wins compares content age rather than push time.
fn bootstrap_row_hlc(columns: &[String], row: &[Value], handle: &SyncCaptureHandle) -> String {
    let payload: Map<String, Value> = columns
        .iter()
        .zip(row.iter())
        .map(|(column, value)| (column.clone(), value.clone()))
        .collect();
    for field in ["updated_at", "edited_at", "created_at", "last_viewed_at"] {
        if let Some(Value::String(text)) = payload.get(field) {
            if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(text) {
                let ms = parsed.timestamp_millis().max(0) as u64;
                return SyncHlc::new(ms, handle.device_id().to_string()).encode();
            }
        }
    }
    handle.tick()
}

/// Rebuild a Set op from a materialized remote row for local merge.
fn materialized_set_op(
    descriptor: &vrcx_0_contracts::SyncTableDescriptor,
    table: &str,
    row: &MaterializedRow,
) -> Option<SyncOpRecord> {
    let entity_key: Vec<Value> = descriptor
        .key_columns
        .iter()
        .map(|column| row.columns.get(*column).cloned().unwrap_or(Value::Null))
        .collect();
    Some(SyncOpRecord {
        op_id: format!(
            "mat/{}/{}/{}",
            table,
            row.sync_hlc,
            serde_json::to_string(&entity_key).unwrap_or_default()
        ),
        table: table.to_string(),
        entity_key,
        kind: SyncOpKind::Set,
        payload: Some(row.columns.clone()),
        hlc: if row.sync_hlc.is_empty() {
            SyncHlc::new(0, row.sync_device.clone()).encode()
        } else {
            row.sync_hlc.clone()
        },
        device: if row.sync_device.is_empty() {
            "remote-materialized".to_string()
        } else {
            row.sync_device.clone()
        },
    })
}

/// Build the remote schema snapshot from the local database.
fn remote_schema_snapshot(db: &DatabaseService) -> EngineResult<Vec<RemoteTableSchema>> {
    let mut schema = Vec::new();
    for plan in synced_local_tables(db)? {
        let column_types = vrcx_0_persistence::sync::table_payload_column_types(db, &plan.table)?;
        let element_fields: Vec<String> = plan
            .descriptor
            .field_semantics
            .iter()
            .filter(|(_, semantic)| matches!(semantic, SyncFieldSemantic::ElementSet))
            .map(|(field, _)| field.to_string())
            .collect();
        let columns = column_types
            .into_iter()
            .map(|(name, declared)| {
                let column_type = if element_fields.contains(&name) {
                    RemoteColumnType::Jsonb
                } else {
                    match declared.to_ascii_uppercase().as_str() {
                        "INTEGER" | "INT" | "BIGINT" | "BOOLEAN" | "BOOL" => {
                            RemoteColumnType::BigInt
                        }
                        "REAL" | "FLOAT" | "DOUBLE" => RemoteColumnType::Double,
                        _ => RemoteColumnType::Text,
                    }
                };
                RemoteColumnDef { name, column_type }
            })
            .collect();
        schema.push(RemoteTableSchema {
            table: plan.table.clone(),
            columns,
            key_columns: plan
                .descriptor
                .key_columns
                .iter()
                .map(|k| k.to_string())
                .collect(),
            row_semantic: plan.descriptor.row_semantic,
            field_semantics: plan
                .descriptor
                .field_semantics
                .iter()
                .map(|(field, semantic)| (field.to_string(), *semantic))
                .collect(),
        });
    }
    Ok(schema)
}

fn schema_hash(schema: &[RemoteTableSchema]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for table in schema {
        table.table.hash(&mut hasher);
        for column in &table.columns {
            column.name.hash(&mut hasher);
            column.column_type.hash(&mut hasher);
        }
    }
    hasher.finish()
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn chrono_now_hours() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64 / 3600)
        .unwrap_or(0)
}

/// One-shot connection probe for the settings UI.
pub async fn test_store_connection(store: &Arc<dyn RemoteSyncStore>) -> SyncConnectionTestResult {
    store
        .test_connection()
        .await
        .unwrap_or_else(|error| SyncConnectionTestResult {
            ok: false,
            server_version: String::new(),
            latency_ms: 0,
            error: Some(error.to_string()),
        })
}

/// Provision the device id without a running engine (status before start).
pub fn local_device_id(db: &DatabaseService) -> EngineResult<String> {
    sync_device_id(db).map_err(SyncEngineError::from)
}
