use std::{collections::HashMap, sync::Mutex};

use super::*;
use crate::{OverlayActivityScope, OverlayActivitySurface, OverlayActivitySurfaceFilters};
use serde_json::{json, Value};

#[derive(Default)]
struct TestConfig {
    values: Mutex<HashMap<String, String>>,
}

impl TestConfig {
    fn set_string(&self, key: &str, value: &str) -> vrcx_0_application_core::Result<()> {
        self.values
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn get_json(&self, key: &str, fallback: Value) -> vrcx_0_application_core::Result<Value> {
        self.get_raw(key)?
            .map(|raw| serde_json::from_str(&raw).map_err(Into::into))
            .unwrap_or(Ok(fallback))
    }
}

impl NotificationConfig for TestConfig {
    fn get_raw(&self, key: &str) -> vrcx_0_application_core::Result<Option<String>> {
        Ok(self.values.lock().unwrap().get(key).cloned())
    }

    fn get_bool(&self, key: &str, default_value: bool) -> vrcx_0_application_core::Result<bool> {
        Ok(self
            .get_raw(key)?
            .and_then(|value| value.parse().ok())
            .unwrap_or(default_value))
    }

    fn get_string(
        &self,
        key: &str,
        default_value: &str,
    ) -> vrcx_0_application_core::Result<String> {
        Ok(self
            .get_raw(key)?
            .unwrap_or_else(|| default_value.to_string()))
    }

    fn set_json(&self, key: &str, value: &Value) -> vrcx_0_application_core::Result<()> {
        self.set_string(key, &serde_json::to_string(value)?)
    }
}

fn test_config(_name: &str) -> std::result::Result<((), TestConfig), Box<dyn std::error::Error>> {
    Ok(((), TestConfig::default()))
}

#[test]
fn backend_load_reads_three_independent_surface_keys(
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let (_dir, config) = test_config("overlay-activity-three-keys")?;
    config.set_string(
        "overlayActivityFilters",
        &serde_json::to_string(&json!({
            "version": 1,
            "wrist": { "types": { "invite": { "scope": "on" } } }
        }))?,
    )?;
    config.set_string(
        "desktopNotificationActivityFilters",
        &serde_json::to_string(&json!({
            "version": 1,
            "types": { "invite": { "scope": "allFavorites" } }
        }))?,
    )?;
    config.set_string(
        "vrNotificationActivityFilters",
        &serde_json::to_string(&json!({
            "version": 1,
            "types": { "invite": { "scope": "off" } }
        }))?,
    )?;
    let filters = load_overlay_activity_filters(&config);
    assert_eq!(
        filters
            .rule_for(OverlayActivitySurface::Wrist, "invite")
            .scope,
        OverlayActivityScope::On
    );
    assert_eq!(
        filters
            .rule_for(OverlayActivitySurface::Desktop, "invite")
            .scope,
        OverlayActivityScope::AllFavorites
    );
    assert_eq!(
        filters.rule_for(OverlayActivitySurface::Vr, "invite").scope,
        OverlayActivityScope::Off
    );
    Ok(())
}

#[test]
fn backend_load_reads_webhook_surface_key() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let (_dir, config) = test_config("overlay-activity-webhook-key")?;
    config.set_string(
        "webhookActivityFilters",
        &serde_json::to_string(&json!({
            "version": 1,
            "types": { "invite": { "scope": "on" } }
        }))?,
    )?;
    let filters = load_overlay_activity_filters(&config);
    assert_eq!(
        filters
            .rule_for(OverlayActivitySurface::Webhook, "invite")
            .scope,
        OverlayActivityScope::On
    );
    Ok(())
}

#[test]
fn backend_load_seeds_tts_filters_from_desktop_once(
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let (_dir, config) = test_config("overlay-activity-tts-seed-desktop")?;
    config.set_string(
        "desktopNotificationActivityFilters",
        &serde_json::to_string(&json!({
            "version": 1,
            "types": { "invite": { "scope": "allFavorites" } }
        }))?,
    )?;
    config.set_string(
        "vrNotificationActivityFilters",
        &serde_json::to_string(&json!({
            "version": 1,
            "types": { "invite": { "scope": "off" } }
        }))?,
    )?;
    let filters = load_overlay_activity_filters(&config);
    assert_eq!(
        filters
            .rule_for(OverlayActivitySurface::Tts, "invite")
            .scope,
        OverlayActivityScope::AllFavorites
    );
    let saved = config.get_json("ttsNotificationActivityFilters", json!({}))?;
    let saved = OverlayActivitySurfaceFilters::from_types_json(&saved);
    assert_eq!(
        saved.types.get("invite").unwrap().scope,
        OverlayActivityScope::AllFavorites
    );
    Ok(())
}

#[test]
fn backend_load_seeds_tts_filters_from_vr_when_desktop_is_off(
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let (_dir, config) = test_config("overlay-activity-tts-seed-vr")?;
    config.set_string(
        "desktopNotificationActivityFilters",
        &serde_json::to_string(&json!({
            "version": 1,
            "types": { "invite": { "scope": "off" } }
        }))?,
    )?;
    config.set_string(
        "vrNotificationActivityFilters",
        &serde_json::to_string(&json!({
            "version": 1,
            "types": { "invite": { "scope": "friends" } }
        }))?,
    )?;
    let filters = load_overlay_activity_filters(&config);

    assert_eq!(
        filters
            .rule_for(OverlayActivitySurface::Tts, "invite")
            .scope,
        OverlayActivityScope::Friends
    );
    Ok(())
}

#[test]
fn backend_save_updates_only_requested_notification_surface(
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let (_dir, config) = test_config("overlay-activity-save-surface")?;
    config.set_string("desktopNotificationActivityFilters", "desktop-before")?;
    let filters = OverlayActivityFilterProfile {
        version: 9,
        types: [(
            "future.activity".to_string(),
            crate::OverlayActivityRule {
                scope: OverlayActivityScope::On,
                favorite_group_keys: crate::OverlayActivityFavoriteGroupKeys::All,
            },
        )]
        .into(),
    };

    let saved = save_notification_activity_filters(
        &config,
        NotificationActivityFiltersSetInput {
            surface: NotificationActivityFilterSurface::Tts,
            filters,
        },
    )?;

    assert_eq!(saved.version, 1);
    assert!(saved.types.contains_key("future.activity"));
    assert_eq!(
        config.get_string("desktopNotificationActivityFilters", "")?,
        "desktop-before"
    );
    assert!(config
        .get_string("ttsNotificationActivityFilters", "")?
        .contains("future.activity"));
    Ok(())
}

#[test]
fn location_hidden_users_load_from_the_feed_list_unless_the_switch_is_off(
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let (_dir, config) = test_config("location-hidden-users")?;
    config.set_string(
        "feedHiddenUsers",
        &serde_json::to_string(&json!([" usr_a ", { "userId": "usr_b" }, ""]))?,
    )?;
    assert_eq!(
        load_location_hidden_user_ids(&config),
        HashSet::from(["usr_a".to_string(), "usr_b".to_string()])
    );

    config.set_string("feedHiddenUsersHideNotifications", "false")?;
    assert!(load_location_hidden_user_ids(&config).is_empty());
    Ok(())
}
