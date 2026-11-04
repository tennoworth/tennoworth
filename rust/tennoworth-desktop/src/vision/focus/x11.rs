use super::{is_game_window, GameFocus, GameWindow};
use xcb::x::{
    Atom, Drawable, GetGeometry, GetProperty, InternAtom, TranslateCoordinates, Window,
    ATOM_CARDINAL, ATOM_NONE, ATOM_STRING, ATOM_WINDOW, ATOM_WM_NAME,
};
use xcb::{Connection, Xid};

/// Read the foreground window from the X11 (or XWayland) display. Under a
/// Wayland session the game runs through XWayland; when a native Wayland
/// window is in front, KWin points `_NET_ACTIVE_WINDOW` at a window that is not
/// the game, so the answer is still "background", not the stale game window.
pub(crate) fn game_focus() -> GameFocus {
    let Ok((connection, screen)) = Connection::connect(None) else {
        return GameFocus::Unknown;
    };
    let Some(root) = connection
        .get_setup()
        .roots()
        .nth(usize::try_from(screen).unwrap_or(0))
        .map(|s| s.root())
    else {
        return GameFocus::Unknown;
    };
    let Some(active_atom) = atom(&connection, b"_NET_ACTIVE_WINDOW") else {
        return GameFocus::Unknown;
    };
    let Some(reply) = property(&connection, root, active_atom, ATOM_WINDOW) else {
        return GameFocus::Unknown;
    };
    // No hint at all: the window manager does not publish focus.
    if reply.format() != 32 {
        return GameFocus::Unknown;
    }
    let Some(&active) = reply.value::<Window>().first() else {
        return GameFocus::Background;
    };
    if active.is_none() {
        return GameFocus::Background;
    }
    let pid = atom(&connection, b"_NET_WM_PID")
        .and_then(|a| property(&connection, active, a, ATOM_CARDINAL))
        .filter(|r| r.format() == 32)
        .and_then(|r| r.value::<u32>().first().copied());
    let title = window_title(&connection, active);
    if !is_game_window(pid, &title, game_acquisition::scan::find_wf_pid()) {
        return GameFocus::Background;
    }
    match geometry(&connection, active) {
        Some(window) => GameFocus::Focused(window),
        None => GameFocus::Unknown,
    }
}

fn atom(connection: &Connection, name: &[u8]) -> Option<Atom> {
    let cookie = connection.send_request(&InternAtom {
        only_if_exists: true,
        name,
    });
    connection
        .wait_for_reply(cookie)
        .ok()
        .map(|r| r.atom())
        .filter(|a| *a != ATOM_NONE)
}

fn property(
    connection: &Connection,
    window: Window,
    property: Atom,
    property_type: Atom,
) -> Option<xcb::x::GetPropertyReply> {
    let cookie = connection.send_request(&GetProperty {
        delete: false,
        window,
        property,
        r#type: property_type,
        long_offset: 0,
        long_length: 1024,
    });
    connection.wait_for_reply(cookie).ok()
}

fn window_title(connection: &Connection, window: Window) -> String {
    let utf8 = atom(connection, b"UTF8_STRING");
    let net_wm_name = atom(connection, b"_NET_WM_NAME");
    let named = utf8
        .zip(net_wm_name)
        .and_then(|(utf8, name)| property(connection, window, name, utf8))
        .map(|r| String::from_utf8_lossy(r.value()).into_owned())
        .filter(|t| !t.is_empty());
    named.unwrap_or_else(|| {
        property(connection, window, ATOM_WM_NAME, ATOM_STRING)
            .map(|r| String::from_utf8_lossy(r.value()).into_owned())
            .unwrap_or_default()
    })
}

fn geometry(connection: &Connection, window: Window) -> Option<GameWindow> {
    let geometry = connection
        .wait_for_reply(connection.send_request(&GetGeometry {
            drawable: Drawable::Window(window),
        }))
        .ok()?;
    let position = connection
        .wait_for_reply(connection.send_request(&TranslateCoordinates {
            src_window: window,
            dst_window: geometry.root(),
            src_x: 0,
            src_y: 0,
        }))
        .ok()?;
    Some(GameWindow {
        x: i32::from(position.dst_x()),
        y: i32::from(position.dst_y()),
        width: u32::from(geometry.width()),
        height: u32::from(geometry.height()),
    })
}
