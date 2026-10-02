//! Remote database sync protocol kernel.
//!
//! Pure data types shared by three layers:
//! - `vrcx-0-persistence` (local change capture in `_sync_outbox`),
//! - `vrcx-0-application-sync` (merge engine, push/pull orchestration),
//! - `vrcx-0-outbound-adapters` (PostgreSQL materialization SQL).
//!
//! The sync model: every change is an immutable fact (`SyncOpRecord`) addressed
//! by a natural key. Facts are appended to the remote `sync_ops` log and
//! materialized into remote tables inside the same transaction; PostgreSQL is
//! the single serialization point. Each field merges with the semantics that is
//! mathematically correct for its data shape, so concurrent writes from
//! multiple clients converge deterministically instead of being arbitrated.

use serde::{Deserialize, Serialize};

/// Hybrid logical clock timestamp.
///
/// Encoded as `<physical-ms:017 decimal>-<logical:08 decimal>-<device>`; every
/// component is fixed-width, so plain lexicographic string comparison equals
/// (physical, logical, device) tuple comparison — a total order across
/// devices. Decimal milliseconds is deliberate: SQLite capture triggers stamp
/// the same shape with `strftime`, so write-time stamps produced inside the
/// database and engine-generated stamps compare directly. `device` only breaks
/// ties when two clocks emit the same (physical, logical) pair, so
/// millisecond-identical concurrent writes still have exactly one
/// deterministic winner on every replica.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct SyncHlc {
    pub physical_ms: u64,
    pub logical: u32,
    pub device: String,
}

impl SyncHlc {
    pub fn new(physical_ms: u64, device: impl Into<String>) -> Self {
        Self {
            physical_ms,
            logical: 0,
            device: device.into(),
        }
    }

    /// Zero-padded canonical form; lexicographic order == causal order.
    pub fn encode(&self) -> String {
        format!(
            "{:017}-{:08}-{}",
            self.physical_ms, self.logical, self.device
        )
    }

    pub fn decode(encoded: &str) -> Option<Self> {
        let mut parts = encoded.splitn(3, '-');
        let physical = parts.next()?;
        let logical = parts.next()?;
        let device = parts.next()?;
        Some(Self {
            physical_ms: physical.parse().ok()?,
            logical: logical.parse().ok()?,
            device: device.to_string(),
        })
    }

    /// Advances this clock for a local event, preserving monotonicity even
    /// when the wall clock jumps backwards.
    pub fn tick(&mut self, now_ms: u64) {
        if now_ms > self.physical_ms {
            self.physical_ms = now_ms;
            self.logical = 0;
        } else {
            self.logical = self.logical.saturating_add(1);
        }
    }

    /// Merges a remote timestamp observed via pull, keeping the clock ahead of
    /// both the local wall clock and every observed event.
    pub fn observe(&mut self, other: &SyncHlc, now_ms: u64) {
        if other.physical_ms > self.physical_ms {
            self.physical_ms = other.physical_ms;
            self.logical = other.logical + 1;
        } else if other.physical_ms == self.physical_ms && other.logical > self.logical {
            self.logical = other.logical + 1;
        }
        self.tick(now_ms);
    }

    /// Comparison key for LWW decisions: the encoded form.
    pub fn order_key(&self) -> String {
        self.encode()
    }
}

/// The kind of change a sync op represents.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncOpKind {
    /// Upsert a full row snapshot by natural key.
    Set,
    /// Delete the row addressed by the natural key (add-wins over older sets).
    Delete,
    /// Add a delta to a counter field: payload `{ "field", "delta" }`.
    Inc,
    /// Add an element to a JSON-array field: payload `{ "field", "element" }`.
    SetAdd,
    /// Remove an element from a JSON-array field: payload `{ "field", "element" }`.
    SetRemove,
}

