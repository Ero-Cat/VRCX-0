use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use serde_json::json;

use super::*;
use vrcx_0_application_core::{
    FeedLiveEntry, FriendProjection, RealtimeInstanceQueueKind, RealtimeInstanceQueueProjection,
    RealtimeNotificationProjection, RealtimeNotificationUpsert,
};
use vrcx_0_i18n::{OverlayMessage, OverlayMessageKey};

#[derive(Clone, Default)]
struct TestOverlayActivitySink {
    snapshots: Arc<Mutex<Vec<OverlayActivitySnapshot>>>,
    deliveries: Arc<Mutex<Vec<OverlayActivityDelivery>>>,
}

impl OverlayActivitySink for TestOverlayActivitySink {
    fn emit_overlay_activity_snapshot(&self, snapshot: OverlayActivitySnapshot) {
        self.snapshots.lock().unwrap().push(snapshot);
    }

    fn emit_overlay_activity_delivery(&self, delivery: OverlayActivityDelivery) {
        self.deliveries.lock().unwrap().push(delivery);
    }
}

impl TestOverlayActivitySink {
    fn take(&self) -> Vec<OverlayActivitySnapshot> {
        std::mem::take(&mut *self.snapshots.lock().unwrap())
    }

    fn take_deliveries(&self) -> Vec<OverlayActivityDelivery> {
        std::mem::take(&mut *self.deliveries.lock().unwrap())
    }
}

#[test]
fn activity_text_serializes_as_the_typed_tagged_contract() {
    assert_eq!(
        serde_json::to_value(OverlayActivityText::message(
            OverlayMessage::notifications_gps("Test World")
        ))
        .expect("serialize message text"),
        json!({
            "kind": "message",
            "value": {
                "key": "notifications.gps",
                "params": { "location": "Test World" }
            }
        })
    );
    assert_eq!(
        serde_json::to_value(OverlayActivityText::default()).expect("serialize default text"),
        json!({ "kind": "literal", "value": "" })
    );
}

fn recent_candidate(activity_type: &str, user_id: &str) -> OverlayActivityCandidate {
    OverlayActivityCandidate {
        created_at: chrono::Utc::now().to_rfc3339(),
        ..candidate(activity_type, user_id)
    }
}

#[test]
fn trust_level_friend_projection_preserves_new_level_in_overlay_content() {
    let runtime = OverlayActivityRuntime::with_filters(OverlayActivityFilters::from_json(json!({
        "version": 1,
        "wrist": {
            "types": {
                "TrustLevel": {
                    "scope": "friends",
                    "favoriteGroupKeys": "all"
                }
            }
        }
    })));
    runtime.set_friend_user_ids(["usr_friend"]);
    runtime.ingest_friend_projection(
        &FriendProjection::new(0, 0),
        &[FeedLiveEntry::TrustLevel {
            created_at: "2026-05-31T00:01:00.000Z".into(),
            user_id: "usr_friend".into(),
            display_name: "Friend".into(),
            trust_level: "Trusted User".into(),
            previous_trust_level: "Known User".into(),
            friend_number: 7,
            owner_user_id: String::new(),
        }],
    );

    let entries = runtime.snapshot().entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].activity_type, "TrustLevel");
    assert_eq!(
        entries[0]
            .content
            .body
            .as_message()
            .expect("typed trust level message")
            .params()["trustLevel"],
        "Trusted User"
    );
    assert_eq!(
        entries[0].content.body.source_text(),
        "Trust level is now Trusted User"
    );
}

#[test]
fn player_joining_friend_feed_matches_everyone_in_instance_scope() {
    let runtime = OverlayActivityRuntime::with_filters(OverlayActivityFilters::from_json(json!({
        "version": 1,
        "wrist": {
            "types": {
                "OnPlayerJoining": {
                    "scope": "everyoneInInstance",
                    "favoriteGroupKeys": "all"
                }
            }
        }
    })));
    let projection = FriendProjection::new(0, 0);
    let feed_entries = vec![FeedLiveEntry::OnPlayerJoining {
        created_at: "2026-07-13T10:00:00Z".into(),
        user_id: "usr_joining".into(),
        display_name: "Joining User".into(),
        location: "traveling".into(),
        traveling_to_location: "wrld_current:456".into(),
        world_name: None,
        world_id: None,
        display_location: None,
        owner_user_id: String::new(),
    }];

    runtime.ingest_friend_projection(&projection, &feed_entries);

    let entries = runtime.snapshot().entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].activity_type, "OnPlayerJoining");
}

