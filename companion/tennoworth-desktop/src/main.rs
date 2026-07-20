// TennoWorth desktop shell (Tauri v2). The webview loads the built SPA
// (prototype/dist) over Tauri's asset protocol; the SPA's Transport picks the
// Tauri path at boot and drives wfm-core through these commands instead of the
// loopback HTTP companion.
//
// Commands are deliberately thin adapters over wfm-core (the CLI is the other
// adapter over the same crate):
//   - `health`         → version / platform info (the IPC liveness round-trip)
//   - `scan_inventory` → single-flight memory scan → inventory JSON bytes
//   - WFM session      → `wfm_auth_status` / `wfm_login` / `unlock_jwt` /
//                        `wfm_logout` (see wfm_session.rs — the passphrase
//                        arrives from the webview, never a TTY)
//   - listing/orders   → `submit_plan` / `get_pending_plan` / `resume_pending_plan`
//                        / `discard_pending_plan` / `fetch_orders` / `update_order`
//                        / `delete_order` / `bulk_visibility` — the desktop mirror
//                        of serve's HTTP routes, same wfm-core services
//   - `ask_assistant`  → the DeepSeek relay (key stays in Rust, off the webview)
//
// A verification probe is opt-in behind TENNOWORTH_PROBE=1: it injects a
// document-start script (PROBE_JS) that records origin / storage / fetch / IPC /
// CSP-violation behaviour, drives the real scan button, and exfiltrates the
// evidence via `probe_report` (→ file + stdout) before auto-exiting. Kept behind
// the env so the default run is a plain window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod db;
mod market;
mod sellables;
mod snapshot;
mod wfm_session;

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;
use tauri::menu::{Menu, MenuBuilder, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder, Wry};
use tauri_plugin_notification::NotificationExt;
use zeroize::Zeroizing;

use db::{Db, Reserve, SnapshotSummary};
use market::{MarketCache, RefreshResult};
use sellables::{MarketData, ScanNotification, SellableRow};
use wfm_core::assistant::{
    assistant_rate_limited, assistant_request_too_large, build_assistant_messages, call_deepseek,
    cap_history, deepseek_client, resolve_deepseek_key, short_reason, AssistantMessage,
    AssistantResponse,
};
use wfm_core::inventory::InventoryScanner;
use wfm_core::listing::{
    bulk_set_visibility, delete_order as core_delete_order, execute_plan as core_execute_plan,
    list_user_orders, run_pending, update_order as core_update_order, PerOrderResult, PlanItem,
    PlanRequest, PlanResponse, Unlocked, UpdateRequest, VisibilityRequest, MAX_PLATINUM,
};
use wfm_core::pending::{clear_pending, load_pending, PendingPlan};
use wfm_session::{CmdError, WfmSession};

/// How many sellables the tray menu shows.
const TRAY_LIMIT: usize = 5;

/// Process-wide scanner so the single-flight guard actually serializes two
/// concurrent `scan_inventory` invokes (a second concurrent scan gets
/// ScanError::Busy rather than a redundant parallel walk of the address space).
fn scanner() -> &'static InventoryScanner {
    static SCANNER: OnceLock<InventoryScanner> = OnceLock::new();
    SCANNER.get_or_init(InventoryScanner::new)
}

/// Evidence-facing view of what the tray/notification code last produced. The
/// GTK tray menu isn't reliably screenshot-able under headless Wayland, so the
/// probe reads the labels the rebuild actually pushed and the last notification
/// payload from here instead. Also the single home for the "last notification"
/// so a later window can surface it.
#[derive(Default)]
struct TrayState {
    /// The sellable labels ("Name — Np") the last rebuild put in the menu.
    labels: Mutex<Vec<String>>,
    last_notification: Mutex<Option<ScanNotification>>,
}

/// Rank the full latest-snapshot × market sell list (reads the Db + MarketCache
/// managed state off the handle). Shared by the tray rebuild and the
/// notification so they never disagree.
fn rank_all(app: &AppHandle) -> Vec<SellableRow> {
    let db = app.state::<Db>();
    let cache = app.state::<MarketCache>();
    let market = MarketData::load(&cache);
    sellables::rank_sellables(&db, &market)
}

