//! Outbound port for the remote sync store.
//!
//! PostgreSQL is the reference implementation (in `vrcx-0-outbound-adapters`);
//! the engine only depends on this trait. The contract assumes at-least-once
//! delivery with idempotent application: `push_ops` is transactional and
//! deduplicates by op id, `fetch_ops` returns a strict total order (server
//! arrival sequence), and materialized rows carry the row watermark so a
//! first-merge can be replayed through the same lattice merge as ops.

use async_trait::async_trait;
use serde_json::{Map, Value};
use vrcx_0_contracts::{
    SyncConnectionTestResult, SyncDeviceRecord, SyncFieldSemantic, SyncOpRecord, SyncRowSemantic,
};

/// Column type on the materialized remote schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum RemoteColumnType {
    Text,
    BigInt,
    Double,
    /// JSON-array columns merged per element (`ElementSet` fields).
    Jsonb,
}

#[derive(Clone, Debug)]
pub struct RemoteColumnDef {
    pub name: String,
    pub column_type: RemoteColumnType,
}

/// Everything the store needs to create one materialized table.
#[derive(Clone, Debug)]
pub struct RemoteTableSchema {
    pub table: String,
    pub columns: Vec<RemoteColumnDef>,
    pub key_columns: Vec<String>,
    pub row_semantic: SyncRowSemantic,
    pub field_semantics: Vec<(String, SyncFieldSemantic)>,
}

#[derive(Clone, Debug)]
pub struct PulledOp {
    /// Server arrival sequence; strictly increasing, doubles as the pull
    /// cursor so every replica applies order-sensitive merges (element sets)
    /// in the same order the single serialization point produced.
    pub server_seq: i64,
    pub op: SyncOpRecord,
}

#[derive(Clone, Debug)]
pub struct MaterializedRow {
    pub sync_hlc: String,
    pub sync_device: String,
    pub columns: Map<String, Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum SyncStoreError {
    #[error(
        "Remote sync schema version {remote} is newer than this app supports ({supported}); update the app first."
    )]
    RemoteSchemaTooNew { remote: i64, supported: i64 },
    #[error("{0}")]
    Other(String),
}

impl From<String> for SyncStoreError {
    fn from(message: String) -> Self {
        Self::Other(message)
    }
}

impl From<&str> for SyncStoreError {
    fn from(message: &str) -> Self {
        Self::Other(message.to_string())
    }
}

pub type SyncStoreResult<T> = std::result::Result<T, SyncStoreError>;

#[async_trait]
pub trait RemoteSyncStore: Send + Sync {
    /// Connectivity + authentication probe with server version and latency.
    async fn test_connection(&self) -> SyncStoreResult<SyncConnectionTestResult>;

    async fn server_meta_get(&self, key: &str) -> SyncStoreResult<Option<String>>;
    async fn server_meta_set(&self, key: &str, value: &str) -> SyncStoreResult<()>;

    /// Create/upgrade the protocol tables and materialized business schema.
    /// The engine passes a schema snapshot built from the local database
    /// (columns are discovered there at runtime). Returns the remote protocol
    /// version after ensuring it; fails with
    /// [`SyncStoreError::RemoteSchemaTooNew`] when the remote is ahead.
    async fn ensure_schema(
        &self,
        tables: &[RemoteTableSchema],
        supported_version: i64,
    ) -> SyncStoreResult<i64>;

    /// True when no facts were ever pushed (first device bootstraps).
    async fn remote_is_empty(&self) -> SyncStoreResult<bool>;

    /// Highest issued server sequence (0 when empty).
    async fn latest_seq(&self) -> SyncStoreResult<i64>;

    /// Lowest retained server sequence (used to detect GC gaps).
    async fn first_retained_seq(&self) -> SyncStoreResult<i64>;

    /// Append facts and materialize them transactionally; idempotent by op id.
    async fn push_ops(&self, ops: &[SyncOpRecord]) -> SyncStoreResult<()>;

    /// Facts strictly after the cursor, in arrival order.
    async fn fetch_ops(&self, cursor: i64, limit: i64) -> SyncStoreResult<Vec<PulledOp>>;

    /// Materialized current state of one table for first-merge/reconciliation.
    async fn fetch_materialized(
        &self,
        table: &str,
        limit: i64,
        offset: i64,
    ) -> SyncStoreResult<Vec<MaterializedRow>>;

    async fn devices_upsert(&self, record: &SyncDeviceRecord) -> SyncStoreResult<()>;
    async fn devices_list(&self) -> SyncStoreResult<Vec<SyncDeviceRecord>>;

    /// Drop ops older than the retention window; returns rows removed.
    async fn ops_gc(&self, retain_days: i64) -> SyncStoreResult<u64>;
}
