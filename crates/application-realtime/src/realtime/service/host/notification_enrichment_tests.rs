use super::test_support::*;
use super::*;
use vrcx_0_core::OwnerId;

#[test]
fn persisted_notification_output_reaches_its_dedicated_observer_without_overlay_work() -> Result<()>
{
    let (_dir, runtime, active_session) =
        runtime_with_active_session("notification-dedicated-observer")?;
    runtime
        .runtime()
        .apply_notification_output(RealtimeNotificationOutput {
            owner_user_id: OwnerId::new(active_session.user_id),
            projection: RealtimeNotificationProjection {
                generation: 7,
                ..RealtimeNotificationProjection::default()
            },
            ..RealtimeNotificationOutput::default()
        });

    let projections = runtime.notification_projection_observer_for_test().take();
    assert_eq!(projections.len(), 1);
    assert_eq!(projections[0].generation, 7);
    assert!(runtime
        .activity_sink_for_test()
        .notification_by_id("missing")
        .is_none());
    Ok(())
}

#[test]
fn notification_cache_hits_enrich_projection_and_persistence() -> Result<()> {
    let (_dir, runtime, active_session) = runtime_with_active_session("notification-cache-hit")?;
    runtime.cache_world_for_test("wrld_cached", "Cached World", "2026-01-01T00:00:00.000Z");
    runtime
        .runtime()
        .world_cache
        .get_summary("wrld_cached")?
        .expect("cached world should load on demand");
    runtime.runtime().ingest_user_facts(vec![json!({
        "user": {
            "id": "usr_sender",
            "displayName": "Cached Sender"
        },
        "source": "test",
        "isFriend": true
    })]);
    runtime.runtime().deps.event_bus.take_events_for_test();
    let notification = json!({
        "id": "notif-cache-hit",
        "createdAt": "2026-06-21T00:00:00.000Z",
        "type": "invite",
        "senderUserId": "usr_sender",
        "senderUsername": "usr_sender",
        "message": "Join me",
        "details": {
            "worldId": "wrld_cached",
            "worldName": "wrld_cached"
        }
    });

    runtime
        .runtime()
        .apply_notification_output(RealtimeNotificationOutput {
            owner_user_id: OwnerId::new(active_session.user_id.clone()),
            projection: RealtimeNotificationProjection {
                generation: 7,
                upserts: vec![RealtimeNotificationUpsert {
                    notification: notification.clone().into(),
                    insert_defaults: None,
                    notify_menu: true,
                    deliver_runtime: true,
                    run_automation: false,
                }],
                ..RealtimeNotificationProjection::default()
            },
            persistence: RealtimePersistenceBatch {
                notification_v2_upserts: vec![notification],
                ..RealtimePersistenceBatch::default()
            },
        });

    let events = runtime.runtime().deps.event_bus.take_events_for_test();
    let projection = events
        .iter()
        .find(|event| event.name == "realtimeNotificationProjection")
        .expect("cache-hit notification should emit a realtime projection");
    let projected = &projection.payload["upserts"][0]["notification"];
    assert_eq!(projected["senderDisplayName"], "Cached Sender");
    assert_eq!(projected["senderUsername"], "Cached Sender");
    assert_eq!(projected["details"]["worldName"], "Cached World");

    let rows = notification_list_query(
        runtime.database(),
        NotificationListQueryInput {
            user_id: active_session.user_id,
            search: String::new(),
            filters: Vec::new(),
            per_table_limit: 10,
            limit: 10,
            include_unseen: false,
        },
    )?;
    let row = rows
        .iter()
        .find(|row| row.id == "notif-cache-hit")
        .expect("notification should be persisted");
    assert_eq!(row.sender_username, "Cached Sender");
    assert_eq!(row.details["worldName"], "Cached World");
    Ok(())
}

