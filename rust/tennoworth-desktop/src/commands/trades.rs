//! Ledger + EE.log status commands.

use tauri::{AppHandle, Manager, State};

use crate::persistence::{Db, TradeRow};
use crate::services::eelog_state::EeLogState;
use crate::services::wfm_session::CmdError;

#[tauri::command]
pub fn trade_session_state(
    app: AppHandle,
    db: State<'_, Db>,
) -> Result<TradeSessionState, CmdError> {
    let market = crate::services::sellables::MarketData::load(
        &app.state::<crate::services::market::MarketCache>(),
    );
    let allowance = db
        .trade_allowance(crate::services::allowance::unix_now())
        .map_err(|e| CmdError::internal(e.to_string()))?;
    let quantities = market.session_quantities(&db).map_err(CmdError::internal)?;
    let session = app.state::<std::sync::Arc<crate::services::wfm_session::WfmSession>>();
    let unlocked = session.require_unlocked().ok();
    let bulk_slugs = unlocked
        .as_ref()
        .map(|s| {
            s.catalog
                .iter()
                .filter(|(_, c)| c.bulk_tradable && c.session_supported)
                .map(|(slug, _)| slug.clone())
                .collect()
        })
        .unwrap_or_default();
    let set_recipes: std::collections::BTreeMap<_, _> = market.session_recipes().into_iter().filter(|(slug, _)| {
        unlocked.as_ref().and_then(|s| s.catalog.get(slug)).is_some_and(|c| c.session_supported && c.max_rank.is_none() && c.subtypes.is_empty())
    }).collect();
    let supported_slugs = unlocked.map(|s| {
        s.catalog
            .iter()
            .filter(|(slug, c)| c.session_supported || set_recipes.contains_key(*slug))
            .map(|(slug, _)| slug.clone())
            .collect()
    });
    Ok(TradeSessionState {
        allowance,
        quantities,
        bulk_slugs,
        supported_slugs,
        set_recipes,
    })
}

#[derive(serde::Serialize)]
pub struct TradeSessionState {
    pub set_recipes: std::collections::BTreeMap<String, std::collections::BTreeMap<String, u32>>,
    pub allowance: crate::services::allowance::AllowanceView,
    pub quantities: std::collections::BTreeMap<String, u32>,
    pub bulk_slugs: Vec<String>,
    pub supported_slugs: Option<Vec<String>>,
}

#[tauri::command]
pub fn list_trades(db: State<'_, Db>, limit: Option<i64>) -> Result<Vec<TradeRow>, CmdError> {
    db.list_trades(limit.unwrap_or(200).clamp(1, 2000))
        .map_err(|e| CmdError::internal(format!("list trades: {e}")))
}

#[derive(serde::Serialize)]
pub struct EeLogStatus {
    /// The EE.log path being tailed, or null when the game's log could not be
    /// found (game never run on this machine, non-Steam install, custom
    /// prefix) - the SPA explains the `TENNOWORTH_EELOG` override.
    pub path: Option<String>,
    pub auto_close: bool,
}

#[tauri::command]
pub fn eelog_status(db: State<'_, Db>, ee: State<'_, EeLogState>) -> EeLogStatus {
    let auto_close = db
        .get_setting(crate::services::trades::SETTING_AUTO_CLOSE)
        .ok()
        .flatten()
        .map(|v| v != "off")
        .unwrap_or(true);
    EeLogStatus {
        path: ee.path.as_ref().map(|p| p.display().to_string()),
        auto_close,
    }
}
