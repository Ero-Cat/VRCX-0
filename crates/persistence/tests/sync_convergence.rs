//! Multi-replica convergence tests for the sync capture/apply lattice.
//!
//! Three independent SQLite databases play the roles of three devices
//! writing concurrently (including the millisecond-close same-key writes that
//! several always-on clients produce). Ops flow between them in shuffled
//! arrival orders — the property under test is that every replica ends with
//! the identical table contents, regardless of delivery order.

use serde_json::Value;
use vrcx_0_contracts::SyncOpRecord;

fn p(entries: &[(&str, Value)]) -> std::collections::HashMap<String, Value> {
    entries
        .iter()
        .map(|(key, value)| (format!("@{key}"), value.clone()))
        .collect()
}
use vrcx_0_persistence::sync::{
    apply_pulled_ops, install_capture, outbox_pending_count, outbox_take, outbox_trim_pushed,
};
use vrcx_0_persistence::DatabaseService;

struct Replica {
    name: &'static str,
    db: DatabaseService,
    handle: vrcx_0_persistence::sync::SyncCaptureHandle,
}

impl Replica {
    fn new(name: &'static str) -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "vrcx-0-sync-conv-{name}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = DatabaseService::new(&dir.join("VRCX-0.sqlite3")).unwrap();
        vrcx_0_persistence::game_log::ensure_game_log_tables(&db).unwrap();
        vrcx_0_persistence::maintenance::ensure_required_database_schema(&db).unwrap();
        db.test_execute_non_query(
            "CREATE TABLE IF NOT EXISTS browse_history (
                owner_user_id TEXT NOT NULL,
                entity_kind TEXT NOT NULL,
                entity_id TEXT NOT NULL,
                first_viewed_at TEXT,
                last_viewed_at TEXT,
                view_count INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (owner_user_id, entity_kind, entity_id)
            )",
            &Default::default(),
        )
        .unwrap();
        let handle = install_capture(&db).unwrap();
        Self { name, db, handle }
    }

    fn device(&self) -> String {
        self.handle.device_id().to_string()
    }
}

fn three_replicas() -> Vec<Replica> {
    vec![
        Replica::new("alpha"),
        Replica::new("beta"),
        Replica::new("gamma"),
    ]
}

/// Deliver every replica's pending ops to all others, rotating the delivery
/// order each round, until nothing is pending anywhere.
fn gossip_until_quiet(replicas: &mut Vec<Replica>, rounds: usize) {
    let orders: Vec<Vec<usize>> = vec![vec![0, 1, 2], vec![2, 0, 1], vec![1, 2, 0]];
    for round in 0..rounds {
        let order = orders[round % orders.len()].clone();
        let mut batches: Vec<(usize, Vec<SyncOpRecord>, i64)> = Vec::new();
        for index in &order {
            if let Some(batch) =
                outbox_take(&replicas[*index].db, &replicas[*index].handle, 100).unwrap()
            {
                batches.push((*index, batch.ops, batch.max_seq));
            }
        }
        if batches.is_empty() {
            return;
        }
        let mut cursor = round as i64 * 10_000;
        for (source, ops, max_seq) in batches {
            for (index, replica) in replicas.iter().enumerate() {
                if index == source {
                    continue;
                }
                cursor += 1;
                let stats = apply_pulled_ops(&replica.db, &replica.device(), &ops, cursor).unwrap();
                if std::env::var("SYNC_DEBUG").is_ok() {
                    eprintln!(
                        "round {round}: {} -> {} applied={} pending={} stale={} missing={} ops={:?}",
                        replicas[source].name,
                        replica.name,
                        stats.applied,
                        stats.skipped_pending,
                        stats.skipped_stale,
                        stats.skipped_missing,
                        ops.iter().map(|o| (o.kind.clone(), o.hlc.clone())).collect::<Vec<_>>()
                    );
                }
            }
            outbox_trim_pushed(&replicas[source].db, max_seq).unwrap();
        }
    }
    panic!("gossip did not settle within {rounds} rounds");
}