#[test]
fn notification_cache_hit_enriches_avatar_image_for_runtime_delivery() -> Result<()> {
    let (_dir, runtime, active_session) =
        runtime_with_active_session("notification-avatar-cache-hit")?;
    runtime.runtime().ingest_user_facts(vec![json!({
        "user": {
            "id": "usr_sender",
            "displayName": "Cached Sender",
            "iconUrl": "https://images.example/user-icon.png"
        },
        "source": "test",
        "isFriend": true
    })]);
    runtime.runtime().deps.event_bus.take_events_for_test();
    let notification = json!({
        "id": "notif-avatar-cache-hit",
        "createdAt": "2026-06-21T00:00:00.000Z",
        "type": "friendRequest",
        "senderUserId": "usr_sender",
        "senderUsername": "usr_sender",
        "message": "Friend request"
    });

    runtime
        .runtime()
        .apply_notification_output(RealtimeNotificationOutput {
            owner_user_id: OwnerId::new(active_session.user_id.clone()),
            projection: RealtimeNotificationProjection {
                generation: 7,
                upserts: vec![RealtimeNotificationUpsert {
                    notification: notification.clone().into(),
                    insert_defaults: None,
                    notify_menu: true,
                    deliver_runtime: true,
                    run_automation: false,
                }],
                ..RealtimeNotificationProjection::default()
            },
            persistence: RealtimePersistenceBatch {
                notification_v2_upserts: vec![notification],
                ..RealtimePersistenceBatch::default()
            },
        });

    let events = runtime.runtime().deps.event_bus.take_events_for_test();
    let projection = events
        .iter()
        .find(|event| event.name == "realtimeNotificationProjection")
        .expect("cache-hit notification should emit a realtime projection");
    let projected = &projection.payload["upserts"][0]["notification"];
    assert_eq!(
        projected["imageUrl"],
        "https://images.example/user-icon.png"
    );

    let delivered = runtime
        .activity_sink_for_test()
        .notification_by_id("notif-avatar-cache-hit")
        .expect("runtime delivery should reach the activity sink");
    assert_eq!(
        delivered.facts.image_url,
        "https://images.example/user-icon.png"
    );
    Ok(())
}

#[test]
fn notification_avatar_resolves_from_user_id_when_sender_field_absent() -> Result<()> {
    let (_dir, runtime, active_session) =
        runtime_with_active_session("notification-avatar-user-id")?;
    runtime.runtime().ingest_user_facts(vec![json!({
        "user": {
            "id": "usr_sender",
            "displayName": "Cached Sender",
            "iconUrl": "https://images.example/user-icon.png"
        },
        "source": "test",
        "isFriend": true
    })]);
    runtime.runtime().deps.event_bus.take_events_for_test();
    let notification = json!({
        "id": "notif-avatar-user-id",
        "createdAt": "2026-06-21T00:00:00.000Z",
        "type": "friendRequest",
        "userId": "usr_sender",
        "senderUsername": "usr_sender",
        "message": "Friend request"
    });

    runtime
        .runtime()
        .apply_notification_output(RealtimeNotificationOutput {
            owner_user_id: OwnerId::new(active_session.user_id.clone()),
            projection: RealtimeNotificationProjection {
                generation: 7,
                upserts: vec![RealtimeNotificationUpsert {
                    notification: notification.clone().into(),
                    insert_defaults: None,
                    notify_menu: true,
                    deliver_runtime: true,
                    run_automation: false,
                }],
                ..RealtimeNotificationProjection::default()
            },
            persistence: RealtimePersistenceBatch {
                notification_v2_upserts: vec![notification],
                ..RealtimePersistenceBatch::default()
            },
        });

    let events = runtime.runtime().deps.event_bus.take_events_for_test();
    let projection = events
        .iter()
        .find(|event| event.name == "realtimeNotificationProjection")
        .expect("user-id sender notification should emit a realtime projection");
    let projected = &projection.payload["upserts"][0]["notification"];
    assert_eq!(
        projected["imageUrl"],
        "https://images.example/user-icon.png"
    );
    Ok(())
}