/// Build the tray menu from the top sellables: one enabled item per sellable
/// ("Name — Np", id `sell:<slug>`), a separator, then Open / Rescan / Quit.
/// An empty list shows a single disabled hint instead.
fn build_tray_menu(app: &AppHandle, top: &[SellableRow]) -> tauri::Result<Menu<Wry>> {
    let mut mb = MenuBuilder::new(app);
    let mut sellable_items: Vec<MenuItem<Wry>> = Vec::new();
    if top.is_empty() {
        let hint = MenuItem::with_id(
            app,
            "noop",
            "No sellables yet — scan your inventory",
            false,
            None::<&str>,
        )?;
        sellable_items.push(hint);
    } else {
        for r in top {
            let label = format!("{} — {}p", r.name, r.price.round() as i64);
            let item =
                MenuItem::with_id(app, format!("sell:{}", r.slug), label, true, None::<&str>)?;
            sellable_items.push(item);
        }
    }
    for item in &sellable_items {
        mb = mb.item(item);
    }
    let open = MenuItem::with_id(app, "open", "Open TennoWorth", true, None::<&str>)?;
    let rescan = MenuItem::with_id(app, "rescan", "Rescan", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    mb.separator()
        .item(&open)
        .item(&rescan)
        .item(&quit)
        .build()
}

/// The human labels a menu built from `top` shows (for evidence / the probe).
fn sellable_labels(top: &[SellableRow]) -> Vec<String> {
    if top.is_empty() {
        return vec!["No sellables yet — scan your inventory".to_string()];
    }
    top.iter()
        .map(|r| format!("{} — {}p", r.name, r.price.round() as i64))
        .collect()
}

/// Recompute the ranking and swap the tray menu in. Best-effort at every step:
/// a menu-build error or a missing tray (init failed / de-scoped) is logged and
/// swallowed — the window and notifications must keep working regardless. Called
/// at startup, after each scan, and after a market refresh. Returns the full
/// ranked list so a caller (the scan path) can reuse it for the notification.
fn rebuild_tray(app: &AppHandle) -> Vec<SellableRow> {
    let rows = rank_all(app);
    let top: Vec<SellableRow> = rows.iter().take(TRAY_LIMIT).cloned().collect();
    *app.state::<TrayState>().labels.lock().unwrap() = sellable_labels(&top);
    match build_tray_menu(app, &top) {
        Ok(menu) => match app.tray_by_id("main") {
            Some(tray) => {
                if let Err(e) = tray.set_menu(Some(menu)) {
                    eprintln!("tennoworth: tray set_menu failed: {e}");
                }
            }
            None => eprintln!("tennoworth: no tray to update (init failed or de-scoped)"),
        },
        Err(e) => eprintln!("tennoworth: tray menu build failed: {e}"),
    }
    rows
}

/// After a successful scan: rebuild the tray off the new snapshot and fire the
/// post-scan notification — but only when something is actually sellable. No
/// notification on an empty result (build_notification returns None).
fn post_scan_surfaces(app: &AppHandle) {
    let rows = rebuild_tray(app);
    if let Some(n) = sellables::build_notification(&rows) {
        *app.state::<TrayState>().last_notification.lock().unwrap() = Some(n);
        let noun = if n.count == 1 { "item" } else { "items" };
        let body = format!("{} {} worth ~{}p to sell", n.count, noun, n.total_plat);
        if let Err(e) = app
            .notification()
            .builder()
            .title("TennoWorth")
            .body(&body)
            .show()
        {
            eprintln!("tennoworth: post-scan notification failed: {e}");
        }
    }
}

/// Show, un-minimize, and focus the main window — the tray's "Open" and a
/// left-click both route here.
fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Run a scan from the tray "Rescan" item: scan → record snapshot → refresh the
/// tray + notification. Runs on its own thread (the menu-event callback must not
/// block), and mirrors what the SPA-driven `scan_inventory` command does. A scan
/// error is logged, not surfaced (there's no banner behind a tray click).
fn tray_rescan(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || match scanner().scan(None, None) {
        Ok((bytes, info)) => {
            let db = app.state::<Db>();
            if let Err(e) = record_snapshot(&db, "memory", info.build.as_deref(), &bytes) {
                eprintln!("tennoworth: tray rescan snapshot not recorded: {e}");
            }
            post_scan_surfaces(&app);
        }
        Err(e) => eprintln!("tennoworth: tray rescan failed: {}", e.into_message()),
    });
}

