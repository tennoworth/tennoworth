//! Inventory acquisition: memory scan (the app's only UI path) landing in the
//! snapshot history via [`record_snapshot`], plus an `import_snapshot` command
//! the probe uses to seed history without a running game.

use tauri::{AppHandle, State};

use crate::persistence::Db;
use crate::services::acquisition::{record_snapshot, scan_and_record, ScannedInventory};
use crate::shell::tray::post_scan_surfaces;

#[tauri::command]
pub async fn scan_inventory(app: AppHandle) -> Result<ScannedInventory, String> {
    let scan_app = app.clone();
    let scan = tauri::async_runtime::spawn_blocking(move || scan_and_record(&scan_app))
        .await
        .map_err(|e| format!("scan task failed to run: {e}"))??;

    // C6: refresh the tray off the new snapshot and fire the post-scan
    // notification. Best-effort - never let a surface problem fail the scan
    // (the SPA still gets its inventory JSON below).
    post_scan_surfaces(&app);

    Ok(scan)
}

/// Seed an import snapshot as `source='import'` history. Probe-only now (the
/// UI file-drop it used to back is gone - the app scans from the game); the
/// probe calls it to exercise the record path and reach the sell view. Gated
/// behind TENNOWORTH_PROBE like every other probe-only command, so a stock
/// build can't reach it.
#[tauri::command]
pub fn import_snapshot(db: State<'_, Db>, inventory_json: String) -> Result<i64, String> {
    if std::env::var("TENNOWORTH_PROBE").ok().as_deref() != Some("1") {
        return Err("import_snapshot is probe-only".into());
    }
    record_snapshot(&db, "import", None, inventory_json.as_bytes())
}
