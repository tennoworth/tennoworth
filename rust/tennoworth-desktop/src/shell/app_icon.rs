//! The App icon setting: which colour the main window and tray icons use.
//!
//! The webview owns the user's choice ('match' follows its colour mode) and
//! sends the resolved colour here. The applied colour is persisted so the next
//! launch builds the window and tray with it before the webview loads. The
//! installed icons (shortcut, installer, launcher) are always classic blue.
#![allow(
    clippy::unreachable,
    reason = "tauri::command injects unreachable code into async wrappers"
)]

use tauri::image::Image;
use tauri::{AppHandle, Manager};

use crate::command_error::CmdError;
use crate::persistence::Db;

/// The applied colour's `setting` row. The generic `set_setting` command
/// refuses this prefix, so only [`set_app_icon`] writes a validated value.
pub const SETTING_APPLIED: &str = "app-icon.applied";
pub const RESERVED_PREFIX: &str = "app-icon.";

/// The colours the native side can draw, in the order the setting offers them.
pub const COLOURS: [&str; 3] = ["blue", "ink", "rag"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppIconColour {
    Blue,
    Ink,
    Rag,
}

impl AppIconColour {
    pub fn parse(value: &str) -> Option<Self> {
        COLOURS
            .into_iter()
            .zip([Self::Blue, Self::Ink, Self::Rag])
            .find_map(|(name, colour)| (name == value).then_some(colour))
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Blue => "blue",
            Self::Ink => "ink",
            Self::Rag => "rag",
        }
    }

    /// The compact drawing at 32px, which stays legible at tray sizes.
    pub fn tray_image(self) -> Image<'static> {
        match self {
            Self::Blue => tauri::include_image!("icons/32x32.png"),
            Self::Ink => tauri::include_image!("icons/app-icon/ink-32.png"),
            Self::Rag => tauri::include_image!("icons/app-icon/rag-32.png"),
        }
    }

    /// Blue is the bundle's default window icon, so choosing it restores
    /// exactly what an install without the setting shows.
    pub fn window_image(self, app: &AppHandle) -> Option<Image<'static>> {
        match self {
            Self::Blue => app.default_window_icon().map(|icon| icon.clone().to_owned()),
            Self::Ink => Some(tauri::include_image!("icons/app-icon/ink-64.png")),
            Self::Rag => Some(tauri::include_image!("icons/app-icon/rag-64.png")),
        }
    }
}

/// The persisted colour; classic blue when unset, unreadable or unknown.
pub fn stored(db: &Db) -> AppIconColour {
    db.get_setting(SETTING_APPLIED)
        .ok()
        .flatten()
        .and_then(|value| AppIconColour::parse(&value))
        .unwrap_or(AppIconColour::Blue)
}

/// Validates before writing, so an unknown colour never reaches the store.
fn persist(db: &Db, colour: &str) -> Result<AppIconColour, CmdError> {
    let parsed = AppIconColour::parse(colour)
        .ok_or_else(|| CmdError::of("invalid", format!("Unknown app icon colour: {colour}")))?;
    db.set_setting(SETTING_APPLIED, parsed.as_str())
        .map_err(CmdError::internal)?;
    Ok(parsed)
}

/// What a colour change can reach on this desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct AppIconOutcome {
    /// False on a GTK Wayland session: Wayland compositors take the window and
    /// taskbar icon from the installed launcher entry, and GTK 3 has no way to
    /// set one per window there. The tray still changes.
    pub window_icon: bool,
}

fn window_icon_supported() -> bool {
    if cfg!(target_os = "linux") {
        // GTK picks Wayland whenever a Wayland display is present unless
        // GDK_BACKEND forces X11 (XWayland honours _NET_WM_ICON).
        let forced_x11 = std::env::var("GDK_BACKEND").is_ok_and(|b| b.starts_with("x11"));
        return std::env::var_os("WAYLAND_DISPLAY").is_none() || forced_x11;
    }
    true
}

/// Sets the main window and tray icons. Best-effort: either may be missing
/// (tray init failed, window not built yet) and a failed set keeps the old icon.
pub fn apply(app: &AppHandle, colour: AppIconColour) {
    if let Some(window) = app.get_webview_window("main") {
        apply_to_window(&window, colour, app);
    }
    if let Some(tray) = app.tray_by_id("main") {
        if let Err(e) = tray.set_icon(Some(colour.tray_image())) {
            eprintln!("tennoworth: tray icon change failed: {e}");
        }
    }
}