#[test]
fn notification_avatar_fallback_skips_owner_receiver_when_sender_is_absent() -> Result<()> {
    let (_dir, runtime, active_session) =
        runtime_with_active_session("notification-avatar-receiver")?;
    runtime.runtime().ingest_user_facts(vec![json!({
        "user": {
            "id": "usr_self",
            "displayName": "Self",
            "iconUrl": "https://images.example/self-icon.png"
        },
        "source": "test",
        "isFriend": false
    })]);
    runtime.runtime().deps.event_bus.take_events_for_test();
    let notification = json!({
        "id": "notif-avatar-receiver",
        "createdAt": "2026-06-21T00:00:00.000Z",
        "type": "group.announcement",
        "receiverUserId": "usr_self",
        "userId": "usr_self",
        "message": "Group announcement"
    });

    runtime
        .runtime()
        .apply_notification_output(RealtimeNotificationOutput {
            owner_user_id: OwnerId::new(active_session.user_id.clone()),
            projection: RealtimeNotificationProjection {
                generation: 7,
                upserts: vec![RealtimeNotificationUpsert {
                    notification: notification.clone().into(),
                    insert_defaults: None,
                    notify_menu: true,
                    deliver_runtime: true,
                    run_automation: false,
                }],
                ..RealtimeNotificationProjection::default()
            },
            persistence: RealtimePersistenceBatch {
                notification_v2_upserts: vec![notification],
                ..RealtimePersistenceBatch::default()
            },
        });

    let events = runtime.runtime().deps.event_bus.take_events_for_test();
    let projection = events
        .iter()
        .find(|event| event.name == "realtimeNotificationProjection")
        .expect("receiver-only notification should emit a realtime projection");
    let projected = &projection.payload["upserts"][0]["notification"];
    assert!(projected["imageUrl"].is_null());

    let delivered = runtime
        .activity_sink_for_test()
        .notification_by_id("notif-avatar-receiver")
        .expect("runtime delivery should reach the activity sink");
    assert!(delivered.actor.user_id.is_empty());
    assert!(delivered.facts.image_url.is_empty());
    Ok(())
}

#[test]
fn notification_avatar_fallback_skips_current_user_sender() -> Result<()> {
    let (_dir, runtime, active_session) =
        runtime_with_active_session("notification-avatar-self-sender")?;
    runtime.runtime().ingest_user_facts(vec![json!({
        "user": {
            "id": "usr_self",
            "displayName": "Self",
            "iconUrl": "https://images.example/self-icon.png"
        },
        "source": "test",
        "isFriend": false
    })]);
    runtime.runtime().deps.event_bus.take_events_for_test();
    let notification = json!({
        "id": "notif-avatar-self-sender",
        "createdAt": "2026-06-21T00:00:00.000Z",
        "type": "friendRequest",
        "senderUserId": "usr_self",
        "senderUsername": "Self",
        "message": "Friend request"
    });

    runtime
        .runtime()
        .apply_notification_output(RealtimeNotificationOutput {
            owner_user_id: OwnerId::new(active_session.user_id),
            projection: RealtimeNotificationProjection {
                generation: 7,
                upserts: vec![RealtimeNotificationUpsert {
                    notification: notification.clone().into(),
                    insert_defaults: None,
                    notify_menu: true,
                    deliver_runtime: true,
                    run_automation: false,
                }],
                ..RealtimeNotificationProjection::default()
            },
            persistence: RealtimePersistenceBatch {
                notification_v2_upserts: vec![notification],
                ..RealtimePersistenceBatch::default()
            },
        });

    let events = runtime.runtime().deps.event_bus.take_events_for_test();
    let projection = events
        .iter()
        .find(|event| event.name == "realtimeNotificationProjection")
        .expect("self-sender notification should emit a realtime projection");
    let projected = &projection.payload["upserts"][0]["notification"];
    assert!(projected["imageUrl"].is_null());

    let delivered = runtime
        .activity_sink_for_test()
        .notification_by_id("notif-avatar-self-sender")
        .expect("runtime delivery should reach the activity sink");
    assert_eq!(delivered.actor.user_id, "usr_self");
    assert!(delivered.facts.image_url.is_empty());
    Ok(())
}

