//! Key/value settings, the protection plan, and snapshot history listing - thin pass-throughs to [`crate::persistence::Db`].
#![allow(
    clippy::unreachable,
    reason = "tauri::command injects unreachable code into async wrappers"
)]

use tauri::{AppHandle, Manager, State};

use crate::persistence::{Db, SnapshotSummary};
use crate::services::protection::{GuidanceInventory, ProtectionPlan, ProtectionState};
use crate::command_error::CmdError;
use crate::services::wfm_session::WfmSession;
use std::sync::Arc;

#[tauri::command]
pub async fn protection_state(
    app: AppHandle,
    session: State<'_, Arc<WfmSession>>,
    inventory: GuidanceInventory,
) -> Result<ProtectionState, CmdError> {
    let session = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let market = crate::services::sellables::MarketData::load(
            &app.state::<crate::services::market::MarketCache>(),
        );
        let orders = crate::services::protection::validate_snapshot(
            &app.state::<Db>(),
            inventory.snapshot_id,
        )
        .and_then(|()| {
            session
                .require_unlocked()
                .map_err(|_| "Unlock WFM to account for your current listings.".to_string())
        })
        .and_then(|unlocked| {
            wfm_core::trading::listing::list_user_orders(&unlocked).map_err(|e| e.to_string())
        });
        crate::services::protection::guidance_state(&app.state::<Db>(), &market, orders, inventory)
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

/// Rows with a typed owner are neither readable nor writable through the
/// generic commands: a write would bypass that owner's consent or validation,
/// and the price-report key would link an install's weekly pseudonyms.
fn reserved_for_typed_commands(key: &str) -> Result<(), String> {
    if key.starts_with("usage.") {
        return Err("Usage state is private to its typed commands.".into());
    }
    if key.starts_with("reports.") {
        return Err("Price report state is private to its typed commands.".into());
    }
    if key.starts_with("update-notes.") {
        return Err("Update history is managed by its own commands.".into());
    }
    Ok(())
}

#[tauri::command]
pub fn get_setting(db: State<'_, Db>, key: String) -> Result<Option<String>, String> {
    reserved_for_typed_commands(&key)?;
    db.get_setting(&key).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_setting(db: State<'_, Db>, key: String, value: String) -> Result<(), String> {
    reserved_for_typed_commands(&key)?;
    db.set_setting(&key, &value).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_snapshots(db: State<'_, Db>, limit: i64) -> Result<Vec<SnapshotSummary>, String> {
    db.list_snapshots(limit).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::reserved_for_typed_commands;

    #[test]
    fn typed_settings_are_unreachable_through_the_generic_commands() {
        for key in [
            "usage.enabled",
            "reports.key-v1",
            "reports.sales-consent-v1",
            "reports.sales-mark-v1",
            "reports.sent-v1",
            "update-notes.history-v1",
        ] {
            assert!(reserved_for_typed_commands(key).is_err(), "{key}");
        }
        for key in ["theme.mode", "prompts", "auto-close-sold", "reportsx"] {
            assert!(reserved_for_typed_commands(key).is_ok(), "{key}");
        }
    }
}
