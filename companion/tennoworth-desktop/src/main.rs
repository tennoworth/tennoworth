// TennoWorth desktop shell (Tauri v2). The webview loads the built SPA
// (prototype/dist) over Tauri's asset protocol; the SPA's Transport picks the
// Tauri path at boot and drives wfm-core through the commands in `commands/`.
//
// Commands are deliberately thin adapters over wfm-core (the only adapter —
// the standalone CLI was removed with the CLI release stream), grouped by
// domain:
//   - `health` (below)   → version / platform info (the IPC liveness round-trip)
//   - `commands::inventory` → single-flight memory scan
//   - `commands::settings`  → key/value settings + reserve-copy CRUD
//   - `commands::market`    → market cache/refresh + sellables ranking
//   - `wfm_session`      → `wfm_auth_status` / `wfm_login` / `unlock_jwt` /
//                        `wfm_logout` (the passphrase arrives from the
//                        webview, never a TTY)
//   - `commands::listing`   → `submit_plan` / `get_pending_plan` / `resume_pending_plan`
//                        / `discard_pending_plan` / `fetch_orders` / `update_order`
//                        / `delete_order` / `bulk_visibility` — the desktop
//                        listing/order surface, same wfm-core services
//   - `commands::assistant` → `ask_assistant`, the DeepSeek relay (dormant —
//                        no UI surfaces it; the key stays in Rust, off the
//                        webview)
//   - `update`           → `check_update` / `update_status` / `install_update`
//                        / `restart_app` (C5 auto-update)
//   - `tray`             → system tray + the post-scan notification
//   - `probe`            → the TENNOWORTH_PROBE=1 verification probe
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod db;
mod keyring_store;
mod market;
mod probe;
mod sellables;
mod snapshot;
mod tray;
mod update;
mod wfm_session;

use std::io::Write;
use std::sync::{Arc, OnceLock};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use db::Db;
use market::MarketCache;
use wfm_core::inventory::InventoryScanner;
use wfm_session::WfmSession;

/// Process-wide scanner so the single-flight guard actually serializes two
/// concurrent `scan_inventory` invokes (a second concurrent scan gets
/// ScanError::Busy rather than a redundant parallel walk of the address space).
pub(crate) fn scanner() -> &'static InventoryScanner {
    static SCANNER: OnceLock<InventoryScanner> = OnceLock::new();
    SCANNER.get_or_init(InventoryScanner::new)
}

#[derive(serde::Serialize)]
struct Health {
    ok: bool,
    /// OS family the shell was built for (`linux` / `windows` / `macos`).
    platform: String,
    app_version: String,
    core_version: String,
}

/// IPC liveness + build info. Proves the SPA can reach a live wfm-core over the
/// Tauri boundary.
#[tauri::command]
fn health() -> Health {
    Health {
        ok: true,
        platform: std::env::consts::OS.to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        core_version: wfm_core::version().to_string(),
    }
}