#[test]
fn friend_projection_feed_entries_do_not_restore_removed_friend_membership() {
    let runtime = OverlayActivityRuntime::with_filters(OverlayActivityFilters::from_json(json!({
        "version": 1,
        "wrist": {
            "types": {
                "Unfriend": {
                    "scope": "on",
                    "favoriteGroupKeys": "all"
                },
                "TrustLevel": {
                    "scope": "friends",
                    "favoriteGroupKeys": "all"
                }
            }
        }
    })));
    runtime.set_friend_user_ids(["usr_removed"]);
    let projection = FriendProjection {
        removals: vec!["usr_removed".to_string()],
        ..FriendProjection::new(0, 0)
    };
    let feed_entries = vec![
        FeedLiveEntry::Unfriend {
            created_at: "2026-05-31T00:01:30.000Z".into(),
            user_id: "usr_removed".into(),
            display_name: "Removed User".into(),
            owner_user_id: String::new(),
        },
        FeedLiveEntry::TrustLevel {
            created_at: "2026-05-31T00:01:31.000Z".into(),
            user_id: "usr_removed".into(),
            display_name: "Removed User".into(),
            trust_level: "Trusted User".into(),
            previous_trust_level: "Known User".into(),
            friend_number: 1,
            owner_user_id: String::new(),
        },
    ];

    runtime.ingest_friend_projection(&projection, &feed_entries);

    let entries = runtime.snapshot().entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].activity_type, "Unfriend");
}

#[test]
fn notification_projection_uses_sender_as_actor() {
    let runtime = OverlayActivityRuntime::with_filters(OverlayActivityFilters::from_json(json!({
        "version": 1,
        "wrist": {
            "types": {
                "invite": {
                    "scope": "allFavorites",
                    "favoriteGroupKeys": "all"
                }
            }
        }
    })));
    runtime.set_favorite_groups(OverlayFavoriteGroups::from_pairs([(
        "fav-a",
        ["usr_sender"].as_slice(),
    )]));
    let projection = RealtimeNotificationProjection {
        upserts: vec![RealtimeNotificationUpsert {
            notification: json!({
                "id": "notification-1",
                "type": "invite",
                "createdAt": "2026-05-31T00:02:00.000Z",
                "senderUserId": "usr_sender",
                "senderUsername": "Sender"
            })
            .into(),
            insert_defaults: None,
            notify_menu: true,
            deliver_runtime: true,
            run_automation: true,
        }],
        ..RealtimeNotificationProjection::default()
    };

    runtime.ingest_notification_projection(&projection);

    let entries = runtime.snapshot().entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].source_id, "notification:notification-1");
    assert_eq!(entries[0].actor_user_id, "usr_sender");
}

#[test]
fn notification_projection_does_not_use_receiver_as_actor() {
    let runtime = OverlayActivityRuntime::with_filters(OverlayActivityFilters::from_json(json!({
        "version": 1,
        "wrist": {
            "types": {
                "group.announcement": {
                    "scope": "on",
                    "favoriteGroupKeys": "all"
                }
            }
        }
    })));
    let projection = RealtimeNotificationProjection {
        upserts: vec![RealtimeNotificationUpsert {
            notification: json!({
                "id": "notification-group",
                "type": "group.announcement",
                "createdAt": "2026-05-31T00:02:00.000Z",
                "receiverUserId": "usr_self",
                "userId": "usr_self",
                "message": "Group announcement"
            })
            .into(),
            insert_defaults: None,
            notify_menu: true,
            deliver_runtime: true,
            run_automation: true,
        }],
        ..RealtimeNotificationProjection::default()
    };

    runtime.ingest_notification_projection(&projection);

    let entries = runtime.snapshot().entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].source_id, "notification:notification-group");
    assert!(entries[0].actor_user_id.is_empty());
}

