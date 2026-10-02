#[cfg(test)]
mod tests {

    use super::super::presence_test_support::{friend_view, is_pending_offline, location_tag};
    use super::super::*;

    fn runtime_with_online_status(status: &str) -> RealtimeFriendsRuntime {
        let runtime = RealtimeFriendsRuntime::default();
        runtime.set_baseline(
            FriendRosterBaseline {
                current_user_id: "usr_self".into(),
                friends_by_id: [(
                    "usr_friend".to_string(),
                    FriendBaselineEntry {
                        record: FriendRecord {
                            id: "usr_friend".into(),
                            display_name: "Friend".into(),
                            status: status.into(),
                            ..FriendRecord::default()
                        },
                        presence: FriendBaselinePresence {
                            state: "online".into(),
                            ..FriendBaselinePresence::default()
                        },
                    },
                )]
                .into_iter()
                .collect(),
                ..FriendRosterBaseline::default()
            },
            1,
            0,
        );
        runtime
    }

    #[test]
    fn refetched_profile_trust_change_upserts_and_projects_once() {
        let runtime = RealtimeFriendsRuntime::default();
        runtime.set_baseline(
            FriendRosterBaseline {
                current_user_id: "usr_self".into(),
                friends_by_id: [(
                    "usr_friend".to_string(),
                    FriendBaselineEntry {
                        record: FriendRecord {
                            id: "usr_friend".into(),
                            display_name: "Friend".into(),
                            extra: [
                                ("$trustLevel".into(), json!("User")),
                                ("trustLevel".into(), json!("User")),
                                ("tags".into(), json!(["system_trust_known"])),
                            ]
                            .into_iter()
                            .collect(),
                            ..FriendRecord::default()
                        },
                        presence: FriendBaselinePresence {
                            state: "offline".into(),
                            location: "offline".into(),
                            ..FriendBaselinePresence::default()
                        },
                    },
                )]
                .into_iter()
                .collect(),
                ..FriendRosterBaseline::default()
            },
            1,
            0,
        );
        let profile = json!({
            "id": "usr_friend",
            "displayName": "Friend",
            "state": "offline",
            "location": "offline",
            "tags": ["system_trust_veteran"]
        });

        let first_rev = runtime.friend_rev_of(1, "usr_friend").unwrap_or_default();
        let RealtimeFriendApplyResult::Output(first) = runtime.apply_refetched_user_profile_if_rev(
            1,
            "usr_friend",
            first_rev,
            profile.clone(),
            "2026-05-15T00:00:01Z",
        ) else {
            panic!("trust-changing profile should produce an output");
        };
        assert_eq!(first.persistence.friend_log_upserts.len(), 1);
        assert_eq!(
            first.persistence.friend_log_upserts[0].trust_level,
            "Trusted User"
        );
        assert_eq!(
            first
                .persistence
                .feed_entries
                .iter()
                .filter(|entry| entry.to_json()["type"] == "TrustLevel")
                .count(),
            1
        );
        assert_eq!(
            first
                .persistence
                .feed_entries
                .iter()
                .filter(|entry| entry.to_json()["type"] == "TrustLevel")
                .count(),
            1
        );

        let second_rev = runtime.friend_rev_of(1, "usr_friend").unwrap_or_default();
        if let RealtimeFriendApplyResult::Output(second) = runtime
            .apply_refetched_user_profile_if_rev(
                1,
                "usr_friend",
                second_rev,
                profile,
                "2026-05-15T00:00:02Z",
            )
        {
            assert!(second.persistence.friend_log_upserts.is_empty());
            assert!(second
                .persistence
                .feed_entries
                .iter()
                .all(|entry| entry.to_json()["type"] != "TrustLevel"));
        }
    }

