//! Inventory acquisition: the memory scan and the snapshot row it lands in.
//!
//! This is the acquisition capability's owner rather than a command helper,
//! because two entry points drive it: the `scan_inventory` command the SPA
//! calls, and the tray's Scan action. While it lived under `commands/`, the tray
//! had to reach into the IPC layer for it and the IPC layer had to reach back
//! into `shell/` to refresh surfaces afterwards, so the two layers imported each
//! other. Both now depend on this module, and it depends on neither.

use tauri::{AppHandle, Emitter, Manager};

use crate::persistence::Db;

/// The scan's result on the wire: the raw inventory JSON plus the history row it
/// was recorded as.
#[derive(Clone, serde::Serialize)]
pub struct ScannedInventory {
    pub inventory: String,
    pub snapshot_id: Option<i64>,
}

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

/// Hand a completed background scan to the webview so the open app can adopt it
/// or offer it, instead of showing an inventory a cadence out of date.
pub(crate) fn publish_scan(app: &AppHandle, payload: &ScannedInventory) {
    let _ = app.emit(EVENT_INVENTORY_SCANNED, payload);
}

/// The log position a scan started from, when a tailer is running. A scan is an
/// accounting boundary: the allowance logic needs to know which log region the
/// snapshot it is compared against actually covers.
fn scan_boundary(app: &AppHandle) -> Option<crate::services::eelog::LogPosition> {
    let state = app.try_state::<crate::services::eelog_state::EeLogState>()?;
    crate::services::eelog::log_position(state.path.as_deref()?)
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

/// Record the scan and the allowance observation it establishes, then report the
/// allowance change. The observation is what ties a remaining-trade count to the
/// snapshot and log region it was measured against, so failing to write it means
/// the previous account's figure must not survive.
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

/// Acquire an inventory and record it. Blocking; the caller runs it on a worker.
///
/// Acquisition and its accounting boundary sit under the same single-flight
/// guard, so a tray scan cannot persist newer data before an in-flight scan has
/// been recorded. Returns the scan's error text verbatim - wfm-core's messages
/// are written for the user ("Warframe doesn't appear to be running…") and the
/// SPA shows them unchanged.
pub(crate) fn scan_and_record(app: &AppHandle) -> Result<ScannedInventory, String> {
    scan_and_record_unless(app, || false)?
        .ok_or_else(|| "The scan was discarded before it was recorded.".to_string())
}

/// [`scan_and_record`], except that the finished walk is dropped unrecorded -
/// `Ok(None)` - when `discard` says so at the moment it would be recorded.
///
/// The automatic scanner needs this: a listing flow opened during the walk
/// submits against the latest snapshot, so recording one after it opened
/// would reject that submit. Checking only before the walk left the seconds a
/// walk takes uncovered.
pub(crate) fn scan_and_record_unless(
    app: &AppHandle,
    discard: impl FnOnce() -> bool,
) -> Result<Option<ScannedInventory>, String> {
    let _guard = wfm_core::trading::plan::PlanGuard::acquire(&SCAN_ACTIVE)
        .ok_or("An inventory scan is already running.")?;
    let started_at = crate::services::allowance::unix_now();
    let before = scan_boundary(app);
    let (bytes, info) = crate::services::inventory::scanner()
        .scan(None, None)
        .map_err(|e| e.into_message())?;
    if discard() {
        return Ok(None);
    }
    let snapshot_id = record_game_scan(app, &bytes, &info, before, started_at);
    Ok(Some(ScannedInventory {
        inventory: String::from_utf8(bytes)
            .map_err(|_| "Inventory response was not valid UTF-8.".to_string())?,
        snapshot_id,
    }))
}

#[cfg(test)]
mod tests {
    use super::{scan_in_progress, ScannedInventory, SCAN_ACTIVE};

    #[test]
    fn scan_response_matches_the_frontend_transport_fixture() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/protection/scan-response.json"
        ))
        .unwrap();
        let result = ScannedInventory {
            inventory: r#"{"Suits":[{"a":1}]}"#.into(),
            snapshot_id: Some(7),
        };
        assert_eq!(serde_json::to_value(result).unwrap(), fixture);
    }

    /// The background scanner decides `Scan` and then asks
    /// [`scan_in_progress`] before spending an attempt. If that query watched a
    /// different flag than [`super::scan_and_record`] claims, a tick colliding
    /// with a user-clicked scan would spend the cadence on a scan it never
    /// started.
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