/// Build and register the system tray. Best-effort: any failure (including the
/// forced-failure test hook) returns Err, which the caller logs and swallows so
/// startup never dies on a tray problem — the Linux baseline is window +
/// notifications, tray is a bonus.
fn init_tray(app: &AppHandle) -> tauri::Result<()> {
    // Test hook: force the tray-init failure path so the graceful-degradation
    // branch is verifiable (the window must still work).
    if std::env::var("TENNOWORTH_TRAY_FAIL").ok().as_deref() == Some("1") {
        return Err(tauri::Error::FailedToReceiveMessage);
    }
    let rows = rank_all(app);
    let top: Vec<SellableRow> = rows.iter().take(TRAY_LIMIT).cloned().collect();
    *app.state::<TrayState>().labels.lock().unwrap() = sellable_labels(&top);
    let menu = build_tray_menu(app, &top)?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or(tauri::Error::FailedToReceiveMessage)?;
    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("TennoWorth — what to sell right now")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main_window(app),
            "rescan" => tray_rescan(app),
            "quit" => app.exit(0),
            // Clicking a specific sellable opens the full table to act on it.
            id if id.starts_with("sell:") => show_main_window(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
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
/// Tauri boundary (the desktop analogue of the loopback `/health` probe).
#[tauri::command]
fn health() -> Health {
    Health {
        ok: true,
        platform: std::env::consts::OS.to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        core_version: wfm_core::version().to_string(),
    }
}

/// Extract snapshot rows from raw inventory bytes and append them to history as
/// one transactional snapshot. Shared by the memory scan and the file-drop
/// import. Returns the new snapshot id.
fn record_snapshot(
    db: &Db,
    source: &str,
    game_version: Option<&str>,
    bytes: &[u8],
) -> Result<i64, String> {
    let items = snapshot::extract_items(bytes)
        .map_err(|e| format!("parse inventory for snapshot: {e}"))?;
    db.insert_snapshot(source, None, game_version, &items)
        .map_err(|e| format!("insert snapshot: {e}"))
}

/// Memory-scan the running game and return the inventory JSON as a string —
/// the exact bytes the CLI would write to inventory.json. Async + spawn_blocking
/// so the (potentially slow) scan never blocks the webview event loop. A busy
/// guard or a missing/unscannable game becomes a rejected invoke carrying
/// wfm-core's graceful, actionable message (e.g. "Warframe doesn't appear to be
/// running…") — the SPA surfaces it verbatim in its error banner.
///
/// On success it also appends a `source='memory'` history snapshot. That insert
/// is best-effort: a failure is logged to stderr and swallowed — losing a
/// history row must never cost the user their scan (scan value > history value).
#[tauri::command]
async fn scan_inventory(app: AppHandle, db: State<'_, Db>) -> Result<String, String> {
    let (bytes, info) = tauri::async_runtime::spawn_blocking(|| scanner().scan(None, None))
        .await
        .map_err(|e| format!("scan task failed to run: {e}"))?
        .map_err(|e| e.into_message())?;

    if let Err(e) = record_snapshot(&db, "memory", info.build.as_deref(), &bytes) {
        eprintln!("tennoworth: inventory snapshot not recorded: {e}");
    }

    // C6: refresh the tray off the new snapshot and fire the post-scan
    // notification. Best-effort — never let a surface problem fail the scan
    // (the SPA still gets its inventory JSON below).
    post_scan_surfaces(&app);

    String::from_utf8(bytes).map_err(|e| format!("inventory response was not valid UTF-8: {e}"))
}

/// Record a dropped inventory.json as a `source='import'` history snapshot.
/// Used by the desktop file-drop path; unlike the scan path this surfaces the
/// error to the caller (the SPA logs it and moves on).
#[tauri::command]
fn import_snapshot(db: State<'_, Db>, inventory_json: String) -> Result<i64, String> {
    record_snapshot(&db, "import", None, inventory_json.as_bytes())
}

#[tauri::command]
fn get_setting(db: State<'_, Db>, key: String) -> Result<Option<String>, String> {
    db.get_setting(&key).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_setting(db: State<'_, Db>, key: String, value: String) -> Result<(), String> {
    db.set_setting(&key, &value).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_reserves(db: State<'_, Db>) -> Result<Vec<Reserve>, String> {
    db.get_reserves().map_err(|e| e.to_string())
}

#[tauri::command]
fn set_reserve(db: State<'_, Db>, slug: String, keep: i64) -> Result<(), String> {
    db.set_reserve(&slug, keep).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_reserve(db: State<'_, Db>, slug: String) -> Result<(), String> {
    db.delete_reserve(&slug).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_snapshots(db: State<'_, Db>, limit: i64) -> Result<Vec<SnapshotSummary>, String> {
    db.list_snapshots(limit).map_err(|e| e.to_string())
}

/// The app-data-cached market snapshot, or null on a first run / unreadable
/// cache. No network: the SPA reads this at boot to prefer the cache (last
/// known-good from the live server) over the compile-time bundled floor.
#[tauri::command]
fn cached_market(cache: State<'_, MarketCache>) -> Option<String> {
    cache.cached()
}

/// Conditionally refresh the market snapshot from tennoworth.app (ETag /
/// If-None-Match), updating the app-data cache. Async + spawn_blocking so the
/// (network) call never blocks the webview event loop, mirroring scan_inventory
/// (reqwest::blocking must not run on an async worker thread). Every network /
/// HTTP / body failure is swallowed inside `market::refresh` and returns a
/// no-op RefreshResult — the only Err here is the blocking task failing to run.
#[tauri::command]
async fn refresh_market(app: AppHandle, cache: State<'_, MarketCache>) -> Result<RefreshResult, String> {
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

/// The tray labels the last rebuild pushed + the last notification payload —
/// evidence surface for the probe (the GTK menu isn't screenshot-able headless)
/// and the backing for a later in-window "last scan" recap.
#[derive(serde::Serialize)]
struct TrayStateReport {
    labels: Vec<String>,
    last_notification: Option<ScanNotification>,
}

#[tauri::command]
fn tray_state(state: State<'_, TrayState>) -> TrayStateReport {
    TrayStateReport {
        labels: state.labels.lock().unwrap().clone(),
        last_notification: *state.last_notification.lock().unwrap(),
    }
}

/// Probe-only: run the full post-scan surface path (rebuild tray + fire the
/// notification) against whatever the latest snapshot is, so the notification
/// payload can be asserted without a running game (a seeded import_snapshot is
/// enough). Gated behind TENNOWORTH_PROBE so a normal build can't reach it.
#[tauri::command]
fn debug_post_scan(app: AppHandle) -> Result<Option<ScanNotification>, String> {
    if std::env::var("TENNOWORTH_PROBE").ok().as_deref() != Some("1") {
        return Err("debug_post_scan is probe-only".into());
    }
    post_scan_surfaces(&app);
    Ok(*app.state::<TrayState>().last_notification.lock().unwrap())
}

/// Rank the latest snapshot × market by the shared sell-priority score and
/// return the top `limit` sellables. The single join both the tray menu and the
/// post-scan notification consume; also available to the SPA. Reads the freshest
/// market it holds (app-data cache, else the compile-time bundle).
#[tauri::command]
fn top_sellables(
    db: State<'_, Db>,
    cache: State<'_, MarketCache>,
    limit: usize,
) -> Vec<sellables::SellableRow> {
    let market = sellables::MarketData::load(&cache);
    let mut rows = sellables::rank_sellables(&db, &market);
    rows.truncate(limit);
    rows
}

// ---- WFM session + listing commands ---------------------------------------
//
// The desktop mirror of serve's listing routes: same wfm-core services, with
// the passphrase arriving from the webview (`wfm_login` / `unlock_jwt`)
// instead of a TTY prompt. Every fallible command rejects with a serialized
// CmdError {code, message}; `needs_login` / `needs_unlock` drive the SPA's
// login and passphrase dialogs — the desktop analogue of serve's
// 401 needs_login:true vs 503 split. The plaintext JWT stays inside
// WfmSession; no command returns it, and no command logs it.

#[derive(serde::Serialize)]
struct WfmAuthStatus {
    /// A login envelope exists on disk (encrypted; says nothing about the
    /// passphrase being known).
    logged_in: bool,
    /// This process holds the decrypted JWT in memory.
    unlocked: bool,
}

#[tauri::command]
fn wfm_auth_status(session: State<'_, Arc<WfmSession>>) -> WfmAuthStatus {
    let (logged_in, unlocked) = session.auth_status();
    WfmAuthStatus { logged_in, unlocked }
}

/// Sign in to warframe.market with credentials from the SPA's login dialog,
/// persist the encrypted JWT (unchanged envelope format), and unlock the
/// session. Network — spawn_blocking keeps the webview event loop free.
#[tauri::command]
async fn wfm_login(
    session: State<'_, Arc<WfmSession>>,
    email: String,
    password: String,
    passphrase: String,
    platform: String,
) -> Result<(), CmdError> {
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        // Zeroizing scrubs OUR copies of the secrets when the closure ends —
        // best-effort (the IPC deserializer made its own transient copies).
        let password = Zeroizing::new(password);
        let passphrase = Zeroizing::new(passphrase);
        s.login(&email, &password, &passphrase, &platform)
    })
    .await
    .map_err(|e| CmdError::internal(format!("login task failed to run: {e}")))?
}

/// Decrypt the stored JWT with the passphrase from the SPA's unlock dialog and
/// warm the WFM catalog. Missing file → `needs_login`; wrong passphrase →
/// `bad_passphrase`; catalog/me failure → `wfm` (transient, retryable).
#[tauri::command]
async fn unlock_jwt(
    session: State<'_, Arc<WfmSession>>,
    passphrase: String,
) -> Result<(), CmdError> {
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let passphrase = Zeroizing::new(passphrase);
        s.unlock(&passphrase)
    })
    .await
    .map_err(|e| CmdError::internal(format!("unlock task failed to run: {e}")))?
}

/// Lock the session and scrub the in-memory JWT. The on-disk envelope stays —
/// re-unlocking needs only the passphrase, not a fresh WFM login.
#[tauri::command]
fn wfm_logout(session: State<'_, Arc<WfmSession>>) {
    session.logout();
}

const PLAN_BUSY_MSG: &str = "A listing plan is already running — wait for it to finish.";

/// Execute a listing batch — the desktop POST /plan. Pacing, caps, pending-file
/// persistence, and per-item results all come from wfm-core's execute_plan.
#[tauri::command]
async fn submit_plan(
    session: State<'_, Arc<WfmSession>>,
    items: Vec<PlanItem>,
) -> Result<PlanResponse, CmdError> {
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let unlocked = s.require_unlocked()?;
        let _guard = s.begin_plan().ok_or_else(|| CmdError::of("busy", PLAN_BUSY_MSG))?;
        Ok(core_execute_plan(s.pending_path(), &unlocked, PlanRequest { items }))
    })
    .await
    .map_err(|e| CmdError::internal(format!("plan task failed to run: {e}")))?
}

/// The last interrupted plan, or null. No auth — mirrors serve's JWT-free
/// GET /plan/pending, so the SPA can poll it before any unlock.
#[tauri::command]
fn get_pending_plan(session: State<'_, Arc<WfmSession>>) -> Option<PendingPlan> {
    load_pending(session.pending_path())
}

#[tauri::command]
fn discard_pending_plan(session: State<'_, Arc<WfmSession>>) {
    clear_pending(session.pending_path());
}

/// Re-run the pending plan, skipping items already in a terminal state.
#[tauri::command]
async fn resume_pending_plan(
    session: State<'_, Arc<WfmSession>>,
) -> Result<PlanResponse, CmdError> {
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        // Pending-first ordering mirrors serve (its 404 outranks auth): with
        // nothing to resume the user must not be bounced into a login dialog.
        let mut pending = load_pending(s.pending_path())
            .ok_or_else(|| CmdError::of("no_pending", "No pending plan to resume."))?;
        let unlocked = s.require_unlocked()?;
        let _guard = s.begin_plan().ok_or_else(|| CmdError::of("busy", PLAN_BUSY_MSG))?;
        let response = run_pending(s.pending_path(), &unlocked, &mut pending);
        clear_pending(s.pending_path());
        Ok(response)
    })
    .await
    .map_err(|e| CmdError::internal(format!("resume task failed to run: {e}")))?
}

