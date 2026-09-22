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
#[derive(serde::Serialize)]
pub struct ScannedInventory {
    pub inventory: String,
    pub snapshot_id: Option<i64>,
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
    static ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let _guard = wfm_core::trading::plan::PlanGuard::acquire(&ACTIVE)
        .ok_or("An inventory scan is already running.")?;
    let started_at = crate::services::allowance::unix_now();
    let before = scan_boundary(app);
    let (bytes, info) = crate::services::inventory::scanner()
        .scan(None, None)
        .map_err(|e| e.into_message())?;
    let snapshot_id = record_game_scan(app, &bytes, &info, before, started_at);
    Ok(ScannedInventory {
        inventory: String::from_utf8(bytes)
            .map_err(|_| "Inventory response was not valid UTF-8.".to_string())?,
        snapshot_id,
    })
}

#[cfg(test)]
mod tests {
    use super::ScannedInventory;

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
}