#[test]
fn notification_projection_keeps_unresolved_direct_actor_with_user_id_title() {
    let (runtime, sink) = webhook_only_invite_runtime();
    let projection = RealtimeNotificationProjection {
        upserts: vec![RealtimeNotificationUpsert {
            notification: json!({
                "id": "notification-1",
                "type": "invite",
                "createdAt": chrono::Utc::now().to_rfc3339(),
                "senderUserId": "usr_sender"
            })
            .into(),
            insert_defaults: None,
            notify_menu: true,
            deliver_runtime: true,
            run_automation: true,
        }],
        ..RealtimeNotificationProjection::default()
    };

    let entries = runtime.ingest_notification_projection(&projection);

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].actor_user_id, "usr_sender");
    assert_eq!(
        entries[0].content.title,
        OverlayActivityText::literal("usr_sender")
    );
    let deliveries = sink.take_deliveries();
    assert_eq!(deliveries.len(), 1);
    assert!(deliveries[0].webhook);
    assert_eq!(
        deliveries[0].entry.content.title.source_text(),
        "usr_sender"
    );
}

#[test]
fn notification_projection_uses_nested_sender_display_name() {
    let (runtime, sink) = webhook_only_invite_runtime();
    let projection = RealtimeNotificationProjection {
        upserts: vec![RealtimeNotificationUpsert {
            notification: json!({
                "id": "notification-1",
                "type": "invite",
                "createdAt": chrono::Utc::now().to_rfc3339(),
                "senderUserId": "usr_sender",
                "details": {
                    "senderDisplayName": "Sender"
                }
            })
            .into(),
            insert_defaults: None,
            notify_menu: true,
            deliver_runtime: true,
            run_automation: true,
        }],
        ..RealtimeNotificationProjection::default()
    };

    let entries = runtime.ingest_notification_projection(&projection);

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].actor_display_name, "Sender");
    let deliveries = sink.take_deliveries();
    assert_eq!(deliveries.len(), 1);
    assert!(deliveries[0].webhook);
    assert_eq!(deliveries[0].entry.actor_display_name, "Sender");
}

#[test]
fn friend_projection_location_content_exposes_raw_and_display_location() {
    let runtime = OverlayActivityRuntime::with_filters(OverlayActivityFilters::from_json(json!({
        "version": 1,
        "wrist": {
            "types": {
                "GPS": {
                    "scope": "friends",
                    "favoriteGroupKeys": "all"
                }
            }
        }
    })));
    runtime.set_friend_user_ids(["usr_location"]);
    let projection = FriendProjection::new(0, 0);
    let feed_entries = vec![FeedLiveEntry::Gps {
        created_at: "2026-05-31T00:02:30.000Z".into(),
        user_id: "usr_location".into(),
        display_name: "Location User".into(),
        location: "wrld_world:12345".into(),
        world_name: "World Name".into(),
        previous_location: String::new(),
        time: 0,
        group_name: "Group Name".into(),
        world_id: None,
        display_location: None,
        owner_user_id: String::new(),
    }];

    runtime.ingest_friend_projection(&projection, &feed_entries);

    let entries = runtime.snapshot().entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].content.location, "wrld_world:12345");
    assert_eq!(entries[0].content.world_id, "wrld_world");
    assert_eq!(
        entries[0].content.display_location,
        "World Name public(Group Name)"
    );
}

#[test]
fn snapshot_marks_favorite_relation_before_friend_relation() {
    let runtime = OverlayActivityRuntime::with_filters(OverlayActivityFilters::from_json(json!({
        "version": 1,
        "wrist": {
            "types": {
                "friendRequest": {
                    "scope": "on",
                    "favoriteGroupKeys": "all"
                }
            }
        }
    })));
    runtime.set_friend_user_ids(["usr_favorite", "usr_friend"]);
    runtime.set_favorite_groups(OverlayFavoriteGroups::from_pairs([(
        "fav-a",
        ["usr_favorite"].as_slice(),
    )]));

    runtime.ingest_candidate(candidate("friendRequest", "usr_favorite"));
    runtime.ingest_candidate(candidate("friendRequest", "usr_friend"));
    runtime.ingest_candidate(candidate("friendRequest", "usr_other"));

    let entries = runtime.snapshot().entries;
    assert_eq!(entries.len(), 3);
    assert_eq!(
        entries[0].actor_relation,
        OverlayActivityActorRelation::Favorite
    );
    assert_eq!(
        entries[1].actor_relation,
        OverlayActivityActorRelation::Friend
    );
    assert_eq!(
        entries[2].actor_relation,
        OverlayActivityActorRelation::None
    );
}

