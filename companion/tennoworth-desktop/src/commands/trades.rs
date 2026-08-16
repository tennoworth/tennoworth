//! Ledger + EE.log status commands.

use tauri::State;

use crate::db::{Db, TradeRow};
use crate::eelog_state::EeLogState;
use crate::wfm_session::CmdError;

#[tauri::command]
pub fn list_trades(db: State<'_, Db>, limit: Option<i64>) -> Result<Vec<TradeRow>, CmdError> {
    db.list_trades(limit.unwrap_or(200).clamp(1, 2000))
        .map_err(|e| CmdError::internal(format!("list trades: {e}")))
}

#[derive(serde::Serialize)]
pub struct EeLogStatus {
    /// The EE.log path being tailed, or null when the game's log could not be
    /// found (game never run on this machine, non-Steam install, custom
    /// prefix) — the SPA explains the `TENNOWORTH_EELOG` override.
    pub path: Option<String>,
    pub auto_close: bool,
}

#[tauri::command]
pub fn eelog_status(db: State<'_, Db>, ee: State<'_, EeLogState>) -> EeLogStatus {
    let auto_close = db
        .get_setting(crate::trades::SETTING_AUTO_CLOSE)
        .ok()
        .flatten()
        .map(|v| v != "off")
        .unwrap_or(true);
    EeLogStatus { path: ee.path.as_ref().map(|p| p.display().to_string()), auto_close }
}
