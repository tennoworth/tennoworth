//! Key/value settings and per-item reserve-copy CRUD, plus snapshot history
//! listing - thin pass-throughs to [`crate::persistence::Db`].
#![allow(clippy::unreachable, reason = "tauri::command injects unreachable code into async wrappers")]

use tauri::{AppHandle, Manager, State};

use crate::persistence::{Db, ListingLogEntry, Reserve, SnapshotSummary};
use crate::services::protection::{ProtectionPlan, ProtectionState};
use crate::services::wfm_session::{CmdError, WfmSession};
use std::sync::Arc;

#[tauri::command]
pub async fn protection_state(
    app: AppHandle,
    session: State<'_, Arc<WfmSession>>,
) -> Result<ProtectionState, CmdError> {
    let session = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let market = crate::services::sellables::MarketData::load(
            &app.state::<crate::services::market::MarketCache>(),
        );
        let orders = session
            .require_unlocked()
            .map_err(|_| "Unlock WFM to account for your current listings.".to_string())
            .and_then(|unlocked| {
                wfm_core::trading::listing::list_user_orders(&unlocked).map_err(|e| e.to_string())
            });
        crate::services::protection::state(&app.state::<Db>(), &market, orders)
            .map_err(CmdError::internal)
    })
    .await
    .map_err(|e| CmdError::internal(e.to_string()))?
}

#[tauri::command]
pub fn save_protection_plan(
    app: AppHandle,
    session: State<'_, Arc<WfmSession>>,
    plan: ProtectionPlan,
) -> Result<(), CmdError> {
    let _guard = session.begin_plan().ok_or_else(|| {
        CmdError::of(
            "busy",
            "Wait for the current listing operation before changing protection.",
        )
    })?;
    let market = crate::services::sellables::MarketData::load(
        &app.state::<crate::services::market::MarketCache>(),
    );
    plan.save(&app.state::<Db>(), &market)
        .map_err(CmdError::internal)?;
    crate::shell::tray::rebuild_tray(&app);
    Ok(())
}

#[tauri::command]
pub fn get_setting(db: State<'_, Db>, key: String) -> Result<Option<String>, String> {
    db.get_setting(&key).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_setting(db: State<'_, Db>, key: String, value: String) -> Result<(), String> {
    db.set_setting(&key, &value).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_reserves(db: State<'_, Db>) -> Result<Vec<Reserve>, String> {
    db.get_reserves().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_reserve(db: State<'_, Db>, slug: String, keep: i64) -> Result<(), String> {
    db.set_reserve(&slug, keep).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_reserve(db: State<'_, Db>, slug: String) -> Result<(), String> {
    db.delete_reserve(&slug).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_snapshots(db: State<'_, Db>, limit: i64) -> Result<Vec<SnapshotSummary>, String> {
    db.list_snapshots(limit).map_err(|e| e.to_string())
}

/// What we listed, when, at what price, and whether it worked - newest first.
/// Written by the listing commands; see `commands::listing::record_plan`.
#[tauri::command]
pub fn list_listing_log(db: State<'_, Db>, limit: i64) -> Result<Vec<ListingLogEntry>, String> {
    db.list_listing_log(limit).map_err(|e| e.to_string())
}