#[test]
fn notification_projection_without_ids_uses_stable_fallback_source_ids() {
    let runtime = OverlayActivityRuntime::with_filters(OverlayActivityFilters::from_json(json!({
        "version": 1,
        "wrist": {
            "types": {
                "invite": {
                    "scope": "on",
                    "favoriteGroupKeys": "all"
                }
            }
        }
    })));
    let projection = RealtimeNotificationProjection {
        upserts: vec![
            RealtimeNotificationUpsert {
                notification: json!({
                    "type": "invite",
                    "createdAt": "2026-05-31T00:02:00.000Z",
                    "senderUserId": "usr_sender",
                    "senderUsername": "Sender",
                    "message": "first"
                })
                .into(),
                insert_defaults: None,
                notify_menu: true,
                deliver_runtime: true,
                run_automation: true,
            },
            RealtimeNotificationUpsert {
                notification: json!({
                    "type": "invite",
                    "createdAt": "2026-05-31T00:02:00.000Z",
                    "senderUserId": "usr_sender",
                    "senderUsername": "Sender",
                    "message": "second"
                })
                .into(),
                insert_defaults: None,
                notify_menu: true,
                deliver_runtime: true,
                run_automation: true,
            },
        ],
        ..RealtimeNotificationProjection::default()
    };

    runtime.ingest_notification_projection(&projection);

    let entries = runtime.snapshot().entries;
    assert_eq!(entries.len(), 2);
    assert!(entries[0]
        .source_id
        .starts_with("notification:invite:usr_sender:2026-05-31T00:02:00.000Z:"));
    assert!(entries[1]
        .source_id
        .starts_with("notification:invite:usr_sender:2026-05-31T00:02:00.000Z:"));
    assert_ne!(entries[0].source_id, entries[1].source_id);
}

#[test]
fn queue_projection_only_ingests_ready_events() {
    let runtime = OverlayActivityRuntime::new();
    runtime.ingest_instance_queue_projection(&RealtimeInstanceQueueProjection {
        kind: RealtimeInstanceQueueKind::Update,
        instance_location: "wrld_1:123".to_string(),
        world_id: "wrld_1".to_string(),
        world_name: "Queue World".to_string(),
        position: 2,
        queue_size: 4,
        received_at: "2026-05-31T00:03:00.000Z".to_string(),
        generation: 1,
    });
    runtime.ingest_instance_queue_projection(&RealtimeInstanceQueueProjection {
        kind: RealtimeInstanceQueueKind::Ready,
        instance_location: "wrld_1:123".to_string(),
        world_id: "wrld_1".to_string(),
        world_name: "Queue World".to_string(),
        position: 0,
        queue_size: 0,
        received_at: "2026-05-31T00:03:10.000Z".to_string(),
        generation: 1,
    });

    let entries = runtime.snapshot().entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].activity_type, "group.queueReady");
    assert_eq!(
        entries[0]
            .content
            .title
            .as_message()
            .expect("typed queue-ready title")
            .key(),
        OverlayMessageKey::NotificationsGroupQueueReadyTitle
    );
    assert_eq!(entries[0].content.summary, "Instance Queue Ready");
}

#[test]
fn runtime_emits_snapshot_when_activity_changes_and_clears() {
    let runtime = OverlayActivityRuntime::with_filters(OverlayActivityFilters::from_json(json!({
        "version": 1,
        "wrist": {
            "types": {
                "invite": {
                    "scope": "on",
                    "favoriteGroupKeys": "all"
                }
            }
        }
    })));
    let sink = TestOverlayActivitySink::default();
    runtime.set_sink(sink.clone());

    runtime.ingest_candidate(candidate("invite", "usr_sender"));
    runtime.clear_runtime_state();

    let snapshots = sink.take();
    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[0].entries.len(), 1);
    assert_eq!(snapshots[0].entries[0].activity_type, "invite");
    assert!(snapshots[1].entries.is_empty());
}

