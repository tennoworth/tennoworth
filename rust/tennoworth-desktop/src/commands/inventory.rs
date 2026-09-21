//! Inventory acquisition: memory scan (the app's only UI path) landing in the
//! snapshot history via [`record_snapshot`], plus an `import_snapshot` command
//! the probe uses to seed history without a running game.
#![allow(
    clippy::unreachable,
    reason = "tauri::command injects unreachable code into async wrappers"
)]

use tauri::{AppHandle, Emitter, Manager, State};

use crate::persistence::Db;
use crate::shell::tray::post_scan_surfaces;

/// Emitted with [`ScannedInventory`] after a scan the app started on its own -
/// the automatic scanner, or the tray's Rescan. The webview's own
/// `scan_inventory` call already receives the same payload as its response, so
/// it emits nothing.
pub const EVENT_INVENTORY_SCANNED: &str = "inventory-scanned";

/// Single-flight guard for the whole scan-and-record boundary: a second
/// concurrent scan - a tick landing on a user's click - must not walk the game's
/// address space twice, and must not record a snapshot ahead of the first one's
/// accounting.
static SCAN_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether a scan already owns the scanner. The automatic scanner asks before it
/// spends an attempt, so a collision with a user-clicked scan is a silent skip
/// rather than a failed attempt against the user's cadence.
pub(crate) fn scan_in_progress() -> bool {
    SCAN_ACTIVE.load(std::sync::atomic::Ordering::SeqCst)
}

fn scan_boundary(app: &AppHandle) -> Option<crate::services::eelog::LogPosition> {
    let state = app.try_state::<crate::services::eelog_state::EeLogState>()?;
    crate::services::eelog::log_position(state.path.as_deref()?)
}

fn record_game_scan(
    app: &AppHandle,
    bytes: &[u8],
    info: &wfm_core::acquisition::scan::SessionInfo,
    before: Option<crate::services::eelog::LogPosition>,
    started_at: i64,
) -> Option<i64> {
    let after = scan_boundary(app);
    let observed_at = crate::services::allowance::unix_now();
    let db = app.state::<Db>();
    let recorded = (|| -> Result<i64, String> {
        let id = record_snapshot(&db, "memory", info.build.as_deref(), bytes)?;
        let raw = serde_json::from_slice(bytes).map_err(|e| format!("read scan metadata: {e}"))?;
        let account_key = wfm_core::identity::local_fingerprint(
            "tennoworth-account-v1",
            info.account_id.to_lowercase().as_bytes(),
        );
        let observation = crate::services::allowance::Observation::scanned(
            account_key,
            id,
            &raw,
            before,
            after,
            started_at,
            observed_at,
        );
        db.save_allowance(observation)
            .map_err(|e| format!("save allowance: {e}"))?;
        Ok(id)
    })();
    let snapshot_id = recorded.as_ref().ok().copied();
    if let Err(error) = recorded {
        // A successful scan may be a different account. Never retain the
        // previous account's allowance when recording the new one fails.
        let _ = db.clear_allowance();
        eprintln!("tennoworth: scan observation not recorded: {error}");
    }
    let _ = app.emit(crate::services::allowance::EVENT_ALLOWANCE_CHANGED, ());
    snapshot_id
}

#[derive(Clone, serde::Serialize)]
pub struct ScannedInventory {
    pub(crate) inventory: String,
    pub(crate) snapshot_id: Option<i64>,
}

/// Hand a completed background scan to the webview so the open app can adopt it
/// or offer it, instead of showing an inventory a cadence out of date.
pub(crate) fn publish_scan(app: &AppHandle, payload: &ScannedInventory) {
    let _ = app.emit(EVENT_INVENTORY_SCANNED, payload);
}

pub(crate) fn scan_and_record(app: &AppHandle) -> Result<ScannedInventory, String> {
    // Keep acquisition and its accounting boundary under the same single-flight
    // guard; a tray scan must not persist newer data before this scan is recorded.
    let _guard = wfm_core::trading::plan::PlanGuard::acquire(&SCAN_ACTIVE)
        .ok_or("An inventory scan is already running.")?;
    let started_at = crate::services::allowance::unix_now();
    let before = scan_boundary(app);
    let (bytes, info) = crate::services::inventory::scanner()
        .scan(None, None)
        .map_err(|e| e.into_message())?;
    let snapshot_id = record_game_scan(app, &bytes, &info, before, started_at);
    Ok(ScannedInventory { inventory: String::from_utf8(bytes).map_err(|_| "Inventory response was not valid UTF-8.".to_string())?, snapshot_id })
}

/// Extract snapshot rows from raw inventory bytes and append them to history as
/// one transactional snapshot. Shared by the memory scan and the probe's
/// import_snapshot seeding. Returns the new snapshot id.
pub(crate) fn record_snapshot(
    db: &Db,
    source: &str,
    game_version: Option<&str>,
    bytes: &[u8],
) -> Result<i64, String> {
    let items = crate::persistence::snapshot::extract_items(bytes)
        .map_err(|e| format!("parse inventory for snapshot: {e}"))?;
    db.insert_snapshot(source, None, game_version, &items)
        .map_err(|e| format!("insert snapshot: {e}"))
}

/// Return inventory JSON together with the identity recorded under the scan guard.
/// Async + spawn_blocking
/// so the (potentially slow) scan never blocks the webview event loop. A busy
/// guard or a missing/unscannable game becomes a rejected invoke carrying
/// wfm-core's graceful, actionable message (e.g. "Warframe doesn't appear to be
/// running…") - the SPA surfaces it verbatim in its error banner.
///
/// On success it also appends a `source='memory'` history snapshot. That insert
/// is best-effort: a failure is logged to stderr and swallowed - losing a
/// history row must never cost the user their scan (scan value > history value).
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

#[cfg(test)]
mod tests {
    use super::{scan_in_progress, ScannedInventory, SCAN_ACTIVE};

    #[test]
    fn scan_response_matches_the_frontend_transport_fixture() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!("../../../../tests/fixtures/protection/scan-response.json")).unwrap();
        let result = ScannedInventory { inventory: r#"{"Suits":[{"a":1}]}"#.into(), snapshot_id: Some(7) };
        assert_eq!(serde_json::to_value(result).unwrap(), fixture);
    }

    /// The background scanner decides `Scan` and then asks
    /// [`scan_in_progress`] before spending an attempt. If that query watched a
    /// different flag than [`scan_and_record`] claims, a tick colliding with a
    /// user-clicked scan would spend the cadence on a scan it never started.
    #[test]
    fn the_scheduler_sees_the_same_single_flight_flag_the_scan_takes() {
        assert!(!scan_in_progress());
        let held = wfm_core::trading::plan::PlanGuard::acquire(&SCAN_ACTIVE)
            .expect("no other test holds the scan flag");
        assert!(
            scan_in_progress(),
            "a held scan must be visible to the scheduler"
        );
        drop(held);
        assert!(!scan_in_progress());
    }
}
