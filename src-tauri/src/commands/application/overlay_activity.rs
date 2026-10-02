#![allow(non_snake_case)]

use tauri::State;
use vrcx_0_application_activity::notification::{
    ActivityFilterProfile, NotificationActivityFilterProfiles, NotificationActivityFiltersSetInput,
};
use vrcx_0_application_activity::{activity_type_definitions, ActivityTypeDefinition};

use crate::error::AppError;
use crate::state::AppState;

#[tauri::command(async)]
#[specta::specta]
pub fn app__overlay_activity_definitions_get() -> Result<Vec<ActivityTypeDefinition>, AppError> {
    Ok(activity_type_definitions())
}

#[tauri::command(async)]
#[specta::specta]
pub fn app__notification_activity_filters_get(
    state: State<'_, AppState>,
) -> NotificationActivityFilterProfiles {
    state.runtime_host().notification_activity_filter_profiles()
}

#[tauri::command(async)]
#[specta::specta]
pub fn app__notification_test_send(state: State<'_, AppState>, message: String) {
    state.runtime_host().send_test_notification(&message);
}

#[tauri::command(async)]
#[specta::specta]
pub fn app__notification_activity_filters_set(
    state: State<'_, AppState>,
    input: NotificationActivityFiltersSetInput,
) -> Result<ActivityFilterProfile, AppError> {
    state
        .runtime_host()
        .set_notification_activity_filters(input)
        .map_err(AppError::from)
}
