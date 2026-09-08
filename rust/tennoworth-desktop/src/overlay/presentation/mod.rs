use super::{OverlayState, RelicOverlayResult, EVENT_HIDE, EVENT_UPDATE};
use std::sync::mpsc;
use std::time::Duration;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder,
};
#[cfg(target_os = "linux")]
pub(crate) mod wayland;

pub(super) fn present_overlay(
    app: &AppHandle,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    result: &RelicOverlayResult,
) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        let geometry = wayland::OverlayGeometry {
            x,
            y,
            width,
            height,
        };
        match wayland::show(geometry, result) {
            Ok(()) => {
                if let Some(window) = app.get_webview_window("relic-overlay") {
                    let _ = window.hide();
                }
                if let Some(state) = app.try_state::<OverlayState>() {
                    state.set_presentation_backend(app, "wayland-layer-shell");
                }
                return Ok(());
            }
            Err(error) => {
                eprintln!("tennoworth: native Wayland overlay unavailable, using window fallback: {error}");
                if let Some(state) = app.try_state::<OverlayState>() {
                    state.set_presentation_backend(app, "tauri-window");
                }
            }
        }
    }
    show_tauri_overlay(app, x, y, width, height)?;
    push_tauri_overlay_result(app, result)
}

pub(super) fn show_tauri_overlay(
    app: &AppHandle,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let app_for_window = app.clone();
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    app.run_on_main_thread(move || {
        let result = show_overlay_on_main_thread(&app_for_window, x, y, width, height);
        let _ = reply_tx.send(result);
    })
    .map_err(|e| format!("scheduling overlay window: {e}"))?;
    reply_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "timed out creating overlay window".to_string())?
}

pub(super) fn show_overlay_on_main_thread(
    app: &AppHandle,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let window = ensure_overlay_window(app)?;
    window
        .set_always_on_top(true)
        .map_err(|e| format!("making overlay topmost: {e}"))?;
    window
        .set_focusable(false)
        .map_err(|e| format!("making overlay non-activating: {e}"))?;
    configure_linux_overlay_focus(&window)?;
    window
        .set_size(PhysicalSize::new(width, height))
        .map_err(|e| format!("sizing overlay: {e}"))?;
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|e| format!("positioning overlay: {e}"))?;
    window.show().map_err(|e| format!("showing overlay: {e}"))?;
    // A hidden topmost window can lose its z-band while a borderless game is
    // active. Reassert after mapping without activating or stealing focus.
    window
        .set_always_on_top(true)
        .map_err(|e| format!("raising overlay above the game: {e}"))?;
    window
        .set_ignore_cursor_events(true)
        .map_err(|e| format!("making overlay click-through: {e}"))?;
    Ok(())
}

pub(super) fn ensure_overlay_window(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
    let window = match app.get_webview_window("relic-overlay") {
        Some(window) => window,
        None => WebviewWindowBuilder::new(
            app,
            "relic-overlay",
            WebviewUrl::App("index.html?surface=relic-overlay".into()),
        )
        .title("TennoWorth relic overlay")
        .transparent(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focusable(false)
        .focused(false)
        .shadow(false)
        .resizable(false)
        .visible(false)
        .build()
        .map_err(|e| format!("creating overlay window: {e}"))?,
    };
    Ok(window)
}

pub(super) fn push_overlay_result(
    app: &AppHandle,
    result: &RelicOverlayResult,
) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if wayland::is_active() && wayland::update(result)? {
        return Ok(());
    }
    push_tauri_overlay_result(app, result)
}

pub(super) fn push_tauri_overlay_result(
    app: &AppHandle,
    result: &RelicOverlayResult,
) -> Result<(), String> {
    let event_error = app
        .emit_to("relic-overlay", EVENT_UPDATE, result)
        .err()
        .map(|error| error.to_string());
    let eval_error = app
        .get_webview_window("relic-overlay")
        .ok_or_else(|| "overlay window is missing".to_string())
        .and_then(|window| {
            let payload = serde_json::to_string(result)
                .map_err(|error| format!("encoding overlay result: {error}"))?;
            window
                .eval(format!(
                    "window.__TENNOWORTH_RELIC_OVERLAY_UPDATE__?.({payload})"
                ))
                .map_err(|error| format!("updating overlay webview: {error}"))
        })
        .err();
    if let Some(eval_error) = eval_error {
        return Err(format!(
            "cached_overlay_display_failed: bridge={eval_error}; event={}",
            event_error.unwrap_or_else(|| "sent (listener delivery unverified)".into())
        ));
    }
    Ok(())
}

pub(crate) fn prewarm_overlay_window(app: &AppHandle) {
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        if wayland::available() {
            return;
        }
        if let Some(state) = app.try_state::<OverlayState>() {
            state.set_presentation_backend(app, "tauri-window");
        }
    }
    if let Err(error) = ensure_overlay_window(app) {
        eprintln!("tennoworth: could not prewarm relic overlay: {error}");
    }
}

#[cfg(target_os = "linux")]
pub(super) fn configure_linux_overlay_focus(window: &tauri::WebviewWindow) -> Result<(), String> {
    use gtk::prelude::{GtkWindowExt, WidgetExt};

    let native = window
        .gtk_window()
        .map_err(|e| format!("accessing native overlay window: {e}"))?;
    native.set_accept_focus(false);
    native.set_focus_on_map(false);
    native.set_can_focus(false);
    native.set_type_hint(gtk::gdk::WindowTypeHint::Toolbar);
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub(super) fn configure_linux_overlay_focus(_window: &tauri::WebviewWindow) -> Result<(), String> {
    Ok(())
}

pub(super) fn hide_overlay(app: &AppHandle) {
    #[cfg(target_os = "linux")]
    wayland::hide();
    if let Some(window) = app.get_webview_window("relic-overlay") {
        let _ = app.emit_to("relic-overlay", EVENT_HIDE, ());
        let _ = window.eval("window.__TENNOWORTH_RELIC_OVERLAY_HIDE__?.()");
        let _ = window.hide();
    }
    if let Some(state) = app.try_state::<OverlayState>() {
        state.lifecycle.clear();
        *state
            .current_result
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
    }
}

pub(super) fn preferred_presentation_backend() -> &'static str {
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return "wayland-layer-shell";
    }
    "tauri-window"
}
