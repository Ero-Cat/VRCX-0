#![allow(non_snake_case)]

use tauri::State;

use crate::error::AppError;
use crate::state::AppState;

use vrcx_0_contracts::{
    SyncBootstrapProgress, SyncConnectionFields, SyncConnectionTestResult, SyncStatusSnapshot,
};
use vrcx_0_runtime_host_desktop::local_data::ConfigWriteEntry;

fn config_entry(key: &str, value: impl std::fmt::Display) -> ConfigWriteEntry {
    ConfigWriteEntry {
        key: key.to_string(),
        value: value.to_string(),
    }
}

/// Persist sync settings and (re)start the engine accordingly. Passing
/// `None` keeps a setting group unchanged; an empty password keeps the
/// stored one.
#[tauri::command(async)]
#[specta::specta]
pub async fn sync__configure(
    state: State<'_, AppState>,
    enabled: Option<bool>,
    connection: Option<vrcx_0_contracts::SyncConnectionInput>,
    intervalSec: Option<i64>,
) -> Result<SyncStatusSnapshot, AppError> {
    let mut entries = Vec::new();
    if let Some(enabled) = enabled {
        entries.push(config_entry("remoteSync.enabled", enabled));
    }
    if let Some(fields) = connection {
        entries.push(config_entry("remoteSync.host", fields.host.trim()));
        if (1..65536).contains(&fields.port) {
            entries.push(config_entry("remoteSync.port", fields.port));
        }
        entries.push(config_entry("remoteSync.user", fields.user.trim()));
        if let Some(password) = fields.password {
            if !password.trim().is_empty() {
                entries.push(config_entry("remoteSync.password", password.trim()));
            }
        }
        entries.push(config_entry("remoteSync.database", fields.database.trim()));
        entries.push(config_entry("remoteSync.tlsVerify", fields.tls_verify));
        entries.push(config_entry(
            "remoteSync.allowPlaintext",
            fields.allow_plaintext,
        ));
    }
    if let Some(interval) = intervalSec {
        entries.push(config_entry("remoteSync.intervalSec", interval.max(5)));
    }
    if !entries.is_empty() {
        state
            .runtime_host()
            .config_set_values(entries)
            .map_err(AppError::from)?;
    }
    let host = state.runtime_host().remote_sync();
    // Field edits auto-save quietly: only (re)start the engine when the
    // enable state changed or an engine is already running and must pick up
    // the new connection details.
    let engine_running = host.current().is_some();
    if enabled.is_some() || engine_running {
        host.restart_from_config().await.map_err(AppError::from)?;
    }
    Ok(host.status().await)
}

/// Stored connection fields without the password value.
#[tauri::command(async)]
#[specta::specta]
pub async fn sync__get_connection(
    state: State<'_, AppState>,
) -> Result<SyncConnectionFields, AppError> {
    Ok(state.runtime_host().remote_sync().connection_fields())
}

/// Probe the given connection details without persisting them.
#[tauri::command(async)]
#[specta::specta]
pub async fn sync__test_connection(
    state: State<'_, AppState>,
    connection: vrcx_0_contracts::SyncConnectionInput,
) -> Result<SyncConnectionTestResult, AppError> {
    let fields = connection;
    Ok(state
        .runtime_host()
        .remote_sync()
        .test_connection(
            &fields.host,
            fields.port,
            &fields.user,
            fields.password.as_deref(),
            &fields.database,
            fields.tls_verify,
            fields.allow_plaintext,
        )
        .await)
}

#[tauri::command(async)]
#[specta::specta]
pub async fn sync__status(state: State<'_, AppState>) -> Result<SyncStatusSnapshot, AppError> {
    Ok(state.runtime_host().remote_sync().status().await)
}

#[tauri::command(async)]
#[specta::specta]
pub async fn sync__trigger_now(state: State<'_, AppState>) -> Result<SyncStatusSnapshot, AppError> {
    Ok(state
        .runtime_host()
        .remote_sync()
        .run_cycle_now()
        .await
        .map_err(AppError::from)?)
}

#[tauri::command(async)]
#[specta::specta]
pub async fn sync__bootstrap_progress(
    state: State<'_, AppState>,
) -> Result<SyncBootstrapProgress, AppError> {
    Ok(state
        .runtime_host()
        .remote_sync()
        .current()
        .map(|engine| engine.bootstrap_progress())
        .unwrap_or_default())
}
