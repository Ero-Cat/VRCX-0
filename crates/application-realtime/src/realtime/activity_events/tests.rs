use serde_json::json;
use vrcx_0_contracts::activity::{ActivityKind, ActivitySubject};
use vrcx_0_contracts::feed_live::FeedLiveEntry;

use super::{feed_activity_event, notification_activity_event};

#[test]
fn notifications_use_the_sender_as_actor_and_subject() {
    let event = notification_activity_event(&json!({
        "id": "notification-1",
        "type": "invite",
        "createdAt": "2026-05-31T00:02:00.000Z",
        "senderUserId": "usr_sender",
        "senderUsername": "Sender",
        "details": {
            "worldId": "wrld_1",
            "worldName": "Invite World",
            "inviteMessage": "come over"
        }
    }))
    .expect("invite event");

    assert_eq!(event.kind, ActivityKind::Invite);
    assert_eq!(event.source_id, "notification:notification-1");
    assert_eq!(event.actor.user_id, "usr_sender");
    assert_eq!(event.actor.display_name, "Sender");
    assert_eq!(event.subject, ActivitySubject::User("usr_sender".into()));
    assert_eq!(event.facts.location, "wrld_1");
    assert_eq!(event.facts.world_name, "Invite World");
    assert_eq!(event.facts.message, "come over");
}

#[test]
fn notifications_never_use_the_receiver_as_actor() {
    let event = notification_activity_event(&json!({
        "id": "notification-group",
        "type": "group.announcement",
        "createdAt": "2026-05-31T00:02:00.000Z",
        "receiverUserId": "usr_self",
        "userId": "usr_self",
        "message": "Weekly meetup",
        "data": { "groupName": "Maple Club" }
    }))
    .expect("group announcement event");

    assert!(event.actor.user_id.is_empty());
    assert_eq!(event.subject, ActivitySubject::None);
    assert_eq!(event.facts.group_name, "Maple Club");
    assert_eq!(event.facts.message, "Weekly meetup");
}

#[test]
fn notifications_read_a_nested_sender_display_name() {
    let event = notification_activity_event(&json!({
        "id": "notification-1",
        "type": "invite",
        "senderUserId": "usr_sender",
        "details": { "senderDisplayName": "Sender" }
    }))
    .expect("invite event");

    assert_eq!(event.actor.display_name, "Sender");
}

#[test]
fn notifications_without_ids_get_stable_distinct_source_ids() {
    let invite = |message: &str| {
        notification_activity_event(&json!({
            "type": "invite",
            "createdAt": "2026-05-31T00:02:00.000Z",
            "senderUserId": "usr_sender",
            "message": message
        }))
        .expect("invite event")
        .source_id
    };

    let first = invite("first");
    assert!(first.starts_with("notification:invite:usr_sender:2026-05-31T00:02:00.000Z:"));
    assert_eq!(first, invite("first"));
    assert_ne!(first, invite("second"));
}

#[test]
fn unsupported_notification_types_produce_no_activity() {
    assert!(notification_activity_event(&json!({
        "id": "notification-message",
        "type": "message"
    }))
    .is_none());
}

#[test]
fn friend_feed_entries_keep_their_kind_specific_facts() {
    let avatar = feed_activity_event(&FeedLiveEntry::Avatar {
        created_at: "2026-05-31T00:01:00.000Z".into(),
        user_id: "usr_friend".into(),
        display_name: "Friend".into(),
        owner_id: String::new(),
        previous_owner_id: String::new(),
        avatar_name: "New Avatar".into(),
        previous_avatar_name: String::new(),
        current_avatar_image_url: "https://images.example/avatar.png".into(),
        previous_current_avatar_image_url: String::new(),
        owner_user_id: String::new(),
    })
    .expect("avatar event");
    assert_eq!(avatar.kind, ActivityKind::AvatarChange);
    assert_eq!(avatar.facts.avatar_name, "New Avatar");
    assert_eq!(avatar.facts.image_url, "https://images.example/avatar.png");

    let renamed = feed_activity_event(&FeedLiveEntry::DisplayName {
        created_at: "2026-05-31T00:01:00.000Z".into(),
        user_id: "usr_friend".into(),
        display_name: "New Name".into(),
        previous_display_name: "Old Name".into(),
        friend_number: 7,
        owner_user_id: String::new(),
    })
    .expect("display name event");
    assert_eq!(renamed.kind, ActivityKind::DisplayName);
    assert_eq!(renamed.actor.display_name, "New Name");
    assert_eq!(renamed.facts.previous_display_name, "Old Name");

    let joining = feed_activity_event(&FeedLiveEntry::OnPlayerJoining {
        created_at: "2026-07-13T10:00:00Z".into(),
        user_id: "usr_joining".into(),
        display_name: "Joining User".into(),
        location: "traveling".into(),
        traveling_to_location: "wrld_current:456".into(),
        world_name: None,
        world_id: None,
        display_location: None,
        owner_user_id: String::new(),
    })
    .expect("joining event");
    assert!(joining.in_current_instance);
}