#[test]
fn runtime_emits_snapshot_when_filters_change() {
    let runtime = OverlayActivityRuntime::with_filters(OverlayActivityFilters::from_json(json!({
        "version": 1,
        "wrist": {
            "types": {
                "invite": {
                    "scope": "on",
                    "favoriteGroupKeys": "all"
                }
            }
        }
    })));
    let sink = TestOverlayActivitySink::default();
    runtime.set_sink(sink.clone());
    runtime.ingest_candidate(candidate("invite", "usr_sender"));
    sink.take();

    runtime.set_filters(OverlayActivityFilters::from_json(json!({
        "version": 1,
        "wrist": {
            "types": {
                "invite": {
                    "scope": "off",
                    "favoriteGroupKeys": "all"
                }
            }
        }
    })));

    let snapshots = sink.take();
    assert_eq!(snapshots.len(), 1);
    assert!(snapshots[0].entries.is_empty());
    assert!(runtime.snapshot().entries.is_empty());
}

#[test]
fn delivery_requires_live_session_event() {
    let runtime = OverlayActivityRuntime::new();
    let sink = TestOverlayActivitySink::default();
    runtime.set_sink(sink.clone());

    runtime.ingest_candidate(recent_candidate("friendRequest", "usr_a"));
    assert!(sink.take_deliveries().is_empty());
    assert_eq!(runtime.snapshot().entries.len(), 1);

    runtime.set_delivery_armed(true);
    runtime.ingest_candidate(recent_candidate("friendRequest", "usr_b"));
    let deliveries = sink.take_deliveries();
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].entry.actor_user_id, "usr_b");
    assert!(deliveries[0].desktop);
    assert!(deliveries[0].vr);

    runtime.ingest_candidate(candidate("friendRequest", "usr_c"));
    assert!(sink.take_deliveries().is_empty());
}

#[test]
fn delivery_fires_for_missed_event_after_live_session_started() {
    let runtime = OverlayActivityRuntime::new();
    let sink = TestOverlayActivitySink::default();
    runtime.set_sink(sink.clone());
    runtime.set_delivery_armed(true);

    let now = chrono::Utc::now();
    runtime.inner.state.lock().unwrap().live_since = Some(now - chrono::Duration::seconds(120));

    let mut missed = candidate("friendRequest", "usr_missed");
    missed.created_at = (now - chrono::Duration::seconds(90)).to_rfc3339();
    runtime.ingest_candidate(missed);
    let deliveries = sink.take_deliveries();
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].entry.actor_user_id, "usr_missed");
}

#[test]
fn default_webhook_surface_is_opt_in() {
    let filters = OverlayActivityFilters::default();

    assert_eq!(
        filters
            .rule_for(OverlayActivitySurface::Webhook, "friendRequest")
            .scope,
        OverlayActivityScope::Off
    );
    assert_eq!(
        filters
            .rule_for(OverlayActivitySurface::Webhook, "Online")
            .scope,
        OverlayActivityScope::Off
    );
}

#[test]
fn delivery_fires_for_desktop_only_without_wrist_entry() {
    let runtime = OverlayActivityRuntime::with_filters(OverlayActivityFilters::from_json(json!({
        "version": 1,
        "wrist": { "types": { "invite": { "scope": "off", "favoriteGroupKeys": "all" } } },
        "desktop": { "types": { "invite": { "scope": "on", "favoriteGroupKeys": "all" } } },
        "vr": { "types": { "invite": { "scope": "off", "favoriteGroupKeys": "all" } } }
    })));
    let sink = TestOverlayActivitySink::default();
    runtime.set_sink(sink.clone());
    runtime.set_delivery_armed(true);

    let entry = runtime.ingest_candidate(recent_candidate("invite", "usr_sender"));

    assert!(entry.is_some());
    assert!(runtime.snapshot().entries.is_empty());
    let deliveries = sink.take_deliveries();
    assert_eq!(deliveries.len(), 1);
    assert!(deliveries[0].desktop);
    assert!(!deliveries[0].vr);
}