/// The user's current WFM listings, enriched with display names (GET /orders).
#[tauri::command]
async fn fetch_orders(session: State<'_, Arc<WfmSession>>) -> Result<serde_json::Value, CmdError> {
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let unlocked = s.require_unlocked()?;
        list_user_orders(&unlocked).map_err(CmdError::wfm)
    })
    .await
    .map_err(|e| CmdError::internal(format!("orders task failed to run: {e}")))?
}

/// PATCH one order: price / quantity / visible / rank.
#[tauri::command]
async fn update_order(
    session: State<'_, Arc<WfmSession>>,
    order_id: String,
    patch: UpdateRequest,
) -> Result<PerOrderResult, CmdError> {
    // Same cap as the create path — mirrors serve's pre-auth 400 so an edit
    // can't push a listing past what the WFM UI allows.
    if let Some(p) = patch.platinum {
        if p > MAX_PLATINUM {
            return Err(CmdError::wfm(format!("price {p}p > max {MAX_PLATINUM}p")));
        }
    }
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let unlocked = s.require_unlocked()?;
        core_update_order(&unlocked, &order_id, &patch).map_err(CmdError::wfm)
    })
    .await
    .map_err(|e| CmdError::internal(format!("order update task failed to run: {e}")))?
}

#[tauri::command]
async fn delete_order(
    session: State<'_, Arc<WfmSession>>,
    order_id: String,
) -> Result<(), CmdError> {
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let unlocked = s.require_unlocked()?;
        core_delete_order(&unlocked, &order_id).map_err(CmdError::wfm)
    })
    .await
    .map_err(|e| CmdError::internal(format!("order delete task failed to run: {e}")))?
}

/// Bulk-toggle listing visibility (POST /orders/visibility). Per-order results;
/// pacing lives in wfm-core's bulk_set_visibility.
#[tauri::command]
async fn bulk_visibility(
    session: State<'_, Arc<WfmSession>>,
    order_ids: Vec<String>,
    visible: bool,
) -> Result<Vec<PerOrderResult>, CmdError> {
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let unlocked = s.require_unlocked()?;
        Ok(bulk_set_visibility(&unlocked, &VisibilityRequest { order_ids, visible }))
    })
    .await
    .map_err(|e| CmdError::internal(format!("visibility task failed to run: {e}")))?
}

