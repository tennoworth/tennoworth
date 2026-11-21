//! Steam Gaming Mode runs every app inside gamescope. Apps there get an
//! XWayland `DISPLAY` and `GAMESCOPE_WAYLAND_DISPLAY`, but no
//! `WAYLAND_DISPLAY`, and gamescope's window manager publishes no client
//! list and no stacking hints, so desktop assumptions about windows fail there.

/// Inside gamescope, as Steam's Gaming Mode or a nested session.
pub(crate) fn active() -> bool {
    std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty())
}

/// The Wayland display for layer-shell overlays: the session's own, else
/// gamescope's, which offers layer-shell although apps are not pointed at it.
#[cfg(target_os = "linux")]
pub(crate) fn overlay_wayland_display() -> Option<std::ffi::OsString> {
    overlay_display_from(
        std::env::var_os("WAYLAND_DISPLAY"),
        std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY"),
    )
}

#[cfg(target_os = "linux")]
fn overlay_display_from(
    wayland: Option<std::ffi::OsString>,
    gamescope: Option<std::ffi::OsString>,
) -> Option<std::ffi::OsString> {
    let set = |value: &std::ffi::OsString| !value.is_empty();
    wayland.filter(set).or_else(|| gamescope.filter(set))
}

/// Where a Wayland display name's socket lives: an absolute name is the path,
/// anything else is relative to `XDG_RUNTIME_DIR`, as libwayland resolves it.
#[cfg(target_os = "linux")]
pub(crate) fn wayland_socket_path(
    display: &std::ffi::OsStr,
    runtime_dir: Option<std::ffi::OsString>,
) -> Option<std::path::PathBuf> {
    let display = std::path::Path::new(display);
    if display.is_absolute() {
        return Some(display.to_path_buf());
    }
    runtime_dir
        .filter(|dir| !dir.is_empty())
        .map(|dir| std::path::PathBuf::from(dir).join(display))
}

/// A Wayland connection to gamescope's own display, which Gaming Mode apps
/// are not pointed at but which offers layer-shell and gamescope's protocols.
#[cfg(target_os = "linux")]
pub(crate) fn connect_wayland() -> Result<wayland_client::Connection, String> {
    let display = std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "not running under gamescope".to_string())?;
    let path = wayland_socket_path(&display, std::env::var_os("XDG_RUNTIME_DIR"))
        .ok_or_else(|| "no XDG_RUNTIME_DIR for the gamescope Wayland socket".to_string())?;
    let stream = std::os::unix::net::UnixStream::connect(&path)
        .map_err(|error| format!("connecting to gamescope's Wayland socket: {error}"))?;
    wayland_client::Connection::from_socket(stream)
        .map_err(|error| format!("connecting to gamescope's Wayland socket: {error}"))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::{overlay_display_from, wayland_socket_path};
    use std::ffi::OsString;
    use std::path::PathBuf;

    #[test]
    fn the_session_display_wins_over_gamescope() {
        let chosen = overlay_display_from(Some("wayland-1".into()), Some("gamescope-0".into()));
        assert_eq!(chosen, Some(OsString::from("wayland-1")));
    }

    #[test]
    fn gaming_mode_falls_back_to_the_gamescope_display() {
        let chosen = overlay_display_from(None, Some("gamescope-0".into()));
        assert_eq!(chosen, Some(OsString::from("gamescope-0")));
        let chosen = overlay_display_from(Some(OsString::new()), Some("gamescope-0".into()));
        assert_eq!(chosen, Some(OsString::from("gamescope-0")));
    }

    #[test]
    fn no_wayland_display_means_no_layer_shell_attempt() {
        assert_eq!(overlay_display_from(None, None), None);
        assert_eq!(overlay_display_from(None, Some(OsString::new())), None);
    }

    #[test]
    fn socket_paths_resolve_like_libwayland() {
        assert_eq!(
            wayland_socket_path("gamescope-0".as_ref(), Some("/run/user/1000".into())),
            Some(PathBuf::from("/run/user/1000/gamescope-0"))
        );
        assert_eq!(
            wayland_socket_path("/tmp/gs/socket".as_ref(), None),
            Some(PathBuf::from("/tmp/gs/socket"))
        );
        assert_eq!(wayland_socket_path("gamescope-0".as_ref(), None), None);
    }
}