#[test]
fn current_instance_gps_is_hidden_from_vr_and_hmd_but_kept_on_wrist() {
    let runtime = OverlayActivityRuntime::with_filters(current_instance_gps_filters(
        "friends",
        "friends",
        "selectedFavorites",
    ));
    let sink = TestOverlayActivitySink::default();
    runtime.set_sink(sink.clone());
    configure_current_instance_friend(&runtime);
    runtime.set_delivery_armed(true);

    runtime.ingest_candidate(current_instance_join_candidate());
    let joined = sink.take_deliveries();
    assert_eq!(joined.len(), 1);
    assert!(joined[0].vr);
    assert!(joined[0].hmd);

    runtime.ingest_candidate(current_instance_gps_candidate("wrld_current:123"));

    let entries = runtime.snapshot().entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].activity_type, "GPS");
    let gps = sink.take_deliveries();
    assert_eq!(gps.len(), 1);
    assert!(gps[0].desktop);
    assert!(!gps[0].vr);
    assert!(!gps[0].hmd);
    assert!(gps[0].webhook);
    assert!(gps[0].tts);
}

#[test]
fn current_instance_gps_is_hidden_only_on_surfaces_that_delivered_joined() {
    let runtime =
        OverlayActivityRuntime::with_filters(current_instance_gps_filters("friends", "off", "off"));
    let sink = TestOverlayActivitySink::default();
    runtime.set_sink(sink.clone());
    configure_current_instance_friend(&runtime);
    runtime.set_delivery_armed(true);

    runtime.ingest_candidate(current_instance_join_candidate());
    let joined = sink.take_deliveries();
    assert_eq!(joined.len(), 1);
    assert!(joined[0].vr);
    assert!(!joined[0].hmd);

    runtime.ingest_candidate(current_instance_gps_candidate("wrld_current:123"));

    let gps = sink.take_deliveries();
    assert_eq!(gps.len(), 1);
    assert!(!gps[0].vr);
    assert!(gps[0].hmd);
}

#[test]
fn current_instance_gps_is_kept_when_joined_was_not_live() {
    let runtime = OverlayActivityRuntime::with_filters(current_instance_gps_filters(
        "friends", "friends", "off",
    ));
    let sink = TestOverlayActivitySink::default();
    runtime.set_sink(sink.clone());
    configure_current_instance_friend(&runtime);

    runtime.ingest_candidate(current_instance_join_candidate());
    assert!(sink.take_deliveries().is_empty());
    runtime.set_delivery_armed(true);
    runtime.ingest_candidate(current_instance_gps_candidate("wrld_current:123"));

    let gps = sink.take_deliveries();
    assert_eq!(gps.len(), 1);
    assert!(gps[0].vr);
    assert!(gps[0].hmd);
}

#[test]
fn current_instance_gps_coverage_is_cleared_when_player_leaves() {
    let runtime = OverlayActivityRuntime::with_filters(current_instance_gps_filters(
        "friends", "friends", "off",
    ));
    let sink = TestOverlayActivitySink::default();
    runtime.set_sink(sink.clone());
    configure_current_instance_friend(&runtime);
    runtime.set_delivery_armed(true);

    runtime.ingest_candidate(current_instance_join_candidate());
    assert_eq!(sink.take_deliveries().len(), 1);
    runtime.set_current_instance_presence("wrld_current:123", std::iter::empty::<&str>());
    runtime.set_current_instance_presence("wrld_current:123", ["usr_selected"]);
    runtime.ingest_candidate(current_instance_gps_candidate("wrld_current:123"));

    let gps = sink.take_deliveries();
    assert_eq!(gps.len(), 1);
    assert!(gps[0].vr);
    assert!(gps[0].hmd);
}