/// One immutable change fact flowing between replicas.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncOpRecord {
    /// Unique, monotonic per device: `<hlc>`. Remote uses it for idempotent
    /// dedup of at-least-once delivery.
    pub op_id: String,
    /// Physical table name on the originating device (already prefix-expanded
    /// for per-user tables).
    pub table: String,
    /// Natural-key values identifying the row across devices.
    pub entity_key: Vec<serde_json::Value>,
    pub kind: SyncOpKind,
    /// Row snapshot (`Set`), field payload (`Inc`/`SetAdd`/`SetRemove`), or null.
    #[serde(default)]
    pub payload: Option<serde_json::Map<String, serde_json::Value>>,
    pub hlc: String,
    pub device: String,
}

/// How row identity and existence merge.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncRowSemantic {
    /// Insert-only fact: same natural key is the same fact everywhere.
    /// Updates only touch fields carrying explicit field semantics.
    GSet,
    /// Mutable row: every field defaults to last-write-wins by HLC.
    Lww,
}

/// How an individual field merges when facts collide.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncFieldSemantic {
    /// Last write wins by (hlc, device). The only sound automatic merge for
    /// concurrently edited scalars.
    Lww,
    /// Boolean OR: once true, never reverted (`seen`, `expired`).
    OrBool,
    /// Maximum by comparison (`expires_at`, `updated_at`, `last_viewed_at`).
    MaxValue,
    /// Positive counter merged as deltas against the last synced snapshot
    /// (`view_count`): concurrent increments sum, none are lost.
    CounterDelta,
    /// JSON array merged per element in op arrival order
    /// (`favorite_group_collection.group_ids`).
    ElementSet,
}

/// One entry of the synced-table catalog.
#[derive(Clone, Debug)]
pub struct SyncTableDescriptor {
    /// Table name; per-user tables use the `{p}_` prefix placeholder.
    pub template: &'static str,
    /// True when the name is a per-user template instantiated per account.
    pub per_user: bool,
    /// Natural-key columns identifying the row across devices.
    pub key_columns: &'static [&'static str],
    pub row_semantic: SyncRowSemantic,
    /// Field overrides; every other column follows the row semantic.
    pub field_semantics: &'static [(&'static str, SyncFieldSemantic)],
}

impl SyncTableDescriptor {
    pub fn matches_table(&self, table: &str) -> bool {
        if self.per_user {
            let Some(rest) = table.strip_suffix(self.template) else {
                return false;
            };
            // The remainder must look like a real user table prefix: VRChat
            // user ids are `usr_<hex>`, normalized to `usr<hex>` (a leading
            // underscore is prepended when the id starts with a digit). This
            // stops global tables that merely end with a template name
            // (e.g. a future `widget_notes`) from hijacking per-user
            // descriptors like `_notes`.
            looks_like_user_prefix(rest)
        } else {
            table == self.template
        }
    }
}

/// A stripped remainder plausibly identifies a per-user table prefix
/// (normalized `usr<hex>` VRChat id, or `_digit…` for digit-leading ids).
pub fn looks_like_user_prefix(prefix: &str) -> bool {
    if let Some(rest) = prefix.strip_prefix("usr") {
        return !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_alphanumeric());
    }
    if let Some(rest) = prefix.strip_prefix('_') {
        return rest.chars().next().is_some_and(|ch| ch.is_ascii_digit())
            && rest.chars().all(|ch| ch.is_ascii_alphanumeric());
    }
    false
}