/// The DeepSeek advisor relay (POST /assistant) — the only command with
/// third-party egress. Key resolution, caps, prompt fencing, and the ≤20/60s
/// throttle all mirror serve; the API key never reaches the webview.
#[tauri::command]
async fn ask_assistant(
    session: State<'_, Arc<WfmSession>>,
    question: String,
    history: Vec<AssistantMessage>,
    context: Option<String>,
) -> Result<AssistantResponse, CmdError> {
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let context = context.unwrap_or_default();
        if assistant_request_too_large(&question, &context) {
            return Err(CmdError::of("too_large", "Question or context is too large."));
        }
        let api_key = resolve_deepseek_key(
            std::env::var("DEEPSEEK_API_KEY").ok().as_deref(),
            s.key_dir(),
        )
        .ok_or_else(|| {
            CmdError::of(
                "no_api_key",
                "No DeepSeek API key configured — set DEEPSEEK_API_KEY or the deepseek-key config file.",
            )
        })?;
        // Checked just before the upstream call — a rejected/oversized/keyless
        // request never counts against the budget (same as serve).
        {
            let mut calls = s.assistant_calls.lock().expect("assistant_calls mutex poisoned");
            if assistant_rate_limited(&mut calls, Instant::now()) {
                return Err(CmdError::of(
                    "rate_limited",
                    "Too many advisor requests — wait a minute and try again.",
                ));
            }
        }
        let messages = build_assistant_messages(&context, &cap_history(history), &question);
        let client = deepseek_client().map_err(|e| CmdError::of("upstream", short_reason(&e)))?;
        let (answer, usage) = call_deepseek(&client, &api_key, messages)
            .map_err(|e| CmdError::of("upstream", short_reason(&e)))?;
        Ok(AssistantResponse { answer, usage })
    })
    .await
    .map_err(|e| CmdError::internal(format!("assistant task failed to run: {e}")))?
}

/// Probe-only: write a real encrypted login envelope (production encrypt +
/// persist path) so the hermetic probe can drive the needs_unlock /
/// bad_passphrase branches without a live WFM login.
#[tauri::command]
fn debug_write_login(
    session: State<'_, Arc<WfmSession>>,
    passphrase: String,
) -> Result<(), CmdError> {
    if std::env::var("TENNOWORTH_PROBE").ok().as_deref() != Some("1") {
        return Err(CmdError::internal("debug_write_login is probe-only"));
    }
    session.debug_write_login(&passphrase)
}

/// Probe-only: flip the session to unlocked with a synthetic credential bundle
/// (empty catalog, fake JWT) — no network. Listing commands then exercise
/// their offline validation paths; anything that would hit WFM fails per-item.
#[tauri::command]
fn debug_seed_unlocked(session: State<'_, Arc<WfmSession>>) -> Result<(), CmdError> {
    if std::env::var("TENNOWORTH_PROBE").ok().as_deref() != Some("1") {
        return Err(CmdError::internal("debug_seed_unlocked is probe-only"));
    }
    session.debug_set_unlocked(Unlocked {
        jwt: "probe.jwt.value".into(),
        username: "probe".into(),
        platform: "pc".into(),
        catalog: Arc::new(BTreeMap::new()),
        id_to_name: Arc::new(BTreeMap::new()),
    });
    Ok(())
}

/// Probe-only: persist the evidence JSON to $TENNOWORTH_PROBE_OUT (and echo it
/// to stdout between markers so it is captured even without file access).
#[tauri::command]
fn probe_report(payload: String) -> Result<String, String> {
    let out = std::env::var("TENNOWORTH_PROBE_OUT")
        .unwrap_or_else(|_| "/tmp/tennoworth-probe.json".into());
    std::fs::write(&out, payload.as_bytes()).map_err(|e| e.to_string())?;
    let mut so = std::io::stdout();
    let _ = writeln!(so, "PROBE_REPORT_FILE {out}");
    let _ = writeln!(so, "PROBE_REPORT_BEGIN");
    let _ = writeln!(so, "{payload}");
    let _ = writeln!(so, "PROBE_REPORT_END");
    let _ = so.flush();
    Ok(out)
}

/// Probe-only: close the app so the restart-persistence check can run two clean
/// launches without a human closing the window.
#[tauri::command]
fn probe_exit() {
    let mut so = std::io::stdout();
    let _ = writeln!(so, "PROBE_EXIT");
    let _ = so.flush();
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(300));
        std::process::exit(0);
    });
}

// A minimal DE inventory: four distinct tradeable-category paths (so
// extract_items → item_count == 4), three of which resolve to WFM slugs with
// market stats (so the drop flips to the sell view and the reserve input
// renders). Kept in sync with the categories snapshot::extract_items walks.
const PROBE_FIXTURE: &str = r#"{
  "RawUpgrades": [
    { "ItemType": "/Lotus/Upgrades/Mods/Shotgun/DualStat/AcceleratedBlastMod", "ItemCount": 3 },
    { "ItemType": "/Lotus/Powersuits/Trinity/LinkAugmentCard", "ItemCount": 2 },
    { "ItemType": "/Lotus/Powersuits/Khora/KhoraCrackAugmentCard", "ItemCount": 5 }
  ],
  "MiscItems": [
    { "ItemType": "/Lotus/Types/Items/MiscItems/OrokinCell", "ItemCount": 42 }
  ]
}"#;