#[test]
fn notification_avatar_fallback_preserves_existing_image_and_skips_group_sender() -> Result<()> {
    let (_dir, runtime, active_session) =
        runtime_with_active_session("notification-avatar-existing-and-group")?;
    runtime.runtime().ingest_user_facts(vec![json!({
        "user": {
            "id": "usr_sender",
            "displayName": "Cached Sender",
            "iconUrl": "https://images.example/user-icon.png"
        },
        "source": "test",
        "isFriend": false
    })]);
    runtime.runtime().deps.event_bus.take_events_for_test();
    let existing_image = json!({
        "id": "notif-avatar-existing",
        "createdAt": "2026-06-21T00:00:00.000Z",
        "type": "friendRequest",
        "senderUserId": "usr_sender",
        "senderUsername": "Cached Sender",
        "message": "Friend request",
        "imageUrl": "https://images.example/existing.png"
    });
    let group_sender = json!({
        "id": "notif-avatar-group",
        "createdAt": "2026-06-21T00:00:01.000Z",
        "type": "friendRequest",
        "senderUserId": "grp_sender",
        "senderUsername": "Group Sender",
        "message": "Group request"
    });

    runtime
        .runtime()
        .apply_notification_output(RealtimeNotificationOutput {
            owner_user_id: OwnerId::new(active_session.user_id),
            projection: RealtimeNotificationProjection {
                generation: 7,
                upserts: vec![
                    RealtimeNotificationUpsert {
                        notification: existing_image.clone().into(),
                        insert_defaults: None,
                        notify_menu: true,
                        deliver_runtime: true,
                        run_automation: false,
                    },
                    RealtimeNotificationUpsert {
                        notification: group_sender.clone().into(),
                        insert_defaults: None,
                        notify_menu: true,
                        deliver_runtime: true,
                        run_automation: false,
                    },
                ],
                ..RealtimeNotificationProjection::default()
            },
            persistence: RealtimePersistenceBatch {
                notification_v2_upserts: vec![existing_image, group_sender],
                ..RealtimePersistenceBatch::default()
            },
        });

    let events = runtime.runtime().deps.event_bus.take_events_for_test();
    let projection = events
        .iter()
        .find(|event| event.name == "realtimeNotificationProjection")
        .expect("notifications should emit a realtime projection");
    let upserts = projection.payload["upserts"]
        .as_array()
        .expect("projection upserts");
    let existing = upserts
        .iter()
        .find(|upsert| upsert["notification"]["id"] == "notif-avatar-existing")
        .expect("existing image notification");
    let group = upserts
        .iter()
        .find(|upsert| upsert["notification"]["id"] == "notif-avatar-group")
        .expect("group notification");
    assert_eq!(
        existing["notification"]["imageUrl"],
        "https://images.example/existing.png"
    );
    assert!(group["notification"]["imageUrl"].is_null());
    Ok(())
}

#[test]
fn unresolved_person_location_notification_persists_without_runtime_projection() -> Result<()> {
    let (_dir, runtime, active_session) =
        runtime_with_active_session("notification-unresolved-basic")?;
    runtime.set_task_executor_for_test(DiscardTaskExecutor);
    let notification = json!({
        "id": "notif-unresolved",
        "createdAt": "2026-06-21T00:00:00.000Z",
        "type": "invite",
        "senderUserId": "usr_missing",
        "senderUsername": "usr_missing",
        "message": "Join me",
        "details": {
            "worldId": "wrld_missing",
            "worldName": "wrld_missing"
        }
    });

    runtime
        .runtime()
        .apply_notification_output(RealtimeNotificationOutput {
            owner_user_id: OwnerId::new(active_session.user_id.clone()),
            projection: RealtimeNotificationProjection {
                generation: 7,
                upserts: vec![RealtimeNotificationUpsert {
                    notification: notification.clone().into(),
                    insert_defaults: None,
                    notify_menu: true,
                    deliver_runtime: true,
                    run_automation: true,
                }],
                ..RealtimeNotificationProjection::default()
            },
            persistence: RealtimePersistenceBatch {
                notification_v2_upserts: vec![notification],
                ..RealtimePersistenceBatch::default()
            },
        });

    let events = runtime.runtime().deps.event_bus.take_events_for_test();
    assert!(
        events
            .iter()
            .all(|event| event.name != "realtimeNotificationProjection"),
        "unresolved notification should not be emitted to runtime/UI projection"
    );

    let rows = notification_list_query(
        runtime.database(),
        NotificationListQueryInput {
            user_id: active_session.user_id,
            search: String::new(),
            filters: Vec::new(),
            per_table_limit: 10,
            limit: 10,
            include_unseen: false,
        },
    )?;
    let row = rows
        .iter()
        .find(|row| row.id == "notif-unresolved")
        .expect("unresolved notification should still be persisted");
    assert_eq!(row.sender_user_id, "usr_missing");
    assert_eq!(row.sender_username, "");
    assert_eq!(row.details["worldId"], "wrld_missing");
    assert_eq!(row.details["worldName"], "");
    assert!(
        runtime
            .runtime()
            .state
            .lock()
            .unwrap()
            .world_enrichment
            .inflight
            .contains("wrld_missing"),
        "notification resolver failures should register async world warm"
    );
    Ok(())
}

