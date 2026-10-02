use std::sync::Arc;

use vrcx_0_application_activity::{
    ActivityActorRelation, ActivityCategory, ActivityContent, ActivityDelivery, ActivityEntry,
};
use vrcx_0_application_core::{
    HostSessionGameProcessStatus, HostSessionRuntime, RuntimeEventBus, TaskSupervisor,
};
use vrcx_0_contracts::activity::ActivityKind;
use vrcx_0_outbound_adapters::LocalAuthCredentialStore;
use vrcx_0_persistence::config::ConfigRepository;

use super::LocalNotificationPolicy;
use crate::notification::test_support::{test_config, TestDir};
use crate::notification::{NotificationDoNotDisturbMode, NotificationDoNotDisturbRuntime};
use crate::privacy_lock::PrivacyLockRuntime;
use crate::RuntimeHost;

fn policy(name: &str) -> (TestDir, ConfigRepository, LocalNotificationPolicy) {
    let (dir, config) = test_config(name);
    let session = HostSessionRuntime::new();
    session.apply_game_process_status(HostSessionGameProcessStatus {
        is_game_running: true,
        is_steamvr_running: true,
        changed_at: "2026-10-02T00:00:00Z".into(),
    });
    let do_not_disturb = NotificationDoNotDisturbRuntime::new(
        config.clone(),
        RuntimeEventBus::new(),
        RuntimeHost::new(),
        TaskSupervisor::new(),
    )
    .unwrap();
    let privacy_lock = Arc::new(
        PrivacyLockRuntime::new(
            Arc::new(LocalAuthCredentialStore::from_repository(config.clone())),
            RuntimeEventBus::new(),
        )
        .unwrap(),
    );
    let policy =
        LocalNotificationPolicy::new(config.clone(), session, do_not_disturb, privacy_lock);
    (dir, config, policy)
}

fn desktop_delivery() -> ActivityDelivery {
    ActivityDelivery {
        entry: ActivityEntry {
            sequence: 1,
            source_id: "game-log:join".into(),
            kind: ActivityKind::OnPlayerJoined,
            category: ActivityCategory::CurrentInstance,
            created_at: "2026-10-02T00:00:00.000Z".into(),
            actor_user_id: "usr_traveler".into(),
            actor_display_name: "Traveler".into(),
            content: ActivityContent::default(),
            actor_relation: ActivityActorRelation::None,
        },
        desktop: true,
        vr: false,
        hmd: false,
        webhook: false,
        tts: false,
    }
}

#[test]
fn settings_follow_config_writes() {
    let (_dir, config, policy) = policy("policy-settings");

    assert!(policy.images_enabled());
    config.set_bool("imageNotifications", false).unwrap();
    assert!(!policy.images_enabled());
}

#[test]
fn the_hmd_condition_follows_the_overlay_toast_setting() {
    let (_dir, config, policy) = policy("policy-hmd-condition");

    assert!(policy.hmd_allowed());
    config.set_string("overlayToast", "Game Closed").unwrap();
    assert!(!policy.hmd_allowed());
    config.set_string("overlayToast", "Always").unwrap();
    assert!(policy.hmd_allowed());
}

#[test]
fn do_not_disturb_pauses_every_local_surface() {
    let (_dir, config, policy) = policy("policy-dnd");
    config.set_string("desktopToast", "Always").unwrap();
    config.set_string("overlayToast", "Always").unwrap();

    assert!(policy.plan(&desktop_delivery(), &policy.settings()).desktop);
    policy
        .do_not_disturb
        .set_mode(NotificationDoNotDisturbMode::UntilStopped)
        .unwrap();
    assert!(!policy
        .plan(&desktop_delivery(), &policy.settings())
        .has_local_transport());
    assert!(!policy.hmd_allowed());
}
