//! Trade presence over IPC. Thin adapters over [`crate::services::presence`],
//! which owns the status channel and the stored settings.
#![allow(
    clippy::unreachable,
    reason = "tauri::command injects unreachable code into async wrappers"
)]

use tauri::{AppHandle, Manager, State};

use crate::command_error::CmdError;
use crate::persistence::Db;
use crate::services::presence::{self, PresenceChoice, PresenceSettings, PresenceState, PresenceStatus};

#[tauri::command]
pub fn presence_status(state: State<'_, PresenceState>) -> PresenceStatus {
    state.status()
}

/// Pick a status. Waits for warframe.market's answer off the main thread.
#[tauri::command]
pub async fn set_presence(app: AppHandle, status: PresenceChoice) -> Result<PresenceStatus, CmdError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<PresenceState>();
        state.set(status)?;
        Ok(state.status())
    })
    .await
    .map_err(|e| CmdError::internal(e.to_string()))?
}

/// Persist the settings, then hand the sanitized result to the running loop.
#[tauri::command]
pub fn update_presence_settings(
    db: State<'_, Db>,
    state: State<'_, PresenceState>,
    settings: PresenceSettings,
) -> Result<PresenceSettings, CmdError> {
    let saved = presence::save_settings(&db, settings).map_err(CmdError::internal)?;
    state.settings_changed(saved);
    Ok(saved)
}

/// Resume following now instead of at the next game session.
#[tauri::command]
pub fn follow_game_now(state: State<'_, PresenceState>) {
    state.follow_now();
}