#[test]
fn resolved_sender_does_not_wait_for_world_or_avatar() -> Result<()> {
    let (_dir, runtime, active_session) =
        runtime_with_active_session("notification-resolved-sender-only")?;
    let notification = json!({
        "id": "notif-resolved-sender-only",
        "createdAt": "2026-06-21T00:00:00.000Z",
        "type": "invite",
        "senderUserId": "usr_sender",
        "senderUsername": "Ready Sender",
        "message": "Join me",
        "details": {
            "worldId": "wrld_missing",
            "worldName": "wrld_missing"
        }
    });

    runtime
        .runtime()
        .apply_notification_output(RealtimeNotificationOutput {
            owner_user_id: OwnerId::new(active_session.user_id),
            projection: RealtimeNotificationProjection {
                generation: 7,
                upserts: vec![RealtimeNotificationUpsert {
                    notification: notification.into(),
                    insert_defaults: None,
                    notify_menu: false,
                    deliver_runtime: true,
                    run_automation: false,
                }],
                ..RealtimeNotificationProjection::default()
            },
            ..RealtimeNotificationOutput::default()
        });

    let delivered = runtime
        .activity_sink_for_test()
        .notification_by_id("notif-resolved-sender-only")
        .expect("resolved sender should be delivered without world or avatar resolution");
    assert_eq!(delivered.actor.display_name, "Ready Sender");
    assert!(delivered.facts.world_name.is_empty());
    assert!(delivered.facts.image_url.is_empty());
    Ok(())
}

#[test]
fn cached_user_notification_image_url_returns_none_before_cache_populated() -> Result<()> {
    let (_dir, runtime, _active_session) =
        runtime_with_active_session("cached-user-image-url-miss")?;

    assert_eq!(
        runtime
            .runtime()
            .cached_user_notification_image_url(&runtime.runtime().active_endpoint(), "usr_target"),
        None
    );
    Ok(())
}

#[test]
fn cached_user_notification_image_url_reads_realtime_cache_hit() -> Result<()> {
    let (_dir, runtime, _active_session) =
        runtime_with_active_session("cached-user-image-url-hit")?;
    runtime.runtime().ingest_user_facts(vec![json!({
        "user": {
            "id": "usr_target",
            "displayName": "Target",
            "iconUrl": "https://images.example/user-icon.png"
        },
        "source": "test",
        "isFriend": true
    })]);
    let endpoint = runtime.runtime().active_endpoint();

    assert_eq!(
        runtime
            .runtime()
            .cached_user_notification_image_url(&endpoint, "usr_target"),
        Some("https://images.example/user-icon.png".into())
    );
    Ok(())
}

#[test]
fn notification_facts_prefer_the_current_friend_record() -> Result<()> {
    let (_dir, runtime, active_session) =
        runtime_with_active_session("notification-current-friend-facts")?;
    let endpoint = runtime.runtime().active_endpoint();
    runtime.runtime().ingest_user_facts(vec![json!({
        "user": {
            "id": "usr_target",
            "iconUrl": "https://images.example/stale.png"
        },
        "source": "test",
        "isFriend": true
    })]);
    runtime.runtime().friends.set_baseline(
        vrcx_0_core::friends::FriendRosterBaseline {
            current_user_id: active_session.user_id.clone(),
            endpoint: endpoint.clone(),
            friends_by_id: [(
                "usr_target".to_string(),
                vrcx_0_core::friends::FriendBaselineEntry {
                    record: vrcx_0_core::friends::FriendRecord {
                        id: "usr_target".into(),
                        display_name: "Current Friend".into(),
                        icon_url: "https://images.example/current.png".into(),
                        extra: json!({
                            "world": { "name": "Current World" }
                        })
                        .as_object()
                        .cloned()
                        .unwrap(),
                        ..vrcx_0_core::friends::FriendRecord::default()
                    },
                    presence: vrcx_0_core::friends::FriendBaselinePresence {
                        location: "wrld_target:instance~region(jp)".into(),
                        ..vrcx_0_core::friends::FriendBaselinePresence::default()
                    },
                },
            )]
            .into_iter()
            .collect(),
            ..vrcx_0_core::friends::FriendRosterBaseline::default()
        },
        7,
        1,
    );

    assert_eq!(
        runtime
            .runtime()
            .cached_user_notification_image_url(&endpoint, "usr_target"),
        Some("https://images.example/current.png".into())
    );
    runtime
        .runtime()
        .apply_notification_output(RealtimeNotificationOutput {
            owner_user_id: OwnerId::new(active_session.user_id),
            projection: RealtimeNotificationProjection {
                generation: 7,
                upserts: vec![RealtimeNotificationUpsert {
                    notification: json!({
                        "id": "notif-current-friend-facts",
                        "createdAt": "2026-06-21T00:00:00.000Z",
                        "type": "friendRequest",
                        "senderUserId": "usr_target",
                        "senderUsername": "usr_target"
                    })
                    .into(),
                    insert_defaults: None,
                    notify_menu: false,
                    deliver_runtime: true,
                    run_automation: false,
                }],
                ..RealtimeNotificationProjection::default()
            },
            ..RealtimeNotificationOutput::default()
        });
    let delivered = runtime
        .activity_sink_for_test()
        .notification_by_id("notif-current-friend-facts")
        .expect("friend notification should be delivered without remote resolution");
    assert_eq!(delivered.actor.display_name, "Current Friend");
    Ok(())
}