#[test]
fn gps_for_another_instance_clears_current_instance_joined_coverage() {
    let runtime = OverlayActivityRuntime::with_filters(current_instance_gps_filters(
        "friends", "friends", "off",
    ));
    let sink = TestOverlayActivitySink::default();
    runtime.set_sink(sink.clone());
    configure_current_instance_friend(&runtime);
    runtime.set_delivery_armed(true);

    runtime.ingest_candidate(current_instance_join_candidate());
    assert_eq!(sink.take_deliveries().len(), 1);
    let mut away = current_instance_gps_candidate("wrld_other:456");
    away.source_id = "gps-away".into();
    runtime.ingest_candidate(away);

    let gps = sink.take_deliveries();
    assert_eq!(gps.len(), 1);
    assert!(gps[0].vr);
    assert!(gps[0].hmd);

    let mut returning = current_instance_gps_candidate("wrld_current:123");
    returning.source_id = "gps-returning".into();
    runtime.ingest_candidate(returning);

    let gps = sink.take_deliveries();
    assert_eq!(gps.len(), 1);
    assert!(gps[0].vr);
    assert!(gps[0].hmd);
}

#[test]
fn delivery_fires_for_webhook_only_without_wrist_entry() {
    let runtime = OverlayActivityRuntime::with_filters(OverlayActivityFilters::from_json(json!({
        "version": 1,
        "wrist": { "types": { "invite": { "scope": "off", "favoriteGroupKeys": "all" } } },
        "desktop": { "types": { "invite": { "scope": "off", "favoriteGroupKeys": "all" } } },
        "vr": { "types": { "invite": { "scope": "off", "favoriteGroupKeys": "all" } } },
        "webhook": { "types": { "invite": { "scope": "on", "favoriteGroupKeys": "all" } } }
    })));
    let sink = TestOverlayActivitySink::default();
    runtime.set_sink(sink.clone());
    runtime.set_delivery_armed(true);

    let entry = runtime.ingest_candidate(recent_candidate("invite", "usr_sender"));

    assert!(entry.is_some());
    assert!(runtime.snapshot().entries.is_empty());
    let deliveries = sink.take_deliveries();
    assert_eq!(deliveries.len(), 1);
    assert!(!deliveries[0].desktop);
    assert!(!deliveries[0].vr);
    assert!(deliveries[0].webhook);
}

#[test]
fn dedup_blocks_redelivery_across_surfaces() {
    let runtime = OverlayActivityRuntime::new();
    let sink = TestOverlayActivitySink::default();
    runtime.set_sink(sink.clone());
    runtime.set_delivery_armed(true);

    let first = recent_candidate("friendRequest", "usr_a");
    let duplicate = first.clone();
    assert!(runtime.ingest_candidate(first).is_some());
    assert!(runtime.ingest_candidate(duplicate).is_none());
    assert_eq!(sink.take_deliveries().len(), 1);
}

#[test]
fn location_hidden_users_skip_gps_on_every_surface_but_keep_other_activity() {
    let rule = json!({ "scope": "friends", "favoriteGroupKeys": "all" });
    let surface = json!({ "types": { "GPS": rule, "Status": rule } });
    let runtime = OverlayActivityRuntime::with_filters(OverlayActivityFilters::from_json(json!({
        "version": 1,
        "wrist": surface,
        "desktop": surface,
        "vr": surface,
        "hmd": surface,
        "webhook": surface,
        "tts": surface
    })));
    let sink = TestOverlayActivitySink::default();
    runtime.set_sink(sink.clone());
    runtime.set_delivery_armed(true);
    runtime.set_friend_user_ids(["usr_hidden", "usr_other"]);
    runtime.set_location_hidden_user_ids(HashSet::from(["usr_hidden".to_string()]));

    assert!(runtime
        .ingest_candidate(recent_candidate("GPS", "usr_hidden"))
        .is_none());
    assert!(sink.take_deliveries().is_empty());
    assert!(runtime.snapshot().entries.is_empty());

    assert!(runtime
        .ingest_candidate(recent_candidate("Status", "usr_hidden"))
        .is_some());
    assert!(runtime
        .ingest_candidate(recent_candidate("GPS", "usr_other"))
        .is_some());
    let delivered = sink
        .take_deliveries()
        .into_iter()
        .map(|delivery| (delivery.entry.activity_type, delivery.entry.actor_user_id))
        .collect::<Vec<_>>();
    assert_eq!(
        delivered,
        vec![
            ("Status".to_string(), "usr_hidden".to_string()),
            ("GPS".to_string(), "usr_other".to_string()),
        ]
    );
}

