//! Remote database sync engine.
//!
//! Data model and merge semantics live in `vrcx-0-contracts::sync`; local
//! capture and materialization live in `vrcx-0-persistence::sync`. This crate
//! owns the orchestration: the [`RemoteSyncStore`] port, the push/pull
//! engine, first-merge bootstrap, post-GC reconciliation and the background
//! loop.

pub mod engine;
pub mod store;

pub use engine::{
    local_device_id, test_store_connection, RemoteSyncEngine, SyncEngineError,
    SyncStatusChangedEvent, CONFIG_ALLOW_PLAINTEXT, CONFIG_DATABASE, CONFIG_ENABLED, CONFIG_HOST,
    CONFIG_INTERVAL_SEC, CONFIG_PASSWORD, CONFIG_PORT, CONFIG_RETAIN_DAYS, CONFIG_TLS_VERIFY,
    CONFIG_USER, DEFAULT_INTERVAL_SEC, DEFAULT_RETAIN_DAYS, JOB_NAME,
};
pub use store::{
    MaterializedRow, PulledOp, RemoteColumnDef, RemoteColumnType, RemoteSyncStore,
    RemoteTableSchema, SyncStoreError, SyncStoreResult,
};
