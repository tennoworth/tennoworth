use std::fs;
use std::path::Path;

/// Every IPC command the app registers. Declaring them makes Tauri check app
/// commands against capabilities, so each window reaches only what its
/// capability grants: the reward overlay, which renders game-adjacent content,
/// cannot place orders or read the sign-in state. `tests/command_acl.rs` keeps
/// this list, the handler and the capabilities in step.
const COMMANDS: &[&str] = &[
    "set_app_icon",
    "update_notes",
    "acknowledge_update_notes",
    "update_notes_can_present",
    "health",
    "get_usage_preferences",
    "set_usage_preferences",
    "get_price_report_preferences",
    "set_price_report_preferences",
    "erase_price_reports",
    "wfm_access_status",
    "evaluate_domain",
    "scan_inventory",
    "import_snapshot",
    "get_auto_scan_settings",
    "update_auto_scan_settings",
    "auto_scan_status",
    "set_auto_scan_hold",
    "presence_status",
    "set_presence",
    "update_presence_settings",
    "follow_game_now",
    "get_setting",
    "set_setting",
    "protection_state",
    "save_protection_plan",
    "list_snapshots",
    "cached_market",
    "get_announcements",
    "refresh_market",
    "top_sellables",
    "live_top_prices",
    "riven_comps",
    "cached_history",
    "refresh_history",
    "list_notifications",
    "mark_notifications_read",
    "clear_notifications",
    "get_notification_preferences",
    "set_notification_preferences",
    "test_notification",
    "list_watches",
    "add_watch",
    "delete_watch",
    "check_watches_now",
    "list_trades",
    "eelog_status",
    "listing_outcomes",
    "set_listing_outcomes",
    "trade_session_state",
    "tray_state",
    "check_update",
    "update_status",
    "install_update",
    "restart_app",
    "wfm_auth_status",
    "wfm_login",
    "wfm_login_cancel",
    "wfm_login_with_token",
    "unlock_jwt",
    "try_silent_unlock",
    "wfm_remember_available",
    "wfm_logout",
    "submit_plan",
    "cancel_plan",
    "get_pending_plan",
    "discard_pending_plan",
    "resume_pending_plan",
    "fetch_orders",
    "update_order",
    "delete_order",
    "bulk_visibility",
    "open_external_url",
    "get_overlay_settings",
    "update_overlay_settings",
    "overlay_status",
    "current_overlay_result",
    "preview_relic_overlay",
    "setup_overlay_capture",
    "scan_overlay_now",
    "open_overlay_diagnostics",
    "clear_overlay_diagnostics",
    "debug_write_login",
    "debug_seed_unlocked",
    "debug_post_scan",
    "probe_report",
    "probe_exit",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // `tauri::generate_context!()` panics at COMPILE time if frontendDist
    // (../frontend/dist-desktop, gitignored) is missing - which it always is
    // on CI and fresh checkouts, breaking `cargo test --workspace` before any
    // test runs. A stub index.html satisfies the check; real builds overwrite
    // the directory via beforeBuildCommand.
    let dist = Path::new("../../frontend/dist-desktop");
    if !dist.exists() {
        fs::create_dir_all(dist)?;
        fs::write(
            dist.join("index.html"),
            "<!-- placeholder: run `bun run build:desktop` in frontend/ for the real bundle -->\n",
        )?;
    }
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )?;
    Ok(())
}
