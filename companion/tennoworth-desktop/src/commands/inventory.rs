//! Inventory acquisition: memory scan and file-drop import, both landing in
//! the same snapshot history via [`record_snapshot`].

use tauri::{AppHandle, State};

use crate::db::Db;
use crate::tray::post_scan_surfaces;

/// Extract snapshot rows from raw inventory bytes and append them to history as
/// one transactional snapshot. Shared by the memory scan, the file-drop
/// import, and the tray's "Rescan". Returns the new snapshot id.
pub(crate) fn record_snapshot(
    db: &Db,
    source: &str,
    game_version: Option<&str>,
    bytes: &[u8],
) -> Result<i64, String> {
    let items = crate::snapshot::extract_items(bytes)
        .map_err(|e| format!("parse inventory for snapshot: {e}"))?;
    db.insert_snapshot(source, None, game_version, &items)
        .map_err(|e| format!("insert snapshot: {e}"))
}

/// Memory-scan the running game and return the inventory JSON as a string —
/// the exact bytes the CLI would write to inventory.json. Async + spawn_blocking
/// so the (potentially slow) scan never blocks the webview event loop. A busy
/// guard or a missing/unscannable game becomes a rejected invoke carrying
/// wfm-core's graceful, actionable message (e.g. "Warframe doesn't appear to be
/// running…") — the SPA surfaces it verbatim in its error banner.
///
/// On success it also appends a `source='memory'` history snapshot. That insert
/// is best-effort: a failure is logged to stderr and swallowed — losing a
/// history row must never cost the user their scan (scan value > history value).
#[tauri::command]
pub async fn scan_inventory(app: AppHandle, db: State<'_, Db>) -> Result<String, String> {
    let (bytes, info) = tauri::async_runtime::spawn_blocking(|| crate::scanner().scan(None, None))
        .await
        .map_err(|e| format!("scan task failed to run: {e}"))?
        .map_err(|e| e.into_message())?;

    if let Err(e) = record_snapshot(&db, "memory", info.build.as_deref(), &bytes) {
        eprintln!("tennoworth: inventory snapshot not recorded: {e}");
    }

    // C6: refresh the tray off the new snapshot and fire the post-scan
    // notification. Best-effort — never let a surface problem fail the scan
    // (the SPA still gets its inventory JSON below).
    post_scan_surfaces(&app);

    String::from_utf8(bytes).map_err(|e| format!("inventory response was not valid UTF-8: {e}"))
}

/// Record a dropped inventory.json as a `source='import'` history snapshot.
/// Used by the desktop file-drop path; unlike the scan path this surfaces the
/// error to the caller (the SPA logs it and moves on).
#[tauri::command]
pub fn import_snapshot(db: State<'_, Db>, inventory_json: String) -> Result<i64, String> {
    record_snapshot(&db, "import", None, inventory_json.as_bytes())
}