/// The `configs` table is synced with a key blacklist: credentials and
/// machine-local keys never leave the device. Keys are matched in their
/// normalized `config:<lowercase>` form (the obfuscation registry stores
/// them as `config:vrcx_<name>`). Sealed secret values (`enc1:` prefix) are
/// additionally filtered by value in the capture layer because they are
/// encrypted with a machine-bound key and useless on other devices.
pub const SYNC_EXCLUDED_CONFIG_KEYS: &[&str] = &[
    // Credential-bearing values (mirrors the obfuscation registry and secrets).
    "config:vrcx_assistant.apikey",
    "config:vrcx_integrationapitoken",
    "config:vrcx_llm.endpoints",
    "config:vrcx_mcpservertoken",
    "config:vrcx_shareownerkeys",
    "config:vrcx_translationapikey",
    "config:vrcx_webhookurl",
    "config:vrcx_youtubeapikey",
    // This feature's own database password.
    "config:remotesync.password",
    // Schema/machine-local bookkeeping.
    "config:vrcx_0_databaseversion",
    "config:databaseversion",
    // Legacy upstream VRCX credential keys (defensive).
    "config:lastfmusername",
    "config:lastfmpassword",
    "config:apikey",
    "config:authcookie",
    "config:twofactorauth",
    "config:userid",
];

pub fn is_config_key_syncable(normalized_key: &str) -> bool {
    !SYNC_EXCLUDED_CONFIG_KEYS.contains(&normalized_key)
}

/// Remote schema generation for the sync protocol itself.
pub const SYNC_PROTOCOL_SCHEMA_VERSION: i64 = 1;

