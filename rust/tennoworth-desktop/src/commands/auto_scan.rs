//! Automatic-scan settings and status over IPC. Thin adapters over
//! [`crate::services::auto_scan`], which owns the loop and the stored value.

use tauri::State;

use crate::persistence::Db;
use crate::services::auto_scan::{self, AutoScanSettings, AutoScanState, AutoScanStatus};

#[tauri::command]
pub fn get_auto_scan_settings(db: State<'_, Db>) -> AutoScanSettings {
    auto_scan::load_settings(&db)
}

/// Persist the settings, then hand the sanitized result to the running loop.
/// The loop re-reads them every tick, so a change takes effect within one tick
/// without any wake-up plumbing.
#[tauri::command]
pub fn update_auto_scan_settings(
    db: State<'_, Db>,
    state: State<'_, AutoScanState>,
    settings: AutoScanSettings,
) -> Result<AutoScanSettings, String> {
    let saved = auto_scan::save_settings(&db, settings)?;
    state.set_settings(saved);
    Ok(saved)
}

#[tauri::command]
pub fn auto_scan_status(state: State<'_, AutoScanState>) -> AutoScanStatus {
    state.status()
}

/// Suspend or resume automatic scanning. The webview holds it for as long as an
/// interactive listing flow is open: a background scan records a new snapshot,
/// and a listing submit is rejected unless it names the latest one.
#[tauri::command]
pub fn set_auto_scan_hold(state: State<'_, AutoScanState>, hold: bool) {
    state.set_held(hold);
}
