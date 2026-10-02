use std::collections::{BTreeSet, HashSet};

use crate::{
    OverlayActivityFilters, OverlayActivityRule, OverlayActivityScope, OverlayActivitySurface,
    OverlayActivitySurfaceFilters,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use vrcx_0_application_core::Result;

use super::NotificationConfig;

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OverlayActivityPreferenceFilters {
    pub version: u32,
    pub wrist: OverlayActivityPreferenceSurface,
    pub hmd: OverlayActivityPreferenceSurface,
}

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OverlayActivityPreferenceSurface {
    pub types: std::collections::BTreeMap<String, OverlayActivityRule>,
}

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OverlayActivityFilterProfile {
    pub version: u32,
    pub types: std::collections::BTreeMap<String, OverlayActivityRule>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum NotificationActivityFilterSurface {
    Vr,
    Hmd,
    Desktop,
    Webhook,
    Tts,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationActivityFiltersSetInput {
    pub surface: NotificationActivityFilterSurface,
    pub filters: OverlayActivityFilterProfile,
}

pub fn save_overlay_activity_preference_filters(
    config: &dyn NotificationConfig,
    mut filters: OverlayActivityPreferenceFilters,
) -> Result<OverlayActivityPreferenceFilters> {
    filters.version = 1;
    config.set_json("overlayActivityFilters", &serde_json::to_value(&filters)?)?;
    Ok(filters)
}

pub fn save_notification_activity_filters(
    config: &dyn NotificationConfig,
    input: NotificationActivityFiltersSetInput,
) -> Result<OverlayActivityFilterProfile> {
    let mut filters = input.filters;
    filters.version = 1;
    config.set_json(
        notification_activity_filter_key(input.surface),
        &serde_json::to_value(&filters)?,
    )?;
    Ok(filters)
}

fn notification_activity_filter_key(surface: NotificationActivityFilterSurface) -> &'static str {
    match surface {
        NotificationActivityFilterSurface::Vr => "vrNotificationActivityFilters",
        NotificationActivityFilterSurface::Hmd => "hmdNotificationActivityFilters",
        NotificationActivityFilterSurface::Desktop => "desktopNotificationActivityFilters",
        NotificationActivityFilterSurface::Webhook => "webhookActivityFilters",
        NotificationActivityFilterSurface::Tts => "ttsNotificationActivityFilters",
    }
}

pub fn load_overlay_activity_filters(config: &dyn NotificationConfig) -> OverlayActivityFilters {
    let mut filters = match config.get_raw("overlayActivityFilters") {
        Ok(Some(raw)) => match serde_json::from_str::<Value>(&raw) {
            Ok(value) if OverlayActivityFilters::has_persisted_rules(&value) => {
                OverlayActivityFilters::from_json(value)
            }
            Ok(_) => OverlayActivityFilters::default(),
            Err(error) => {
                tracing::warn!("failed to parse overlay activity filters: {error}");
                OverlayActivityFilters::default()
            }
        },
        Ok(None) => OverlayActivityFilters::default(),
        Err(error) => {
            tracing::warn!("failed to load overlay activity filters: {error}");
            OverlayActivityFilters::default()
        }
    };
    if let Some(desktop) = load_types_key_surface(config, "desktopNotificationActivityFilters") {
        filters.desktop = desktop;
    }
    if let Some(vr) = load_types_key_surface(config, "vrNotificationActivityFilters") {
        filters.vr = vr;
    }
    if let Some(hmd) = load_types_key_surface(config, "hmdNotificationActivityFilters") {
        filters.hmd = hmd;
    }
    if let Some(webhook) = load_types_key_surface(config, "webhookActivityFilters") {
        filters.webhook = webhook;
    }
    if let Some(tts) = load_types_key_surface(config, "ttsNotificationActivityFilters") {
        filters.tts = tts;
    } else {
        filters.tts = seed_tts_notification_activity_filters(config, &filters);
    }
    filters
}

pub fn load_location_hidden_user_ids(config: &dyn NotificationConfig) -> HashSet<String> {
    if !config
        .get_bool("feedHiddenUsersHideNotifications", true)
        .unwrap_or(true)
    {
        return HashSet::new();
    }
    let Some(raw) = config.get_raw("feedHiddenUsers").ok().flatten() else {
        return HashSet::new();
    };
    let Ok(Value::Array(entries)) = serde_json::from_str::<Value>(&raw) else {
        return HashSet::new();
    };
    entries
        .iter()
        .filter_map(|entry| match entry {
            Value::String(user_id) => Some(user_id.as_str()),
            Value::Object(fields) => fields.get("userId").and_then(Value::as_str),
            _ => None,
        })
        .map(str::trim)
        .filter(|user_id| !user_id.is_empty())
        .map(str::to_string)
        .collect()
}

fn seed_tts_notification_activity_filters(
    config: &dyn NotificationConfig,
    filters: &OverlayActivityFilters,
) -> OverlayActivitySurfaceFilters {
    let mut seeded = filters.desktop.clone();
    let activity_types = filters
        .desktop
        .types
        .keys()
        .chain(filters.vr.types.keys())
        .collect::<BTreeSet<_>>();
    for activity_type in activity_types {
        let desktop_rule = filters.rule_for(OverlayActivitySurface::Desktop, activity_type);
        if desktop_rule.scope == OverlayActivityScope::Off {
            let vr_rule = filters.rule_for(OverlayActivitySurface::Vr, activity_type);
            if vr_rule.scope != OverlayActivityScope::Off {
                seeded.types.insert(activity_type.clone(), vr_rule);
            }
        } else {
            seeded.types.insert(activity_type.clone(), desktop_rule);
        }
    }
    if let Ok(value) = serde_json::to_value(&seeded) {
        if let Err(error) = config.set_json("ttsNotificationActivityFilters", &value) {
            tracing::warn!("failed to persist seeded TTS activity filters: {error}");
        }
    }
    seeded
}

fn load_types_key_surface(
    config: &dyn NotificationConfig,
    key: &str,
) -> Option<OverlayActivitySurfaceFilters> {
    let raw = config.get_raw(key).ok().flatten()?;
    let value = serde_json::from_str::<Value>(&raw).ok()?;
    value
        .get("types")
        .is_some_and(Value::is_object)
        .then(|| OverlayActivitySurfaceFilters::from_types_json(&value))
}

#[cfg(test)]
mod tests;