/// The synced-table catalog. Credentials, entity caches, screenshot index and
/// derived activity read-models are deliberately absent: caches are
/// recomputed per device, `avatar_history` is derived from `avatar_wear_log`
/// (recomputed locally after pull), and credentials must never leave the
/// device.
pub const SYNC_TABLE_CATALOG: &[SyncTableDescriptor] = &[
    // ── Game log (append-only facts; location.time updated by single writer) ──
    SyncTableDescriptor {
        template: "gamelog_location",
        per_user: false,
        key_columns: &["created_at", "location"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[("time", SyncFieldSemantic::Lww)],
    },
    SyncTableDescriptor {
        template: "gamelog_join_leave",
        per_user: false,
        key_columns: &["created_at", "type", "display_name"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "gamelog_portal_spawn",
        per_user: false,
        key_columns: &["created_at", "display_name"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "gamelog_video_play",
        per_user: false,
        key_columns: &["created_at", "video_url"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "gamelog_resource_load",
        per_user: false,
        key_columns: &["created_at", "resource_url"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "gamelog_event",
        per_user: false,
        key_columns: &["created_at", "data"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "gamelog_external",
        per_user: false,
        key_columns: &["created_at", "message"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[],
    },
    // ── Global stores ──
    SyncTableDescriptor {
        template: "owners",
        per_user: false,
        key_columns: &["user_id"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "favorite_world",
        per_user: false,
        key_columns: &["world_id", "group_name"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "favorite_avatar",
        per_user: false,
        key_columns: &["avatar_id", "group_name"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "favorite_friend",
        per_user: false,
        key_columns: &["owner_id", "user_id", "group_name"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "favorite_group_collection",
        per_user: false,
        key_columns: &["id"],
        row_semantic: SyncRowSemantic::Lww,
        field_semantics: &[("group_ids", SyncFieldSemantic::ElementSet)],
    },
    SyncTableDescriptor {
        template: "memos",
        per_user: false,
        key_columns: &["user_id"],
        row_semantic: SyncRowSemantic::Lww,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "world_memos",
        per_user: false,
        key_columns: &["world_id"],
        row_semantic: SyncRowSemantic::Lww,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "avatar_memos",
        per_user: false,
        key_columns: &["avatar_id"],
        row_semantic: SyncRowSemantic::Lww,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "avatar_tags",
        per_user: false,
        key_columns: &["avatar_id", "tag"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "browse_history",
        per_user: false,
        key_columns: &["owner_user_id", "entity_kind", "entity_id"],
        row_semantic: SyncRowSemantic::Lww,
        field_semantics: &[
            ("last_viewed_at", SyncFieldSemantic::MaxValue),
            ("view_count", SyncFieldSemantic::CounterDelta),
        ],
    },
    SyncTableDescriptor {
        template: "assistant_session",
        per_user: false,
        key_columns: &["id"],
        row_semantic: SyncRowSemantic::Lww,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "assistant_message",
        per_user: false,
        key_columns: &["id"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "configs",
        per_user: false,
        key_columns: &["key"],
        row_semantic: SyncRowSemantic::Lww,
        field_semantics: &[],
    },
    // ── Per-user realtime tables ({p} = normalized user id prefix) ──
    SyncTableDescriptor {
        template: "_feed_gps",
        per_user: true,
        key_columns: &["created_at", "user_id", "previous_location"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[("time", SyncFieldSemantic::Lww)],
    },
    SyncTableDescriptor {
        template: "_feed_status",
        per_user: true,
        key_columns: &["created_at", "user_id", "previous_status"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "_feed_bio",
        per_user: true,
        key_columns: &["created_at", "user_id", "previous_bio"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "_feed_avatar",
        per_user: true,
        key_columns: &["created_at", "user_id", "previous_current_avatar_image_url"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "_feed_online_offline",
        per_user: true,
        key_columns: &["created_at", "user_id", "type"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "_self_profile_log",
        per_user: true,
        key_columns: &["created_at", "field", "value"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "_friend_log_current",
        per_user: true,
        key_columns: &["user_id"],
        row_semantic: SyncRowSemantic::Lww,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "_friend_log_history",
        per_user: true,
        key_columns: &["created_at", "type", "user_id", "previous_display_name"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "_notifications",
        per_user: true,
        key_columns: &["id"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[
            ("seen", SyncFieldSemantic::OrBool),
            ("expired", SyncFieldSemantic::OrBool),
        ],
    },
    SyncTableDescriptor {
        template: "_notifications_v2",
        per_user: true,
        key_columns: &["id"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[
            ("seen", SyncFieldSemantic::OrBool),
            ("updated_at", SyncFieldSemantic::MaxValue),
            ("expires_at", SyncFieldSemantic::MaxValue),
        ],
    },
    SyncTableDescriptor {
        template: "_avatar_wear_log",
        per_user: true,
        key_columns: &["avatar_id", "started_at"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "_moderation",
        per_user: true,
        key_columns: &["user_id"],
        row_semantic: SyncRowSemantic::Lww,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "_notes",
        per_user: true,
        key_columns: &["user_id"],
        row_semantic: SyncRowSemantic::Lww,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "_mutual_graph_friends",
        per_user: true,
        key_columns: &["friend_id"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "_mutual_graph_links",
        per_user: true,
        key_columns: &["friend_id", "mutual_id"],
        row_semantic: SyncRowSemantic::GSet,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "_mutual_graph_meta",
        per_user: true,
        key_columns: &["friend_id"],
        row_semantic: SyncRowSemantic::Lww,
        field_semantics: &[],
    },
    SyncTableDescriptor {
        template: "_profile_bio",
        per_user: true,
        key_columns: &["user_id"],
        row_semantic: SyncRowSemantic::Lww,
        field_semantics: &[],
    },
];

/// Tables registered at runtime (derived from the live schema, or declared
/// explicitly by app code when new tables ship). Storage is leaked on
/// purpose: the registry is bounded by the table count and lives for the
/// process lifetime, exactly like the static catalog.
static REGISTERED_TABLES: std::sync::RwLock<Vec<&'static SyncTableDescriptor>> =
    std::sync::RwLock::new(Vec::new());

/// Explicit registration interface for app code that ships a new table: call
/// this once (from the ensure-DDL layer) with the intended merge semantics.
/// Idempotent per template.
pub fn register_sync_table(descriptor: SyncTableDescriptor) {
    let leaked: &'static SyncTableDescriptor = Box::leak(Box::new(descriptor));
    let mut registry = REGISTERED_TABLES.write().unwrap();
    if !registry.iter().any(|existing| {
        existing.template == leaked.template && existing.per_user == leaked.per_user
    }) {
        registry.push(leaked);
    }
}

/// Convenience wrapper for runtime registration with owned strings.
#[allow(clippy::boxed_local)]
pub fn register_sync_table_owned(
    template: String,
    per_user: bool,
    key_columns: Vec<String>,
    row_semantic: SyncRowSemantic,
    field_semantics: Vec<(String, SyncFieldSemantic)>,
) {
    fn leak_str(value: String) -> &'static str {
        Box::leak(value.into_boxed_str())
    }
    let keys: &'static [&'static str] = Box::leak(
        key_columns
            .into_iter()
            .map(leak_str)
            .collect::<Vec<&'static str>>()
            .into_boxed_slice(),
    );
    let fields: &'static [(&'static str, SyncFieldSemantic)] = Box::leak(
        field_semantics
            .into_iter()
            .map(|(field, semantic)| (leak_str(field), semantic))
            .collect::<Vec<(&'static str, SyncFieldSemantic)>>()
            .into_boxed_slice(),
    );
    register_sync_table(SyncTableDescriptor {
        template: leak_str(template),
        per_user,
        key_columns: keys,
        row_semantic,
        field_semantics: fields,
    });
}

/// Find the descriptor for a physical table name: the curated static catalog
/// first, then runtime-registered tables (derived or declared).
pub fn sync_table_descriptor(table: &str) -> Option<&'static SyncTableDescriptor> {
    if let Some(entry) = SYNC_TABLE_CATALOG
        .iter()
        .find(|entry| entry.matches_table(table))
    {
        return Some(entry);
    }
    REGISTERED_TABLES.read().ok().and_then(|registry| {
        registry
            .iter()
            .copied()
            .find(|entry| entry.matches_table(table))
    })
}

/// Resolve a physical table name against the catalog.
pub struct SyncTableRef {
    pub descriptor: &'static SyncTableDescriptor,
    /// The concrete physical name (prefix-expanded for per-user tables).
    pub table: String,
}

pub fn resolve_sync_table(table: &str) -> Option<SyncTableRef> {
    sync_table_descriptor(table).map(|descriptor| SyncTableRef {
        descriptor,
        table: table.to_string(),
    })
}

/// Connection form submission for configure/test. The password is only
/// present when the user typed a new one.
#[derive(Clone, Debug, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SyncConnectionInput {
    pub host: String,
    pub port: i64,
    pub user: String,
    #[serde(default)]
    pub password: Option<String>,
    pub database: String,
    #[serde(default = "default_true")]
    pub tls_verify: bool,
    #[serde(default)]
    pub allow_plaintext: bool,
}

fn default_true() -> bool {
    true
}

/// Structured database connection fields for the settings UI. The backend
/// composes the connection string from these; the password never travels
/// back to the frontend (only whether one is stored).
#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SyncConnectionFields {
    pub host: String,
    pub port: i64,
    pub user: String,
    pub database: String,
    pub tls_verify: bool,
    /// Explicit opt-in for servers without TLS (trusted LAN only).
    pub allow_plaintext: bool,
    pub has_password: bool,
    /// Sync cadence in seconds (0 = default 60).
    #[serde(default)]
    pub interval_seconds: i64,
}

/// Per-table ops moved by the most recent completed cycle, so the status
/// card can show what a sync actually touched.
#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SyncTableOpCount {
    pub table: String,
    pub pushed: u64,
    pub pulled: u64,
}

/// Snapshot of sync health surfaced to the frontend.
#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatusSnapshot {
    pub enabled: bool,
    pub configured: bool,
    pub phase: String,
    pub device_id: String,
    pub last_push_at: Option<String>,
    pub last_pull_at: Option<String>,
    pub pending_outbox: i64,
    pub pull_cursor: i64,
    pub last_error: Option<String>,
    pub remote_schema_version: i64,
    pub remote_devices: Vec<SyncDeviceRecord>,
    /// Ops moved by the most recent completed cycle.
    #[serde(default)]
    pub last_pushed_ops: u64,
    #[serde(default)]
    pub last_pulled_ops: u64,
    #[serde(default)]
    pub last_cycle_at: Option<String>,
    /// Tables touched by the most recent completed cycle (sorted by name),
    /// empty when the cycle moved nothing.
    #[serde(default)]
    pub last_cycle_tables: Vec<SyncTableOpCount>,
}

#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SyncDeviceRecord {
    pub device_id: String,
    pub app_version: String,
    pub last_push_at: Option<String>,
    pub last_pull_at: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SyncConnectionTestResult {
    pub ok: bool,
    pub server_version: String,
    pub latency_ms: u64,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SyncBootstrapProgress {
    pub running: bool,
    pub phase: String,
    pub current_table: String,
    pub tables_done: u32,
    pub tables_total: u32,
    pub rows_done: u64,
    /// Total rows at bootstrap start (0 until measured).
    #[serde(default)]
    pub rows_total: i64,
    /// Rows completed within the current table.
    #[serde(default)]
    pub current_table_rows_done: u64,
    /// Total rows of the current table (0 until measured).
    #[serde(default)]
    pub current_table_rows_total: i64,
    /// Per-table snapshot: how far each table is.
    #[serde(default)]
    pub tables: Vec<SyncTableProgress>,
}

/// One table's upload progress inside a bootstrap.
#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SyncTableProgress {
    pub name: String,
    pub rows_done: u64,
    pub rows_total: i64,
    pub done: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hlc_encoded_order_matches_tuple_order() {
        let a = SyncHlc::new(1000, "aaaa");
        let b = SyncHlc::new(1000, "bbbb");
        let mut c = SyncHlc::new(1000, "aaaa");
        c.logical = 1;
        assert!(a.encode() < b.encode());
        assert!(a.encode() < c.encode());
        assert_eq!(SyncHlc::decode(&a.encode()), Some(a));
    }

    #[test]
    fn hlc_tick_survives_clock_rollback() {
        let mut hlc = SyncHlc::new(5000, "dev");
        hlc.tick(4000); // wall clock went backwards
        assert_eq!(hlc.physical_ms, 5000);
        assert_eq!(hlc.logical, 1);
        hlc.tick(6000);
        assert_eq!(hlc.physical_ms, 6000);
        assert_eq!(hlc.logical, 0);
    }

    #[test]
    fn hlc_observe_stays_ahead_of_remote_clock() {
        let mut local = SyncHlc::new(1000, "a");
        let remote = SyncHlc::new(9000, "b");
        local.observe(&remote, 1500);
        assert!(local.physical_ms >= 9000);
        let order = local.order_key();
        assert!(order > remote.order_key());
    }

    #[test]
    fn catalog_matches_global_and_per_user_tables() {
        assert!(sync_table_descriptor("gamelog_location").is_some());
        assert!(sync_table_descriptor("cache_avatar").is_none());
        let feed = sync_table_descriptor("usr123_feed_gps").unwrap();
        assert_eq!(
            feed.key_columns,
            &["created_at", "user_id", "previous_location"]
        );
        assert!(
            sync_table_descriptor("_feed_gps").is_none(),
            "per-user templates must not match bare"
        );
        assert!(sync_table_descriptor("config").is_none());
    }

    #[test]
    fn per_user_match_requires_a_prefix() {
        let descriptor = sync_table_descriptor("usrabc_notifications_v2").unwrap();
        assert_eq!(descriptor.template, "_notifications_v2");
        // The template itself (empty prefix) is not a physical table.
        assert!(!descriptor.matches_table("_notifications_v2"));
        // A global table that merely ends with a per-user template name must
        // not hijack the per-user descriptor.
        assert!(sync_table_descriptor("widget_notes").is_none());
        assert!(sync_table_descriptor("friend_notes").is_none());
    }
}