fn main() {
    let probe = std::env::var("TENNOWORTH_PROBE").ok().as_deref() == Some("1");
    let runtag = std::env::var("TENNOWORTH_RUNTAG").unwrap_or_else(|_| "na".into());

    tauri::Builder::default()
        // MUST be registered first — the plugin has to claim the single-instance
        // lock before anything else initialises, or a second launch does real
        // work (opening the DB, building a webview) before being told to quit.
        //
        // Closing the window hides to tray rather than exiting, so a relaunch
        // would otherwise start a rival process against the same SQLite file.
        // Re-show the window we already have instead.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            tray::show_main_window(app);
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(tray::TrayState::default())
        .manage(update::UpdateState::default())
        .manage(Arc::new(WfmSession::new()))
        .invoke_handler(tauri::generate_handler![
            health,
            commands::inventory::scan_inventory,
            commands::inventory::import_snapshot,
            commands::settings::get_setting,
            commands::settings::set_setting,
            commands::settings::get_reserves,
            commands::settings::set_reserve,
            commands::settings::delete_reserve,
            commands::settings::list_snapshots,
            commands::market::cached_market,
            commands::market::refresh_market,
            commands::market::top_sellables,
            commands::market::tray_state,
            update::check_update,
            update::update_status,
            update::install_update,
            update::restart_app,
            wfm_session::wfm_auth_status,
            wfm_session::wfm_login,
            wfm_session::unlock_jwt,
            wfm_session::try_silent_unlock,
            wfm_session::wfm_logout,
            commands::listing::submit_plan,
            commands::listing::get_pending_plan,
            commands::listing::discard_pending_plan,
            commands::listing::resume_pending_plan,
            commands::listing::fetch_orders,
            commands::listing::update_order,
            commands::listing::delete_order,
            commands::listing::bulk_visibility,
            commands::assistant::ask_assistant,
            probe::debug_write_login,
            probe::debug_seed_unlocked,
            probe::debug_post_scan,
            probe::probe_report,
            probe::probe_exit
        ])
        .setup(move |app| {
            // Open the canonical SQLite store in the platform app-data dir and
            // hand it to the command layer as managed state. A failure here is
            // unrecoverable (the store is canonical) — abort startup with a
            // clear message rather than run with silent, ephemeral state.
            let data_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("resolving app data dir: {e}"))?;
            std::fs::create_dir_all(&data_dir)
                .map_err(|e| format!("creating app data dir {}: {e}", data_dir.display()))?;
            let db_path = data_dir.join("tennoworth.db");
            let store = Db::open(&db_path)
                .map_err(|e| format!("opening state DB {}: {e}", db_path.display()))?;
            app.manage(store);

            // The C4 market cache lives next to the DB in the same app-data dir.
            // Unlike the DB, a missing/unreadable cache is never fatal — the
            // bundled snapshot is the floor — so this can't fail startup.
            app.manage(MarketCache::new(data_dir));

            let mut b = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                .title("TennoWorth")
                .inner_size(1200.0, 800.0);
            // Without an explicit icon the window (and its taskbar/switcher
            // entry) falls back to a generic WM avatar — tray.rs already
            // pulls the same compiled-in icon via default_window_icon().
            if let Some(icon) = app.default_window_icon() {
                b = b.icon(icon.clone())?;
            }
            if probe {
                b = b.initialization_script(probe::build_probe_script(&runtag));
            }
            let w = b.build()?;

            // Desktop window lifecycle: closing the window HIDES it to the tray
            // instead of quitting — only the tray's "Quit" (app.exit) actually
            // exits. Single-instance is assumed, so re-showing is "Open".
            let w_for_close = w.clone();
            w.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = w_for_close.hide();
                }
            });

            // Tray is best-effort (Linux is de-scoped to best-effort behind
            // libayatana; a forced-failure hook exists for testing). A failure
            // is logged and swallowed — window + notifications carry on.
            if let Err(e) = tray::init_tray(&app.handle().clone()) {
                eprintln!("tennoworth: tray unavailable, continuing without it: {e}");
            }

            // C5: launch update check, off the main thread so it can never
            // block startup. NO silent install — a found update only stores
            // status + emits `update-available`; the SPA asks the user, and
            // only their explicit confirmation invokes install_update. Any
            // failure inside check() (offline, bad manifest) already reads as
            // "no update", so this task cannot take the app down.
            let update_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let status = update::check(&update_handle).await;
                if status.available {
                    if let Err(e) = update_handle.emit(update::EVENT_UPDATE_AVAILABLE, &status) {
                        eprintln!("tennoworth: update-available emit failed: {e}");
                    }
                }
            });

            if probe {
                let mut so = std::io::stdout();
                match w.url() {
                    Ok(u) => {
                        let _ = writeln!(so, "PROBE_WEBVIEW_URL {u}");
                    }
                    Err(e) => {
                        let _ = writeln!(so, "PROBE_WEBVIEW_URL_ERR {e}");
                    }
                }
                let _ = writeln!(so, "PROBE_ENABLED true");
                let _ = so.flush();
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