    #[test]
    fn refetched_friend_profile_updates_offline_real_location_to_online() {
        let runtime = RealtimeFriendsRuntime::default();
        runtime.set_baseline(
            FriendRosterBaseline {
                current_user_id: "usr_self".into(),
                friends_by_id: [(
                    "usr_friend".to_string(),
                    FriendBaselineEntry {
                        record: FriendRecord {
                            id: "usr_friend".into(),
                            display_name: "Friend".into(),
                            ..FriendRecord::default()
                        },
                        presence: FriendBaselinePresence {
                            state: "offline".into(),
                            location: "wrld_2:456".into(),
                            ..FriendBaselinePresence::default()
                        },
                    },
                )]
                .into_iter()
                .collect(),
                ..FriendRosterBaseline::default()
            },
            1,
            0,
        );

        let rev = runtime.friend_rev_of(1, "usr_friend").unwrap_or_default();
        let RealtimeFriendApplyResult::Output(output) = runtime
            .apply_refetched_user_profile_if_rev(
                1,
                "usr_friend",
                rev,
                json!({
                    "id": "usr_friend",
                    "displayName": "Friend",
                    "state": "online",
                    "location": "wrld_2:456"
                }),
                "2026-05-15T00:00:01Z",
            )
        else {
            panic!("refetched friend profile should produce an output");
        };

        assert_eq!(
            output.projection.patches[0]
                .presence
                .view
                .section()
                .as_str(),
            "online"
        );
        assert_eq!(
            friend_view(&runtime, "usr_friend").section().as_str(),
            "online"
        );
        assert_eq!(output.persistence.feed_entries.len(), 1);
        let entry = output.persistence.feed_entries[0].to_json();
        assert_eq!(entry["type"], "Online");
        assert_eq!(entry["location"], "wrld_2:456");
    }

    #[test]
    fn refetched_offline_profile_enters_pending_without_status_feed() {
        let runtime = RealtimeFriendsRuntime::default();
        runtime.set_baseline(
            FriendRosterBaseline {
                current_user_id: "usr_self".into(),
                friends_by_id: [(
                    "usr_friend".to_string(),
                    FriendBaselineEntry {
                        record: FriendRecord {
                            id: "usr_friend".into(),
                            display_name: "Friend".into(),
                            status: "join me".into(),
                            status_description: "Old status".into(),
                            ..FriendRecord::default()
                        },
                        presence: FriendBaselinePresence {
                            state: "online".into(),
                            location: "wrld_old:123".into(),
                            ..FriendBaselinePresence::default()
                        },
                    },
                )]
                .into_iter()
                .collect(),
                ..FriendRosterBaseline::default()
            },
            1,
            0,
        );

        let rev = runtime.friend_rev_of(1, "usr_friend").unwrap_or_default();
        let RealtimeFriendApplyResult::Output(output) = runtime
            .apply_refetched_user_profile_if_rev(
                1,
                "usr_friend",
                rev,
                json!({
                    "id": "usr_friend",
                    "displayName": "Friend",
                    "state": "offline",
                    "location": "offline",
                    "status": "active",
                    "statusDescription": "Fresh REST status"
                }),
                "2026-05-15T00:00:01Z",
            )
        else {
            panic!("refetched friend profile should produce an output");
        };

        let view = &output.projection.patches[0].presence.view;
        assert_eq!(view.section().as_str(), "online");
        assert!(is_pending_offline(view));
        assert!(output.persistence.feed_entries.is_empty());
        assert!(output.wake.is_some());
        let fired = runtime
            .wake("usr_friend", "2026-05-15T00:03:00Z")
            .expect("pending finalizes");
        let entry = fired.persistence.feed_entries[0].to_json();
        assert_eq!(entry["type"], "Offline");
        assert_eq!(entry["location"], "wrld_old:123");
    }