#[test]
fn notification_v2_update_sanitizes_id_like_names_before_persistence() -> Result<()> {
    let (_dir, runtime, active_session) =
        runtime_with_active_session("notification-update-sanitize")?;
    let initial = json!({
        "id": "notif-update-sanitize",
        "createdAt": "2026-06-21T00:00:00.000Z",
        "type": "invite",
        "senderUserId": "usr_sender",
        "senderUsername": "Sender",
        "message": "Join me",
        "details": {
            "worldId": "wrld_initial",
            "worldName": "Initial World"
        }
    });
    runtime
        .runtime()
        .apply_notification_output(RealtimeNotificationOutput {
            owner_user_id: OwnerId::new(active_session.user_id.clone()),
            projection: RealtimeNotificationProjection {
                generation: 7,
                upserts: vec![RealtimeNotificationUpsert {
                    notification: initial.clone().into(),
                    insert_defaults: None,
                    notify_menu: false,
                    deliver_runtime: false,
                    run_automation: false,
                }],
                ..RealtimeNotificationProjection::default()
            },
            persistence: RealtimePersistenceBatch {
                notification_v2_upserts: vec![initial],
                ..RealtimePersistenceBatch::default()
            },
        });
    runtime.runtime().deps.event_bus.take_events_for_test();

    let update = json!({
        "id": "notif-update-sanitize",
        "senderUserId": "usr_missing",
        "senderUsername": "usr_missing",
        "details": {
            "worldId": "wrld_missing",
            "worldName": "wrld_missing"
        }
    });
    runtime
        .runtime()
        .apply_notification_output(RealtimeNotificationOutput {
            owner_user_id: OwnerId::new(active_session.user_id.clone()),
            projection: RealtimeNotificationProjection {
                generation: 7,
                upserts: vec![RealtimeNotificationUpsert {
                    notification: update.clone().into(),
                    insert_defaults: Some(
                        json!({
                            "createdAt": "2026-06-21T00:01:00.000Z",
                            "created_at": "2026-06-21T00:01:00.000Z",
                            "seen": false
                        })
                        .into(),
                    ),
                    notify_menu: false,
                    deliver_runtime: false,
                    run_automation: false,
                }],
                ..RealtimeNotificationProjection::default()
            },
            persistence: RealtimePersistenceBatch {
                notification_v2_updates: vec![NotificationV2Update {
                    id: "notif-update-sanitize".into(),
                    updates: update,
                    received_at: "2026-06-21T00:01:00.000Z".into(),
                }],
                ..RealtimePersistenceBatch::default()
            },
        });

    let rows = notification_list_query(
        runtime.database(),
        NotificationListQueryInput {
            user_id: active_session.user_id,
            search: String::new(),
            filters: Vec::new(),
            per_table_limit: 10,
            limit: 10,
            include_unseen: false,
        },
    )?;
    let row = rows
        .iter()
        .find(|row| row.id == "notif-update-sanitize")
        .expect("notification update should be persisted");
    assert_eq!(row.sender_user_id, "usr_missing");
    assert_eq!(row.sender_username, "");
    assert_eq!(row.details["worldId"], "wrld_missing");
    assert_eq!(row.details["worldName"], "");
    Ok(())
}
