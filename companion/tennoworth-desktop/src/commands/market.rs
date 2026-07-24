//! Market snapshot cache access + refresh, and the sellables ranking the SPA
//! reads directly (the tray reads the same ranking via [`crate::tray`]).

use tauri::{AppHandle, State};

use crate::db::Db;
use crate::market::{self, MarketCache, RefreshResult};
use crate::sellables::{self, SellableRow};
use crate::tray::{rebuild_tray, TrayState};

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
        labels: state.labels.lock().unwrap().clone(),
        last_notification: *state.last_notification.lock().unwrap(),
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