/// The window's icons: Tauri's own small one, and on Windows the big one.
pub fn apply_to_window(window: &tauri::WebviewWindow, colour: AppIconColour, app: &AppHandle) {
    if let Some(image) = colour.window_image(app) {
        if let Err(e) = window.set_icon(image) {
            eprintln!("tennoworth: window icon change failed: {e}");
        }
    }
    #[cfg(windows)]
    {
        // Blue clears the big icon so Windows falls back to the executable's
        // own, exactly what an install without the setting shows.
        let big = match colour {
            AppIconColour::Blue => None,
            AppIconColour::Ink | AppIconColour::Rag => colour.window_image(app),
        };
        if let Err(e) = taskbar::set(window, big.as_ref()) {
            eprintln!("tennoworth: taskbar icon change failed: {e}");
        }
    }
}

/// Windows draws the taskbar button and Alt+Tab from the window's ICON_BIG,
/// which Tauri does not expose. This mirrors tao's own `set_taskbar_icon`.
#[cfg(windows)]
mod taskbar {
    use std::sync::Mutex;

    use tauri::image::Image;
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateIcon, DestroyIcon, SendMessageW, HICON, ICON_BIG, WM_SETICON,
    };

    /// The big icon handle the window holds; destroyed once replaced, never
    /// while the window can still draw it.
    static CURRENT: Mutex<Option<isize>> = Mutex::new(None);

    pub fn set(window: &tauri::WebviewWindow, image: Option<&Image<'_>>) -> Result<(), String> {
        let hwnd = window.hwnd().map_err(|e| e.to_string())?;
        let next = image.map(create).transpose()?;
        // SAFETY: hwnd is this process's live main window; WM_SETICON with a
        // valid HICON or 0 (restore the class icon) has no other requirement.
        unsafe {
            SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(ICON_BIG as usize)),
                Some(LPARAM(next.map_or(0, |icon| icon.0 as isize))),
            );
        }
        let mut current = CURRENT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(old) = std::mem::replace(&mut *current, next.map(|icon| icon.0 as isize)) {
            // SAFETY: `old` came from CreateIcon below and the window no longer uses it.
            unsafe {
                let _ = DestroyIcon(HICON(old as _));
            }
        }
        Ok(())
    }

    fn create(image: &Image<'_>) -> Result<HICON, String> {
        let mut bgra = image.rgba().to_vec();
        let mut and_mask = Vec::with_capacity(bgra.len() / 4);
        for pixel in bgra.chunks_exact_mut(4) {
            and_mask.push(pixel[3].wrapping_sub(u8::MAX));
            pixel.swap(0, 2);
        }
        let (width, height) = (
            i32::try_from(image.width()).map_err(|e| e.to_string())?,
            i32::try_from(image.height()).map_err(|e| e.to_string())?,
        );
        // SAFETY: both buffers hold width * height pixels in the layout
        // CreateIcon documents for a 32-bit colour icon, as tao builds them.
        unsafe { CreateIcon(None, width, height, 1, 32, and_mask.as_ptr(), bgra.as_ptr()) }
            .map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub fn set_app_icon(app: AppHandle, colour: String) -> Result<AppIconOutcome, CmdError> {
    let parsed = persist(&app.state::<Db>(), &colour)?;
    apply(&app, parsed);
    Ok(AppIconOutcome { window_icon: window_icon_supported() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_colours_are_rejected_and_not_stored() {
        let db = Db::open_in_memory().unwrap();
        persist(&db, "ink").unwrap();
        for bad in ["", "match", "purple", "Ink", " rag"] {
            let err = persist(&db, bad).unwrap_err();
            assert_eq!(err.code, "invalid", "{bad:?}");
        }
        assert_eq!(stored(&db), AppIconColour::Ink);
    }

    #[test]
    fn the_applied_colour_survives_a_restart() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(stored(&db), AppIconColour::Blue);
        for colour in COLOURS {
            persist(&db, colour).unwrap();
            assert_eq!(stored(&db).as_str(), colour);
        }
    }

    #[test]
    fn an_unreadable_stored_value_falls_back_to_blue() {
        let db = Db::open_in_memory().unwrap();
        db.set_setting(SETTING_APPLIED, "teal").unwrap();
        assert_eq!(stored(&db), AppIconColour::Blue);
    }
}
