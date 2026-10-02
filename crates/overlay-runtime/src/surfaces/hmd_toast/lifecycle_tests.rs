use super::*;
use vrcx_0_contracts::activity::ActivityKind;

#[test]
fn hmd_toast_refresh_hint_waits_until_expiry_without_card_animation() {
    let runtime = VrOverlayRuntime::new_for_test();
    let enqueued_at = Instant::now();
    let timeout = Duration::from_secs(5);
    runtime.enqueue_hmd_toast(hmd_entry("static"), enqueued_at, timeout);

    assert_eq!(runtime.hmd_toast_views(enqueued_at).len(), 1);
    assert_eq!(runtime.hmd_toast_refresh_hint(enqueued_at), Some(timeout));
}

#[test]
fn hmd_toast_expires_at_timeout_without_card_fade_out() {
    let runtime = VrOverlayRuntime::new_for_test();
    let enqueued_at = Instant::now();
    let timeout = Duration::from_secs(5);
    let expires_at = enqueued_at + timeout;
    runtime.enqueue_hmd_toast(hmd_entry("expiry"), enqueued_at, timeout);

    assert_eq!(
        runtime
            .hmd_toast_views(expires_at - Duration::from_millis(1))
            .len(),
        1
    );
    assert!(runtime.hmd_toast_views(expires_at).is_empty());
    assert_eq!(runtime.hmd_toast_refresh_hint(expires_at), None);
}

fn hmd_entry(source_id: &str) -> ActivityEntry {
    ActivityEntry {
        sequence: 1,
        source_id: source_id.to_string(),
        kind: ActivityKind::Status,
        category: vrcx_0_application_activity::ActivityCategory::CurrentInstance,
        created_at: "2026-01-01T00:00:00Z".to_string(),
        actor_user_id: "usr_actor".to_string(),
        actor_display_name: source_id.to_string(),
        content: vrcx_0_application_activity::ActivityContent {
            title: vrcx_0_application_activity::ActivityText::literal(source_id),
            body: vrcx_0_application_activity::ActivityText::literal("Status"),
            location: "wrld_a:123".to_string(),
            ..vrcx_0_application_activity::ActivityContent::default()
        },
        actor_relation: ActivityActorRelation::Favorite,
    }
}