    #[test]
    fn refetched_online_profile_cancels_pending_offline_timer() {
        let runtime = RealtimeFriendsRuntime::default();
        runtime.set_baseline(
            FriendRosterBaseline {
                current_user_id: "usr_self".into(),
                friends_by_id: [(
                    "usr_friend".to_string(),
                    FriendBaselineEntry {
                        record: FriendRecord {
                            id: "usr_friend".into(),
                            display_name: "Friend".into(),
                            ..FriendRecord::default()
                        },
                        presence: FriendBaselinePresence {
                            state: "online".into(),
                            location: "wrld_old:123".into(),
                            ..FriendBaselinePresence::default()
                        },
                    },
                )]
                .into_iter()
                .collect(),
                ..FriendRosterBaseline::default()
            },
            1,
            0,
        );

        let RealtimeFriendApplyResult::Output(location_output) =
            runtime.apply_ws_message(&RealtimeWsMessagePayload {
                json: json!({
                    "type": "friend-location",
                    "content": {
                        "userId": "usr_friend",
                        "location": "offline",
                        "user": {
                            "id": "usr_friend",
                            "displayName": "Friend",
                            "location": "offline"
                        }
                    }
                }),
                raw: "{}".into(),
                received_at: "2026-05-15T00:00:00Z".into(),
            })
        else {
            panic!("friend-location should produce an output");
        };
        assert!(
            location_output.wake.is_some(),
            "offline location should schedule pending timer"
        );

        let rev = runtime.friend_rev_of(1, "usr_friend").unwrap_or_default();
        let RealtimeFriendApplyResult::Output(output) = runtime
            .apply_refetched_user_profile_if_rev(
                1,
                "usr_friend",
                rev,
                json!({
                    "id": "usr_friend",
                    "displayName": "Friend",
                    "state": "online",
                    "location": "wrld_fresh:456"
                }),
                "2026-05-15T00:00:01Z",
            )
        else {
            panic!("refetched friend profile should produce an output");
        };

        let view = &output.projection.patches[0].presence.view;
        assert_eq!(view.section().as_str(), "online");
        let feed_types = output
            .persistence
            .feed_entries
            .iter()
            .map(|entry| entry.to_json()["type"].clone())
            .collect::<Vec<_>>();
        assert_eq!(feed_types, vec![json!("GPS")]);
        assert!(!is_pending_offline(view));
        assert!(runtime.wake("usr_friend", "2026-05-15T00:03:00Z").is_none());
    }

    #[test]
    fn stale_refetched_profile_does_not_overwrite_newer_websocket_status() {
        let runtime = runtime_with_online_status("ask me");
        let refetch_rev = runtime
            .friend_rev_of(1, "usr_friend")
            .expect("friend should have a rev");

        let RealtimeFriendApplyResult::Output(_) =
            runtime.apply_ws_message(&RealtimeWsMessagePayload {
                json: json!({
                    "type": "friend-update",
                    "content": {
                        "userId": "usr_friend",
                        "user": {
                            "id": "usr_friend",
                            "displayName": "Friend",
                            "state": "online",
                            "status": "active",
                            "statusDescription": "freeggs"
                        }
                    }
                }),
                raw: "{}".into(),
                received_at: "2026-05-15T00:00:01Z".into(),
            })
        else {
            panic!("websocket status update should produce an output");
        };

        let result = runtime.apply_refetched_user_profile_if_rev(
            1,
            "usr_friend",
            refetch_rev,
            json!({
                "id": "usr_friend",
                "displayName": "Friend",
                "state": "online",
                "status": "ask me",
                "statusDescription": ""
            }),
            "2026-05-15T00:00:02Z",
        );

        assert!(matches!(result, RealtimeFriendApplyResult::Ignored));
        let snapshot = runtime.snapshot().unwrap();
        let friend = &snapshot.friends_by_id["usr_friend"];
        assert_eq!(friend.status, "active");
        assert_eq!(friend.status_description, "freeggs");
    }

    #[test]
    fn stale_refetched_profile_does_not_revert_display_name() {
        let runtime = runtime_with_online_status("active");
        let refetch_rev = runtime
            .friend_rev_of(1, "usr_friend")
            .expect("friend should have a rev");

        let RealtimeFriendApplyResult::Output(rename) =
            runtime.apply_ws_message(&RealtimeWsMessagePayload {
                json: json!({
                    "type": "friend-update",
                    "content": {
                        "userId": "usr_friend",
                        "user": {
                            "id": "usr_friend",
                            "displayName": "Fresh Name",
                            "state": "offline",
                            "status": "active"
                        }
                    }
                }),
                raw: "{}".into(),
                received_at: "2026-05-15T00:00:01Z".into(),
            })
        else {
            panic!("websocket rename should produce an output");
        };
        assert_eq!(rename.persistence.friend_log_upserts.len(), 1);
        assert_eq!(
            rename.persistence.friend_log_upserts[0].display_name,
            "Fresh Name"
        );
        assert!(rename.projection.friend_log_changed);
        let rename_feed: Vec<_> = rename
            .persistence
            .feed_entries
            .iter()
            .map(|entry| entry.to_json())
            .filter(|entry| entry["type"] == "DisplayName")
            .collect();
        assert_eq!(rename_feed.len(), 1);
        assert_eq!(rename_feed[0]["displayName"], "Fresh Name");
        assert_eq!(rename_feed[0]["previousDisplayName"], "Friend");

        let result = runtime.apply_refetched_user_profile_if_rev(
            1,
            "usr_friend",
            refetch_rev,
            json!({
                "id": "usr_friend",
                "displayName": "Friend",
                "state": "online",
                "status": "active"
            }),
            "2026-05-15T00:00:02Z",
        );

        assert!(matches!(result, RealtimeFriendApplyResult::Ignored));
        let snapshot = runtime.snapshot().unwrap();
        assert_eq!(
            snapshot.friends_by_id["usr_friend"].display_name,
            "Fresh Name"
        );
    }

