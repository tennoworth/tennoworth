//! System tray: menu build/rebuild, the post-scan notification, and window
//! show/rescan handlers wired to tray events. Rebuilds run at startup, after
//! every inventory scan, and after a market refresh - all three call
//! [`rebuild_tray`] so the tray and the post-scan notification never disagree
//! on what's ranked.

use std::sync::Mutex;
use tauri::menu::{Menu, MenuBuilder, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_notification::NotificationExt;

use wfm_core::poison::guard;

use crate::commands::inventory::record_snapshot;
use crate::db::Db;
use crate::market::MarketCache;
use crate::sellables::{self, MarketData, ScanNotification, SellableRow};

/// How many sellables the tray menu shows.
const TRAY_LIMIT: usize = 5;

/// Emitted to the webview when the user closes the window while the tray still
/// exists, so the SPA can show its once-ever "still running in the tray" toast.
/// Event name only - the SPA also pins this literal on the TS side (no way to
/// gate the two against each other across the language boundary).
pub const EVENT_TRAY_HINT: &str = "tray-hint";

/// Evidence-facing view of what the tray/notification code last produced. The
/// GTK tray menu isn't reliably screenshot-able under headless Wayland, so the
/// probe reads the labels the rebuild actually pushed and the last notification
/// payload from here instead. Also the single home for the "last notification"
/// so a later window can surface it.
#[derive(Default)]
pub struct TrayState {
    /// The sellable labels ("Name - Np") the last rebuild put in the menu.
    pub labels: Mutex<Vec<String>>,
    pub last_notification: Mutex<Option<ScanNotification>>,
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
/// ("Name - Np", id `sell:<slug>`), a separator, then Open / Rescan / Quit.
/// An empty list shows a single disabled hint instead.
fn build_tray_menu(app: &AppHandle, top: &[SellableRow]) -> tauri::Result<Menu<Wry>> {
    let mut mb = MenuBuilder::new(app);
    let mut sellable_items: Vec<MenuItem<Wry>> = Vec::new();
    if top.is_empty() {
        let hint = MenuItem::with_id(
            app,
            "noop",
            "No sellables yet - scan your inventory",
            false,
            None::<&str>,
        )?;
        sellable_items.push(hint);
    } else {
        for r in top {
            let label = format!("{} - {}p", r.name, r.price.round() as i64);
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
        return vec!["No sellables yet - scan your inventory".to_string()];
    }
    top.iter()
        .map(|r| format!("{} - {}p", r.name, r.price.round() as i64))
        .collect()
}

/// Recompute the ranking and swap the tray menu in. Best-effort at every step:
/// a menu-build error or a missing tray (init failed / de-scoped) is logged and
/// swallowed - the window and notifications must keep working regardless. Called
/// at startup, after each scan, and after a market refresh. Returns the full
/// ranked list so a caller (the scan path) can reuse it for the notification.
pub fn rebuild_tray(app: &AppHandle) -> Vec<SellableRow> {
    let rows = rank_all(app);
    let top: Vec<SellableRow> = rows.iter().take(TRAY_LIMIT).cloned().collect();
    *guard(&app.state::<TrayState>().labels) = sellable_labels(&top);
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
/// post-scan notification - but only when something is actually sellable. No
/// notification on an empty result (build_notification returns None).
pub fn post_scan_surfaces(app: &AppHandle) {
    let rows = rebuild_tray(app);
    if let Some(n) = sellables::build_notification(&rows) {
        *guard(&app.state::<TrayState>().last_notification) = Some(n);
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

/// Show, un-minimize, and focus the main window - the tray's "Open" and a
/// left-click both route here.
pub(crate) fn show_main_window(app: &AppHandle) {
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
    std::thread::spawn(move || match crate::scanner().scan(None, None) {
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
/// startup never dies on a tray problem - the Linux baseline is window +
/// notifications, tray is a bonus.
pub fn init_tray(app: &AppHandle) -> tauri::Result<()> {
    // Test hook: force the tray-init failure path so the graceful-degradation
    // branch is verifiable (the window must still work).
    if std::env::var("TENNOWORTH_TRAY_FAIL").ok().as_deref() == Some("1") {
        return Err(tauri::Error::FailedToReceiveMessage);
    }
    let rows = rank_all(app);
    let top: Vec<SellableRow> = rows.iter().take(TRAY_LIMIT).cloned().collect();
    *guard(&app.state::<TrayState>().labels) = sellable_labels(&top);
    let menu = build_tray_menu(app, &top)?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or(tauri::Error::FailedToReceiveMessage)?;
    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("TennoWorth - what to sell right now")
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
