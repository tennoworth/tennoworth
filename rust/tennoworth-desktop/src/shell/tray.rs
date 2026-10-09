//! System tray: menu build/rebuild, the post-scan summary payload, and window
//! show/rescan handlers wired to tray events. Rebuilds run at startup, after
//! every inventory scan, and after a market refresh - all three call
//! [`rebuild_tray`] so the tray and the recorded summary never disagree on
//! what's ranked.

use std::sync::Mutex;
use tauri::menu::{CheckMenuItem, Menu, MenuBuilder, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use wfm_core::poison::guard;

use crate::services::acquisition::{publish_scan, scan_and_record};
use crate::persistence::Db;
use crate::services::market::MarketCache;
use crate::services::presence::{self, PresenceChoice, PresenceState};
use crate::services::sellables::{self, MarketData, ScanNotification, SellableRow};

/// How many sellables the tray menu shows.
const TRAY_LIMIT: usize = 5;
const EMPTY_SELLABLES: &str = "No sellables yet - scan your inventory";

/// Emitted to the webview when the user closes the window while the tray still
/// exists, so the SPA can show its once-ever "still running in the tray" toast.
/// Event name only - the SPA also pins this literal on the TS side (no way to
/// gate the two against each other across the language boundary).
pub const EVENT_TRAY_HINT: &str = "tray-hint";

/// Evidence-facing view of what the tray/post-scan code last produced. The GTK
/// tray menu isn't reliably screenshot-able under headless Wayland, so the probe
/// reads the labels the rebuild actually pushed and the last post-scan payload
/// from here instead.
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
fn build_tray_menu(
    app: &AppHandle,
    top: &[SellableRow],
    labels: &[String],
) -> tauri::Result<Menu<Wry>> {
    let mut mb = MenuBuilder::new(app);
    let mut sellable_items: Vec<MenuItem<Wry>> = Vec::new();
    if top.is_empty() {
        let hint = MenuItem::with_id(
            app,
            "noop",
            EMPTY_SELLABLES,
            false,
            None::<&str>,
        )?;
        sellable_items.push(hint);
    } else {
        for (r, label) in top.iter().zip(labels) {
            let item =
                MenuItem::with_id(app, format!("sell:{}", r.slug), label, true, None::<&str>)?;
            sellable_items.push(item);
        }
    }
    for item in &sellable_items {
        mb = mb.item(item);
    }
    mb = mb.separator();
    if let Some(state) = app.try_state::<PresenceState>() {
        let status = state.status();
        let label = status.tray_label();
        if status.signed_in {
            let usable = status.connected && status.problem != Some(presence::PresenceProblem::NotVerified);
            let choice = |id: &str, choice: PresenceChoice| {
                CheckMenuItem::with_id(app, id, choice.label(), usable, status.status == Some(choice), None::<&str>)
            };
            let online = choice("presence:online", PresenceChoice::Online)?;
            let ingame = choice("presence:ingame", PresenceChoice::Ingame)?;
            let invisible = choice("presence:invisible", PresenceChoice::Invisible)?;
            let separator = PredefinedMenuItem::separator(app)?;
            let follow = CheckMenuItem::with_id(app, "presence:follow", "Follow the game", true, status.following, None::<&str>)?;
            let submenu = Submenu::with_id_and_items(
                app,
                "presence",
                label,
                true,
                &[&online, &ingame, &invisible, &separator, &follow],
            )?;
            mb = mb.item(&submenu).separator();
        } else {
            let line = MenuItem::with_id(app, "presence", label, false, None::<&str>)?;
            mb = mb.item(&line).separator();
        }
    }
    let open = MenuItem::with_id(app, "open", "Open TennoWorth", true, None::<&str>)?;
    let rescan = MenuItem::with_id(app, "rescan", "Rescan", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    mb.item(&open).item(&rescan).item(&quit).build()
}

/// A status picked in the tray. It waits for warframe.market, so it runs off
/// the menu thread; the menu is rebuilt afterwards either way, because a native
/// check item ticks itself on click even when the change is refused.
fn tray_presence(app: &AppHandle, choice: PresenceChoice) {
    let app = app.clone();
    std::thread::spawn(move || {
        if let Err(e) = app.state::<PresenceState>().set(choice) {
            eprintln!("tennoworth: tray status change failed: {}", e.message);
        }
        rebuild_tray(&app);
    });
}

/// "Follow the game": off when following, resumed when paused, on when off.
fn tray_follow(app: &AppHandle) {
    let state = app.state::<PresenceState>();
    let status = state.status();
    if status.settings.follow_game && !status.following {
        state.follow_now();
    } else {
        let settings = presence::PresenceSettings { follow_game: !status.settings.follow_game, ..status.settings };
        match presence::save_settings(&app.state::<Db>(), settings) {
            Ok(saved) => state.settings_changed(saved),
            Err(e) => eprintln!("tennoworth: tray follow setting failed: {e}"),
        }
    }
    rebuild_tray(app);
}

/// The human labels a menu built from `top` shows (for evidence / the probe).
fn sellable_labels(top: &[SellableRow]) -> Vec<String> {
    if top.is_empty() {
        return vec![EMPTY_SELLABLES.to_string()];
    }
    top.iter()
        .map(|r| format!("{} - {}p", r.name, r.price.round() as i64))
        .collect()
}

/// Recompute the ranking and swap the tray menu in. Best-effort at every step:
/// a menu-build error or a missing tray (init failed / de-scoped) is logged and
/// swallowed - the window and notifications must keep working regardless. Called
/// at startup, after each scan, and after a market refresh. Returns the full
/// ranked list so a caller (the scan path) can reuse it for the post-scan payload.
pub fn rebuild_tray(app: &AppHandle) -> Vec<SellableRow> {
    let rows = rank_all(app);
    let top: Vec<SellableRow> = rows.iter().take(TRAY_LIMIT).cloned().collect();
    let labels = sellable_labels(&top);
    *guard(&app.state::<TrayState>().labels) = labels.clone();
    match build_tray_menu(app, &top, &labels) {
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

/// After a successful scan: rebuild the tray off the new snapshot and record
/// the payload the scan produced - but only when something is actually
/// sellable. An empty result records nothing (build_notification returns None).
/// A completed scan deliberately produces no notification: the inbox entry and
/// its desktop popup were removed as noise beside the daily sell digest.
pub fn post_scan_surfaces(app: &AppHandle) {
    let rows = rebuild_tray(app);
    if let Some(n) = sellables::build_notification(&rows) {
        *guard(&app.state::<TrayState>().last_notification) = Some(n);
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
/// tray + notification, then hand the payload to the webview. Runs on its own
/// thread (the menu-event callback must not block), and mirrors what the
/// SPA-driven `scan_inventory` command does. A scan error is logged, not
/// surfaced (there's no banner behind a tray click).
fn tray_rescan(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || match scan_and_record(&app) {
        Ok(payload) => {
            post_scan_surfaces(&app);
            publish_scan(&app, &payload);
        }
        Err(e) => eprintln!("tennoworth: tray rescan failed: {e}"),
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
    let labels = sellable_labels(&top);
    *guard(&app.state::<TrayState>().labels) = labels.clone();
    let menu = build_tray_menu(app, &top, &labels)?;
    let colour = crate::shell::app_icon::stored(&app.state::<Db>());
    TrayIconBuilder::with_id("main")
        .icon(colour.tray_image())
        .tooltip("TennoWorth - what to sell right now")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main_window(app),
            "rescan" => tray_rescan(app),
            "quit" => app.exit(0),
            "presence:online" => tray_presence(app, PresenceChoice::Online),
            "presence:ingame" => tray_presence(app, PresenceChoice::Ingame),
            "presence:invisible" => tray_presence(app, PresenceChoice::Invisible),
            "presence:follow" => tray_follow(app),
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