    #[test]
    fn stale_refetched_profile_does_not_overwrite_newer_websocket_location() {
        let runtime = runtime_with_online_status("active");
        let refetch_rev = runtime
            .friend_rev_of(1, "usr_friend")
            .expect("friend should have a rev");

        let RealtimeFriendApplyResult::Output(_) =
            runtime.apply_ws_message(&RealtimeWsMessagePayload {
                json: json!({
                    "type": "friend-location",
                    "content": {
                        "userId": "usr_friend",
                        "location": "wrld_new:123",
                        "user": {
                            "id": "usr_friend",
                            "displayName": "Friend",
                            "location": "wrld_new:123"
                        }
                    }
                }),
                raw: "{}".into(),
                received_at: "2026-05-15T00:00:01Z".into(),
            })
        else {
            panic!("friend-location should produce an output");
        };

        let result = runtime.apply_refetched_user_profile_if_rev(
            1,
            "usr_friend",
            refetch_rev,
            json!({
                "id": "usr_friend",
                "displayName": "Friend",
                "state": "offline",
                "location": "offline"
            }),
            "2026-05-15T00:00:02Z",
        );

        assert!(matches!(result, RealtimeFriendApplyResult::Ignored));
        let friend = friend_view(&runtime, "usr_friend");
        assert_eq!(location_tag(&friend), Some("wrld_new:123"));
        assert_eq!(friend.section().as_str(), "online");
    }

    #[test]
    fn refetched_profile_applies_when_friend_rev_is_unchanged() {
        let runtime = runtime_with_online_status("ask me");
        let refetch_rev = runtime
            .friend_rev_of(1, "usr_friend")
            .expect("friend should have a rev");

        let RealtimeFriendApplyResult::Output(_) = runtime.apply_refetched_user_profile_if_rev(
            1,
            "usr_friend",
            refetch_rev,
            json!({
                "id": "usr_friend",
                "displayName": "Friend",
                "state": "online",
                "status": "active",
                "statusDescription": "freeggs"
            }),
            "2026-05-15T00:00:01Z",
        ) else {
            panic!("current refetched profile should produce an output");
        };

        let snapshot = runtime.snapshot().unwrap();
        let friend = &snapshot.friends_by_id["usr_friend"];
        assert_eq!(friend.status, "active");
        assert_eq!(friend.status_description, "freeggs");
    }

    #[test]
    fn refetched_profile_does_not_add_unknown_friend() {
        let runtime = RealtimeFriendsRuntime::default();
        runtime.set_baseline(
            FriendRosterBaseline {
                current_user_id: "usr_self".into(),
                ..FriendRosterBaseline::default()
            },
            1,
            0,
        );

        let result = runtime.apply_refetched_user_profile_if_rev(
            1,
            "usr_stranger",
            0,
            json!({
                "id": "usr_stranger",
                "displayName": "Stranger",
                "state": "online"
            }),
            "2026-05-15T00:00:00Z",
        );

        assert!(matches!(result, RealtimeFriendApplyResult::Ignored));
        assert!(!runtime
            .snapshot()
            .unwrap()
            .friends_by_id
            .contains_key("usr_stranger"));
    }
}