const PROBE_JS: &str = r#"(function(){
  var R = { runtag: "__RUNTAG__", steps_ts: new Date().toISOString(), cspViolations: [], consoleErrors: [] };
  var FIXTURE = __FIXTURE__;
  try {
    document.addEventListener('securitypolicyviolation', function(e){
      if (R.cspViolations.length < 20) R.cspViolations.push({ blockedURI: e.blockedURI, violatedDirective: e.violatedDirective, effectiveDirective: e.effectiveDirective, disposition: e.disposition });
    });
  } catch(e){}
  try {
    var origErr = console.error.bind(console);
    console.error = function(){ try { if (R.consoleErrors.length < 20) R.consoleErrors.push(Array.prototype.slice.call(arguments).map(String).join(' ').slice(0,200)); } catch(_){} return origErr.apply(null, arguments); };
  } catch(e){}
  function invokeFn(){
    try {
      if (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke) return window.__TAURI__.core.invoke;
      if (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke) return window.__TAURI_INTERNALS__.invoke;
    } catch(e){}
    return null;
  }
  function invk(cmd, args){
    var inv = invokeFn();
    if (!inv) return Promise.resolve('NO_INVOKE_FN');
    return inv(cmd, args).catch(function(e){ return 'ERR:'+(e && e.message || e); });
  }
  // Like invk, but keeps the typed CmdError shape: rejections come back as
  // { ok:false, code, message } so the report can assert needs_login vs
  // needs_unlock vs bad_passphrase instead of a flattened string.
  function invkE(cmd, args){
    var inv = invokeFn();
    if (!inv) return Promise.resolve('NO_INVOKE_FN');
    return inv(cmd, args).then(
      function(v){ return { ok: true, value: v === undefined ? null : v }; },
      function(e){ return { ok: false, code: e && e.code, message: String(e && e.message || e).slice(0, 160) }; }
    );
  }
  function probeFetch(url){
    return fetch(url, { cache:'no-store' }).then(function(r){
      return r.text().then(function(b){ return { ok:r.ok, status:r.status, type:r.type, len:b.length, head:b.slice(0,48) }; });
    }).catch(function(e){ return { error: String(e && e.message || e), name: e && e.name }; });
  }
  function delay(ms){ return new Promise(function(res){ setTimeout(res, ms); }); }
  function curWin(){
    try { if (window.__TAURI__ && window.__TAURI__.window && window.__TAURI__.window.getCurrentWindow) return window.__TAURI__.window.getCurrentWindow(); } catch(e){}
    return null;
  }
  // C6 lifecycle: window.close() fires CloseRequested, which Rust intercepts
  // (prevent_close + hide) — so the window HIDES to the tray and the process
  // stays alive (this very script keeps running). show() reshows it.
  function windowLifecycle(){
    var w = curWin();
    if (!w) { R.lifecycle = 'NO_WINDOW_API'; return Promise.resolve(); }
    R.lifecycle = {};
    return w.isVisible().then(function(v){ R.lifecycle.visibleBeforeClose = v; })
      .then(function(){ return w.close(); })
      .then(function(){ return delay(600); })
      .then(function(){ return w.isVisible(); }).then(function(v){ R.lifecycle.visibleAfterClose = v; })
      .then(function(){ return w.show(); })
      .then(function(){ return delay(300); })
      .then(function(){ return w.isVisible(); }).then(function(v){ R.lifecycle.visibleAfterReshow = v; })
      .then(function(){ R.lifecycle.survivedClose = true; })
      .catch(function(e){ R.lifecycle.err = String(e && e.message || e); });
  }
  // Synthesize a REAL file-drop onto the DropZone: webkit's DragEvent ctor won't
  // carry a dataTransfer, so attach one with a File via defineProperty. This
  // exercises DropZone → handleInventory(origin:'file') → import_snapshot.
  function dropFixture(){
    try {
      var dz = document.querySelector('.dropzone');
      if (!dz) return 'NO_DROPZONE';
      var file = new File([FIXTURE], 'fixture-inventory.json', { type: 'application/json' });
      var dt = new DataTransfer();
      dt.items.add(file);
      var ev = new Event('drop', { bubbles: true, cancelable: true });
      Object.defineProperty(ev, 'dataTransfer', { value: dt });
      dz.dispatchEvent(ev);
      return 'DROPPED';
    } catch(e){ return 'ERR:'+(e && e.message || e); }
  }
  // Drive the REAL reserve input if the sell view rendered (→ setReserveCopies →
  // store.setSetting → set_setting); else fall back to the raw command so the
  // scenario still records evidence. Reports which path was taken.
  function setReserve(){
    try {
      var el = document.querySelector('[data-testid="reserve-copies"]');
      if (el) {
        var setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set;
        setter.call(el, '4');
        el.dispatchEvent(new Event('input', { bubbles: true }));
        return delay(300).then(function(){ return 'UI'; });
      }
    } catch(e){ return Promise.resolve('UI_ERR:'+(e && e.message || e)); }
    return invk('set_setting', { key: 'reserve-copies', value: '4' }).then(function(){ return 'INVOKE'; });
  }
  function run(){
    try {
      R.origin = location.origin;
      R.href = location.href;
      R.protocol = location.protocol;
      R.hasGlobalTauri = typeof window.__TAURI__ !== 'undefined';
      R.hasInternals = typeof window.__TAURI_INTERNALS__ !== 'undefined';
      R.spaTitle = document.title;
      var app = document.querySelector('#app');
      R.appMounted = !!(app && app.childElementCount > 0);
      R.appChildCount = app ? app.childElementCount : -1;
      R.bodyTextLen = (document.body && document.body.innerText || '').length;
      R.desktopBadge = !!document.querySelector('[data-testid="desktop-mode"]');
      R.marketBrowserRendered = !!document.querySelector('.market-browser, [data-testid="market-browser"]');
    } catch(e){ R.envErr = String(e); }
    // Persistence marker chain (webview localStorage — separate from the SQLite
    // store; the real cross-restart proof is reserveAtStart via get_setting).
    var marker = R.runtag + '@' + new Date().toISOString();
    try { R.priorMarker = localStorage.getItem('__tennoworth_probe_marker__'); } catch(e){ R.priorMarker = 'ERR:'+e; }
    try { localStorage.setItem('__tennoworth_probe_marker__', marker); R.wroteMarker = marker; } catch(e){ R.wroteMarker='ERR:'+e; }
    probeFetch('/market.json')
    .then(function(x){ R.fetchMarket = x; })
    .then(function(){ return probeFetch('/wfstat-catalog.json').then(function(x){ R.fetchCatalog = x; }); })
    .then(function(){ return invk('health').then(function(v){ R.invokeHealth = v; }); })
    // C4 market refresh: cache presence before, the conditional-GET outcome, then
    // cache presence after. Across launches this shows 200 → cache written, then
    // If-None-Match → 304 → cache kept (updated:false, no body re-sent).
    .then(function(){ return invk('cached_market').then(function(v){ R.cachedMarketBefore = (typeof v === 'string' && v.length > 0); R.cachedMarketBeforeLen = (typeof v === 'string') ? v.length : 0; }); })
    .then(function(){ return invk('refresh_market').then(function(v){ R.marketRefresh = (v && typeof v === 'object') ? { updated: v.updated, updated_at: v.updated_at, etag: v.etag, bodyLen: v.body ? v.body.length : 0 } : v; }); })
    .then(function(){ return invk('cached_market').then(function(v){ R.cachedMarketAfter = (typeof v === 'string' && v.length > 0); R.cachedMarketAfterLen = (typeof v === 'string') ? v.length : 0; }); })
    // Cross-restart persistence: the value a PRIOR run wrote. null on run 1,
    // '4' on run 2 → proves the SQLite setting survived the restart.
    .then(function(){ return invk('get_setting', { key: 'reserve-copies' }).then(function(v){ R.reserveAtStart = v; }); })
    .then(function(){ return invk('list_snapshots', { limit: 50 }).then(function(v){ R.snapshotsAtStart = v; }); })
    // (b) Scan with no game running: drive the REAL scan button. Graceful error
    // banner, and — critically — NO source='memory' snapshot row is added.
    .then(function(){
      var btn = document.querySelector('[data-testid="desktop-scan"]');
      R.scanButtonFound = !!btn;
      if (!btn) return;
      btn.click();
      return delay(1800).then(function(){
        var banner = document.querySelector('.general-banner .gb-body');
        R.scanBannerText = banner ? (banner.innerText || '').slice(0, 300) : null;
      });
    })
    .then(function(){ return invk('list_snapshots', { limit: 50 }).then(function(v){ R.snapshotsAfterScan = v; }); })
    // (c) File-drop the fixture → source='import' snapshot with item_count == 4.
    .then(function(){ R.dropResult = dropFixture(); return delay(2500); })
    .then(function(){
      R.phaseDoneAfterDrop = !document.querySelector('[data-testid="desktop-mode"] .dropzone, .dropzone');
      R.resultsRowsAfterDrop = document.querySelectorAll('table tbody tr').length;
      return invk('list_snapshots', { limit: 50 }).then(function(v){ R.snapshotsAfterDrop = v; });
    })
    // C6 (top_sellables): rank the imported snapshot × bundled market. With a
    // clean data dir (reserve 0) this is the deterministic 3-item ranking.
    .then(function(){ return invk('top_sellables', { limit: 5 }).then(function(v){ R.topSellables = v; }); })
    // C6 (notification): run the post-scan surface path against the latest
    // snapshot (probe-only, so it works with no game) → payload {count,total}.
    .then(function(){ return invk('debug_post_scan').then(function(v){ R.debugNotify = v; }); })
    // C6 (tray model): the labels the rebuild actually pushed + stored payload.
    .then(function(){ return invk('tray_state').then(function(v){ R.trayState = v; }); })
    // (a) Set reserve via the REAL input (now rendered) → set_setting.
    .then(function(){ return setReserve().then(function(via){ R.reserveSetVia = via; }); })
    .then(function(){ return invk('get_setting', { key: 'reserve-copies' }).then(function(v){ R.reserveAfterSet = v; }); })
    // C7 WFM listing session: the full lock-state machine, hermetic (no WFM
    // network — TENNOWORTH_JWT_PATH/TENNOWORTH_PENDING_PATH point at scratch,
    // and every plan item fails validation before any HTTP).
    .then(function(){ R.wfm = {}; return invkE('wfm_auth_status').then(function(v){ R.wfm.status0 = v; }); })
    // No login file → typed needs_login (the desktop analogue of serve's 401).
    .then(function(){ return invkE('submit_plan', { items: [] }).then(function(v){ R.wfm.planNoLogin = v; }); })
    // Real Sell CTA with no login → the login dialog opens (proactive check).
    .then(function(){
      var btn = document.querySelector('[data-testid="desktop-list"]');
      R.wfm.listBtnFound = !!btn;
      if (btn) btn.click();
      return delay(700);
    })
    .then(function(){
      var d = document.querySelector('[data-testid="wfm-login-dialog"]');
      R.wfm.loginDialogOpen = !!(d && d.open);
      if (d && d.open) d.close();
    })
    // Write a REAL encrypted envelope (production encrypt+persist), then the
    // same call sites must flip to needs_unlock.
    .then(function(){ return invkE('debug_write_login', { passphrase: 'probe-pass-123456' }).then(function(v){ R.wfm.wroteLogin = v; }); })
    .then(function(){ return invkE('wfm_auth_status').then(function(v){ R.wfm.status1 = v; }); })
    .then(function(){ return invkE('submit_plan', { items: [] }).then(function(v){ R.wfm.planLocked = v; }); })
    // Real CTA again → unlock dialog; drive the REAL form with a wrong
    // passphrase → bad_passphrase surfaces in the dialog, which stays open.
    .then(function(){
      var btn = document.querySelector('[data-testid="desktop-list"]');
      if (btn) btn.click();
      return delay(700);
    })
    .then(function(){
      var d = document.querySelector('[data-testid="wfm-unlock-dialog"]');
      R.wfm.unlockDialogOpen = !!(d && d.open);
      if (!(d && d.open)) return;
      var inp = d.querySelector('[data-testid="wfm-unlock-pass"]');
      var setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set;
      setter.call(inp, 'wrong-passphrase');
      inp.dispatchEvent(new Event('input', { bubbles: true }));
      d.querySelector('form').requestSubmit();
      return delay(2500).then(function(){
        var err = d.querySelector('[data-testid="wfm-auth-error"]');
        R.wfm.unlockWrongPassError = err ? (err.innerText || '').slice(0, 120) : null;
        R.wfm.unlockDialogStillOpen = d.open;
        d.close();
      });
    })
    // Seed an unlocked session (probe-only, synthetic bundle — no network),
    // then the CTA opens the review modal with the staged fixture rows.
    .then(function(){ return invkE('debug_seed_unlocked').then(function(v){ R.wfm.seeded = v; }); })
    .then(function(){ return invkE('wfm_auth_status').then(function(v){ R.wfm.status2 = v; }); })
    .then(function(){
      var btn = document.querySelector('[data-testid="desktop-list"]');
      if (btn) btn.click();
      return delay(700);
    })
    .then(function(){
      var modal = document.querySelector('.modal');
      R.wfm.reviewModalOpen = !!modal;
      R.wfm.reviewModalRows = document.querySelectorAll('.modal tbody tr').length;
      var x = document.querySelector('.modal header .x');
      if (x) x.click();
      return delay(300);
    })
    // Offline plan execution: both items fail wfm-core validation BEFORE any
    // HTTP (price under the 5p floor; slug not in the catalog) — exercises the
    // full plan pipeline incl. pending-file seed + clean clear, no network.
    .then(function(){
      return invkE('submit_plan', { items: [
        { slug: 'ash_prime_set', platinum: 1, quantity: 1, order_type: 'sell', visible: false },
        { slug: 'not_a_real_slug', platinum: 10, quantity: 1, order_type: 'sell', visible: false }
      ]}).then(function(v){ R.wfm.planOffline = v; });
    })
    .then(function(){ return invkE('get_pending_plan').then(function(v){ R.wfm.pendingAfterPlan = v; }); })
    .then(function(){ return invkE('resume_pending_plan').then(function(v){ R.wfm.resumeNoPending = v; }); })
    .then(function(){ return invkE('wfm_logout').then(function(v){ R.wfm.logout = v; }); })
    .then(function(){ return invkE('wfm_auth_status').then(function(v){ R.wfm.status3 = v; }); })
    // Locked again with the envelope still on disk → needs_unlock, not login.
    .then(function(){ return invkE('submit_plan', { items: [] }).then(function(v){ R.wfm.planAfterLogout = v; }); })
    // Checkpoint the evidence BEFORE the lifecycle test — if close-to-tray were
    // broken and destroyed the window, the final report below would never write.
    .then(function(){ return invk('probe_report', { payload: JSON.stringify(R) }); })
    // C6 (lifecycle): close hides to tray (process survives), show reshows.
    .then(function(){ return windowLifecycle(); })
    .then(function(){
      R.done = true;
      var json = JSON.stringify(R);
      try { localStorage.setItem('__tennoworth_probe_report__', json); } catch(e){}
      try { document.title = 'PROBE_DONE ' + R.runtag; } catch(e){}
      var inv = invokeFn();
      if (inv) {
        inv('probe_report', { payload: json }).catch(function(){}).then(function(){
          setTimeout(function(){ inv('probe_exit').catch(function(){}); }, 400);
        });
      }
    })
    .catch(function(e){ try { R.fatal='ERR:'+(e && e.message || e); localStorage.setItem('__tennoworth_probe_report__', JSON.stringify(R)); invk('probe_report', { payload: JSON.stringify(R) }); } catch(_){} });
  }
  if (document.readyState === 'complete') setTimeout(run, 900);
  else window.addEventListener('load', function(){ setTimeout(run, 900); });
})();"#;

