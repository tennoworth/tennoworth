//! Market snapshot cache access + refresh, and the sellables ranking the SPA
//! reads directly (the tray reads the same ranking via [`crate::tray`]).

use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use wfm_core::live_top::{fetch_live_tops, LiveTop, LiveTopQuery};
use wfm_core::poison::guard;

use crate::db::Db;
use crate::market::{self, MarketCache, RefreshResult};
use crate::sellables::{self, SellableRow};
use crate::tray::{rebuild_tray, TrayState};
use crate::wfm_session::{CmdError, WfmSession};

/// The tray labels the last rebuild pushed + the last notification payload —
/// evidence surface for the probe (the GTK menu isn't screenshot-able headless)
/// and the backing for a later in-window "last scan" recap.
#[derive(serde::Serialize)]
pub struct TrayStateReport {
    labels: Vec<String>,
    last_notification: Option<sellables::ScanNotification>,
}

/// The app-data-cached market snapshot, or null on a first run / unreadable
/// cache. No network: the SPA reads this at boot to prefer the cache (last
/// known-good from the live server) over the compile-time bundled floor.
#[tauri::command]
pub fn cached_market(cache: State<'_, MarketCache>) -> Option<String> {
    cache.cached()
}

/// Conditionally refresh the market snapshot from tennoworth.app (ETag /
/// If-None-Match), updating the app-data cache. Async + spawn_blocking so the
/// (network) call never blocks the webview event loop, mirroring scan_inventory
/// (reqwest::blocking must not run on an async worker thread). Every network /
/// HTTP / body failure is swallowed inside `market::refresh` and returns a
/// no-op RefreshResult — the only Err here is the blocking task failing to run.
#[tauri::command]
pub async fn refresh_market(app: AppHandle, cache: State<'_, MarketCache>) -> Result<RefreshResult, String> {
    let dir = cache.dir();
    let result = tauri::async_runtime::spawn_blocking(move || market::refresh(&dir))
        .await
        .map_err(|e| format!("market refresh task failed to run: {e}"))?;
    // A fresh market snapshot can re-price the tray's sellables — rebuild it
    // (no notification; that's a scan-only surface). Only when the body changed.
    if result.updated {
        rebuild_tray(&app);
    }
    Ok(result)
}

#[tauri::command]
pub fn tray_state(state: State<'_, TrayState>) -> TrayStateReport {
    TrayStateReport {
        labels: guard(&state.labels).clone(),
        last_notification: *guard(&state.last_notification),
    }
}

/// Rank the latest snapshot × market by the shared sell-priority score and
/// return the top `limit` sellables. The single join both the tray menu and the
/// post-scan notification consume; also available to the SPA. Reads the freshest
/// market it holds (app-data cache, else the compile-time bundle).
#[tauri::command]
pub fn top_sellables(
    db: State<'_, Db>,
    cache: State<'_, MarketCache>,
    limit: usize,
) -> Vec<SellableRow> {
    let market = sellables::MarketData::load(&cache);
    let mut rows = sellables::rank_sellables(&db, &market);
    rows.truncate(limit);
    rows
}

/// Progress event for [`live_top_prices`]: `{ done, total }` after each item.
pub const EVENT_LIVE_TOP_PROGRESS: &str = "live-top-progress";

#[derive(serde::Serialize, Clone)]
struct LiveTopProgress {
    done: usize,
    total: usize,
}

/// Live top-of-book (≤5 best online asks/bids) for each query's exact tier,
/// straight from WFM v2 `/orders/item/{slug}/top`. Public endpoint — works
/// logged out; when a login is unlocked its platform is used so the prices
/// match the market the user actually lists on (else `pc`), and the user's
/// own orders are reported separately (`own_ask` / `own_bid`) instead of
/// being counted as competition. Paced at WFM's
/// 3 req/s ceiling, so 50 items ≈ 17 s: the SPA listens for
/// [`EVENT_LIVE_TOP_PROGRESS`] and shows a counter. Capped at 100 queries per
/// call — a whole-inventory sweep is the scraper's job, not the UI's.
#[tauri::command]
pub async fn live_top_prices(
    app: AppHandle,
    session: State<'_, Arc<WfmSession>>,
    queries: Vec<LiveTopQuery>,
) -> Result<Vec<LiveTop>, CmdError> {
    const MAX_QUERIES: usize = 100;
    if queries.len() > MAX_QUERIES {
        return Err(CmdError::of(
            "too_many",
            format!("{} items requested; the live-price check takes at most {MAX_QUERIES} at a time", queries.len()),
        ));
    }
    // Logged in: use the login's market and keep the user's own orders out of
    // the competition figures. Logged out: pc, everyone counts.
    let (platform, me) = session
        .require_unlocked()
        .map(|u| (u.platform.clone(), Some(u.username.clone())))
        .unwrap_or_else(|_| ("pc".to_string(), None));
    tauri::async_runtime::spawn_blocking(move || {
        fetch_live_tops(&platform, me.as_deref(), &queries, |done, total| {
            let _ = app.emit(EVENT_LIVE_TOP_PROGRESS, LiveTopProgress { done, total });
        })
        .map_err(CmdError::wfm)
    })
    .await
    .map_err(|e| CmdError::internal(format!("live price task failed to run: {e}")))?
}
