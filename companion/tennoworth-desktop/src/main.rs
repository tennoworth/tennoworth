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
mod definitions;
mod eelog;
mod ws_watch;
mod eelog_state;
mod keyring_store;
mod market;
mod overlay;
#[cfg(target_os = "linux")]
mod wayland_overlay;
mod probe;
mod sellables;
mod snapshot;
mod trades;
mod tray;
mod update;
mod watch;
mod wfm_session;

use std::io::Write;
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::ShortcutState;

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
    // Before any network call: every WFM request identifies as this app +
    // version (WFM's rules require a descriptive User-Agent).
    wfm_core::set_app_identity("tennoworth-desktop", env!("CARGO_PKG_VERSION"));
    // No WebKit env shims for AppImage runs: the historical EGL abort on
    // rolling Mesa was root-caused (2026-08-20) to the bundle carrying
    // ubuntu's libwayland-* — fixed by stripping them at bundle time in
    // release-desktop.yml. A WEBKIT_DISABLE_DMABUF_RENDERER=1 shim was tried
    // here first and measurably did NOT help (same abort with it set), so it
    // does not ship.
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
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_global_shortcut::Builder::new()
            .with_handler(|app, _shortcut, event| {
                if event.state() == ShortcutState::Pressed {
                    let _ = overlay::trigger_capture(app, "hotkey");
                }
            })
            .build())
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
            commands::settings::list_listing_log,
            commands::market::cached_market,
            commands::market::refresh_market,
            commands::market::top_sellables,
            commands::market::live_top_prices,
            commands::market::riven_comps,
            commands::market::cached_history,
            commands::market::refresh_history,
            commands::watch::list_watches,
            commands::watch::add_watch,
            commands::watch::delete_watch,
            commands::watch::check_watches_now,
            commands::trades::list_trades,
            commands::trades::eelog_status,
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
            commands::report::report_scan_issue,
            commands::report::open_external_url,
            overlay::get_overlay_settings,
            overlay::update_overlay_settings,
            overlay::overlay_status,
            overlay::current_overlay_result,
            overlay::preview_relic_overlay,
            overlay::setup_overlay_capture,
            overlay::scan_overlay_now,
            overlay::open_overlay_diagnostics,
            overlay::clear_overlay_diagnostics,
            overlay::ocr_boot_probe,
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

            let overlay_state =
                overlay::OverlayState::new(&app.handle().clone(), &app.state::<Db>());
            app.manage(overlay_state);
            overlay::register_configured_shortcut(&app.handle().clone());

            if std::env::var_os("TENNOWORTH_OCR_BOOT_PROBE").is_some() {
                match overlay::ocr_boot_probe(app.state::<overlay::OverlayState>()) {
                    Ok(()) => {
                        let evidence = format!(
                            "OCR_BOOT_PROBE_OK backend={}\n",
                            overlay::capture_backend_name()
                        );
                        if let Some(path) =
                            std::env::var_os("TENNOWORTH_OCR_BOOT_PROBE_OUT")
                        {
                            std::fs::write(path, &evidence)?;
                        }
                        print!("{evidence}");
                        // Let Tauri unwind its Windows plugins and the OCR worker
                        // cleanly. A hard process exit from inside setup trips a
                        // Windows fast-fail (0xc0000409) in the installed build.
                        app.handle().exit(0);
                        return Ok(());
                    }
                    Err(error) => return Err(error.into()),
                }
            }

            // Probe boot mode: verify the Rust boot path - DB, market cache,
            // definitions fetch - then exit BEFORE the webview exists. The
            // GitHub runner's non-interactive session cannot initialize
            // WebView2 (the full probe hangs there on every run), so the
            // runner gate (ui-smoke.yml) verifies everything up to the window
            // this way, and the interactive probe runs locally via
            // scripts/probe-smoke-linux.sh.
            let boot_probe = probe && std::env::var_os("TENNOWORTH_PROBE_BOOT").is_some();

            // The C4 market cache lives next to the DB in the same app-data dir.
            // Unlike the DB, a missing/unreadable cache is never fatal — the
            // bundled snapshot is the floor — so this can't fail startup.
            // C7: scan definitions live in the same dir. Kicked off before the
            // window exists and never awaited — a scan that beats it simply uses
            // the compiled-in patterns, which is the correct fallback and not
            // worth delaying startup for. reqwest::blocking must not run on an
            // async worker, hence spawn_blocking rather than spawn.
            if !boot_probe {
                let defs_dir = data_dir.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let out = definitions::refresh_and_install(&defs_dir);
                    if out.installed {
                        eprintln!(
                            "tennoworth: scan definitions installed (fetched={}, rejected={})",
                            out.fetched,
                            out.rejected.len()
                        );
                    }
                });
            }

            if boot_probe {
                let out = definitions::refresh_and_install(&data_dir);
                println!(
                    "PROBE_BOOT_OK defs_installed={} defs_fetched={} defs_rejected={}",
                    out.installed, out.fetched, out.rejected.len()
                );
                std::process::exit(0);
            }

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
                // CI runs the app under the probe in a non-interactive session
                // where Chromium's GPU process can hang WebView2 init (ui-smoke
                // hung there on every runner run). The probe is the only caller
                // that wants this - shipping it to real users would trade a
                // rare hang for degraded rendering.
                b = b.additional_browser_args("--disable-gpu --no-first-run --disable-extensions");
            }
            let w = b.build()?;
            overlay::prewarm_overlay_window(&app.handle().clone());

            // Desktop window lifecycle: closing the window HIDES it to the tray
            // instead of quitting — only the tray's "Quit" (app.exit) actually
            // exits. Single-instance is assumed, so re-showing is "Open". The
            // first close-with-tray emits a hint so the SPA can show its once-
            // ever "still running in the tray" banner; the AtomicBool caps that
            // to once per session.
            let hint_sent = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let w_for_close = w.clone();
            let app_for_hint = app.handle().clone();
            w.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = w_for_close.hide();
                    if !hint_sent.swap(true, Ordering::Relaxed)
                        && app_for_hint.tray_by_id("main").is_some()
                    {
                        let _ = app_for_hint.emit(crate::tray::EVENT_TRAY_HINT, ());
                    }
                }
            });

            // Tray is best-effort (Linux is de-scoped to best-effort behind
            // libayatana; a forced-failure hook exists for testing). A failure
            // is logged and swallowed — window + notifications carry on.
            if let Err(e) = tray::init_tray(&app.handle().clone()) {
                eprintln!("tennoworth: tray unavailable, continuing without it: {e}");
            }

            // EE.log tailer: trade detection → ledger + notification +
            // auto-close of the sold WFM listing (see trades.rs). Read-only on
            // the game's own log. Not in probe runs.
            let ee_path = if probe { None } else { trades::start_tailer(app.handle().clone()) };
            match &ee_path {
                Some(p) => eprintln!("tennoworth: tailing EE.log at {}", p.display()),
                None => eprintln!("tennoworth: EE.log not found — trade detection off (set TENNOWORTH_EELOG to override)"),
            }
            app.manage(eelog_state::EeLogState { path: ee_path });

            // Price-watch checker: a background pass every CHECK_INTERVAL
            // over the user's watches (see watch.rs). Not in probe runs —
            // the probe must not make WFM calls on a timer.
            if !probe {
                watch::start_checker(app.handle().clone());
                // Fast path beside the poll: WFM's live order stream fires a
                // matching watch in seconds (see ws_watch.rs).
                ws_watch::start_stream(app.handle().clone());
            }

            // C5: launch update check, off the main thread so it can never
            // block startup. NO silent install — a found update only stores
            // status + emits `update-available`; the SPA asks the user, and
            // only their explicit confirmation invokes install_update. Any
            // failure inside check() (offline, bad manifest) already reads as
            // "no update", so this task cannot take the app down.
            let update_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                update::check_and_emit(&update_handle).await;
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