fn main() {
    let probe = std::env::var("TENNOWORTH_PROBE").ok().as_deref() == Some("1");
    let runtag = std::env::var("TENNOWORTH_RUNTAG").unwrap_or_else(|_| "na".into());

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(TrayState::default())
        .manage(Arc::new(WfmSession::new()))
        .invoke_handler(tauri::generate_handler![
            health,
            scan_inventory,
            import_snapshot,
            get_setting,
            set_setting,
            get_reserves,
            set_reserve,
            delete_reserve,
            list_snapshots,
            cached_market,
            refresh_market,
            top_sellables,
            tray_state,
            wfm_auth_status,
            wfm_login,
            unlock_jwt,
            wfm_logout,
            submit_plan,
            get_pending_plan,
            discard_pending_plan,
            resume_pending_plan,
            fetch_orders,
            update_order,
            delete_order,
            bulk_visibility,
            ask_assistant,
            debug_write_login,
            debug_seed_unlocked,
            debug_post_scan,
            probe_report,
            probe_exit
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
            if probe {
                // serde_json turns the fixture &str into a quoted, escaped JS
                // string literal so `var FIXTURE = __FIXTURE__;` parses.
                let fixture_literal = serde_json::to_string(PROBE_FIXTURE)
                    .expect("probe fixture serializes to a JS string literal");
                let js = PROBE_JS
                    .replace("__RUNTAG__", &runtag)
                    .replace("__FIXTURE__", &fixture_literal);
                b = b.initialization_script(&js);
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
            if let Err(e) = init_tray(&app.handle().clone()) {
                eprintln!("tennoworth: tray unavailable, continuing without it: {e}");
            }

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