fn dump(db: &DatabaseService, table: &str, columns: &str, order: &str) -> String {
    let rows = db
        .test_execute(
            &format!("SELECT {columns} FROM {table} ORDER BY {order}"),
            &Default::default(),
        )
        .unwrap();
    rows.iter()
        .map(|row| serde_json::to_string(row).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_replicas_converged(
    replicas: &[Replica],
    table: &str,
    columns: &str,
    order: &str,
) -> String {
    let first = dump(&replicas[0].db, table, columns, order);
    for replica in &replicas[1..] {
        assert_eq!(
            first,
            dump(&replica.db, table, columns, order),
            "replica {} diverged on {table}",
            replica.name
        );
    }
    first
}

#[test]
fn identical_facts_recorded_on_all_devices_dedupe_into_one_row() {
    let mut replicas = three_replicas();
    for replica in &replicas {
        replica
            .db
            .test_execute_non_query(
                "INSERT INTO gamelog_join_leave (created_at, type, display_name, location, user_id, time, owner_id)
                 VALUES ('2026-05-01T10:00:00Z', 'OnPlayerJoined', 'Alice', 'loc1', 'usr_alice', 0, 1)",
                &Default::default(),
            )
            .unwrap();
    }
    gossip_until_quiet(&mut replicas, 6);
    let dump = assert_replicas_converged(
        &replicas,
        "gamelog_join_leave",
        "created_at, type, display_name, user_id, time",
        "created_at, type, display_name",
    );
    assert_eq!(
        dump.lines().count(),
        1,
        "three identical facts collapse into one row"
    );
}

#[test]
fn concurrent_memo_edits_converge_to_one_deterministic_winner() {
    let mut replicas = three_replicas();
    let texts = ["alpha edit", "beta edit", "gamma edit"];
    for (replica, text) in replicas.iter_mut().zip(texts) {
        replica
            .db
            .test_execute_non_query(
                "INSERT INTO memos (user_id, edited_at, memo) VALUES ('usr_x', '2026-05-02', @memo)",
                &p(&[("memo", Value::String(text.into()))]),
            )
            .unwrap();
    }
    gossip_until_quiet(&mut replicas, 6);
    let dump = assert_replicas_converged(&replicas, "memos", "user_id, memo", "user_id");
    // Exactly one winner text; all replicas agree on the same string.
    let winner = dump.lines().next().unwrap_or_default();
    let winner = winner.split('"').nth(3).unwrap_or_default();
    assert!(
        texts.contains(&winner),
        "winner {winner:?} must be one of the concurrent edits"
    );
}

#[test]
fn counters_seeded_and_incremented_independently_sum_exactly() {
    let mut replicas = three_replicas();

    // Alpha and beta both discover the same world independently and browse it
    // a different number of times before ever syncing.
    for (replica, views) in [("alpha", 2u32), ("beta", 5u32)] {
        let target = replicas.iter_mut().find(|r| r.name == replica).unwrap();
        target
            .db
            .test_execute_non_query(
                "INSERT INTO browse_history (owner_user_id, entity_kind, entity_id, first_viewed_at, last_viewed_at, view_count)
                 VALUES ('usr_o', 'world', 'wr1', '2026-05-03', '2026-05-03', 1)",
                &Default::default(),
            )
            .unwrap();
        for _ in 1..views {
            target
                .db
                .test_execute_non_query(
                    "UPDATE browse_history SET view_count = view_count + 1 WHERE entity_id = 'wr1'",
                    &Default::default(),
                )
                .unwrap();
        }
    }
    // One gossip round in between, then more increments land after sync —
    // the steady-state path where deltas must be exact.
    gossip_until_quiet(&mut replicas, 2);
    for (replica, views) in [("alpha", 3u32), ("gamma", 4u32)] {
        let target = replicas.iter_mut().find(|r| r.name == replica).unwrap();
        target
            .db
            .test_execute_non_query(
                "INSERT OR IGNORE INTO browse_history (owner_user_id, entity_kind, entity_id, first_viewed_at, last_viewed_at, view_count)
                 VALUES ('usr_o', 'world', 'wr1', '2026-05-03', '2026-05-03', 0)",
                &Default::default(),
            )
            .unwrap();
        for _ in 0..views {
            target
                .db
                .test_execute_non_query(
                    "UPDATE browse_history SET view_count = view_count + 1 WHERE entity_id = 'wr1'",
                    &Default::default(),
                )
                .unwrap();
        }
    }
    gossip_until_quiet(&mut replicas, 6);

    let dump = assert_replicas_converged(
        &replicas,
        "browse_history",
        "entity_id, view_count",
        "entity_id",
    );
    // 2 (alpha pre-sync) + 5 (beta pre-sync) + 3 (alpha post-sync) + 4 (gamma)
    assert!(
        dump.contains("\"view_count\":14") || dump.ends_with(", 14") || dump.contains(",14"),
        "expected the exact sum 14 in {dump}"
    );
}

#[test]
fn deletes_propagate_and_later_reinsertion_revives() {
    let mut replicas = three_replicas();
    replicas[0]
        .db
        .test_execute_non_query(
            "INSERT INTO memos (user_id, edited_at, memo) VALUES ('usr_del', '2026-05-04', 'doomed')",
            &Default::default(),
        )
        .unwrap();
    gossip_until_quiet(&mut replicas, 4);
    // Everyone has it; alpha deletes.
    replicas[0]
        .db
        .test_execute_non_query(
            "DELETE FROM memos WHERE user_id = 'usr_del'",
            &Default::default(),
        )
        .unwrap();
    gossip_until_quiet(&mut replicas, 4);
    for replica in &replicas {
        let rows = replica
            .db
            .test_execute(
                "SELECT COUNT(*) FROM memos WHERE user_id = 'usr_del'",
                &Default::default(),
            )
            .unwrap();
        assert_eq!(
            rows[0][0].as_i64(),
            Some(0),
            "{} applied the delete",
            replica.name
        );
    }
    // Gamma re-adds later (newer fact) — revival wins.
    replicas[2]
        .db
        .test_execute_non_query(
            "INSERT INTO memos (user_id, edited_at, memo) VALUES ('usr_del', '2026-05-05', 'revived')",
            &Default::default(),
        )
        .unwrap();
    gossip_until_quiet(&mut replicas, 4);
    let dump = assert_replicas_converged(&replicas, "memos", "user_id, memo", "user_id");
    assert!(
        dump.contains("revived"),
        "reinsertion revives the row: {dump}"
    );
}

#[test]
fn echo_suppression_keeps_gossip_terminating() {
    let mut replicas = three_replicas();
    for replica in &replicas {
        replica
            .db
            .test_execute_non_query(
                "INSERT INTO gamelog_external (created_at, message, display_name, user_id, location, owner_id)
                 VALUES ('2026-05-06T00:00:00Z', @msg, 'system', '', '', 0)",
                &p(&[("msg", Value::String(format!("note from {}", replica.name)))]),
            )
            .unwrap();
    }
    gossip_until_quiet(&mut replicas, 6);
    // After convergence nothing may remain pending — pulls must not re-enqueue.
    for replica in &replicas {
        assert_eq!(
            outbox_pending_count(&replica.db).unwrap(),
            0,
            "{} still has pending ops after gossip",
            replica.name
        );
    }
    let dump = assert_replicas_converged(
        &replicas,
        "gamelog_external",
        "created_at, message",
        "created_at, message",
    );
    assert_eq!(
        dump.lines().count(),
        3,
        "each device's note is a distinct fact"
    );
}