fn candidate(activity_type: &str, user_id: &str) -> OverlayActivityCandidate {
    OverlayActivityCandidate {
        source_id: format!("{activity_type}:{user_id}"),
        activity_type: activity_type.to_string(),
        created_at: "2026-05-31T00:00:00.000Z".to_string(),
        actor_user_id: user_id.to_string(),
        actor_display_name: user_id.to_string(),
        current_instance: false,
        favorite_subject: OverlayActivityFavoriteSubject::UserId(user_id.to_string()),
        payload: json!({}).into(),
    }
}

fn current_instance_gps_filters(
    vr_joined_scope: &str,
    hmd_joined_scope: &str,
    wrist_gps_scope: &str,
) -> OverlayActivityFilters {
    OverlayActivityFilters::from_json(json!({
        "version": 1,
        "wrist": { "types": {
            "OnPlayerJoined": { "scope": "off", "favoriteGroupKeys": "all" },
            "GPS": { "scope": wrist_gps_scope, "favoriteGroupKeys": ["fav-selected"] }
        } },
        "desktop": { "types": {
            "OnPlayerJoined": { "scope": "off", "favoriteGroupKeys": "all" },
            "GPS": { "scope": "friends", "favoriteGroupKeys": "all" }
        } },
        "vr": { "types": {
            "OnPlayerJoined": { "scope": vr_joined_scope, "favoriteGroupKeys": "all" },
            "GPS": { "scope": "selectedFavorites", "favoriteGroupKeys": ["fav-selected"] }
        } },
        "hmd": { "types": {
            "OnPlayerJoined": { "scope": hmd_joined_scope, "favoriteGroupKeys": "all" },
            "GPS": { "scope": "selectedFavorites", "favoriteGroupKeys": ["fav-selected"] }
        } },
        "webhook": { "types": {
            "OnPlayerJoined": { "scope": "off", "favoriteGroupKeys": "all" },
            "GPS": { "scope": "friends", "favoriteGroupKeys": "all" }
        } },
        "tts": { "types": {
            "OnPlayerJoined": { "scope": "off", "favoriteGroupKeys": "all" },
            "GPS": { "scope": "friends", "favoriteGroupKeys": "all" }
        } }
    }))
}

fn configure_current_instance_friend(runtime: &OverlayActivityRuntime) {
    runtime.set_friend_user_ids(["usr_selected"]);
    runtime.set_favorite_groups(OverlayFavoriteGroups::from_pairs([(
        "fav-selected",
        ["usr_selected"].as_slice(),
    )]));
    runtime.set_current_instance_presence("wrld_current:123", ["usr_selected"]);
}

fn current_instance_join_candidate() -> OverlayActivityCandidate {
    let mut row = recent_candidate("OnPlayerJoined", "usr_selected");
    row.current_instance = true;
    row.payload = json!({
        "location": "wrld_current:123",
        "worldId": "wrld_current"
    })
    .into();
    row
}

fn current_instance_gps_candidate(location: &str) -> OverlayActivityCandidate {
    let mut row = recent_candidate("GPS", "usr_selected");
    row.payload = json!({
        "type": "GPS",
        "userId": "usr_selected",
        "location": location
    })
    .into();
    row
}

fn webhook_only_invite_runtime() -> (OverlayActivityRuntime, TestOverlayActivitySink) {
    let runtime = OverlayActivityRuntime::with_filters(OverlayActivityFilters::from_json(json!({
        "version": 1,
        "wrist": { "types": { "invite": { "scope": "off", "favoriteGroupKeys": "all" } } },
        "desktop": { "types": { "invite": { "scope": "off", "favoriteGroupKeys": "all" } } },
        "vr": { "types": { "invite": { "scope": "off", "favoriteGroupKeys": "all" } } },
        "webhook": { "types": { "invite": { "scope": "on", "favoriteGroupKeys": "all" } } }
    })));
    let sink = TestOverlayActivitySink::default();
    runtime.set_sink(sink.clone());
    runtime.set_delivery_armed(true);
    (runtime, sink)
}
