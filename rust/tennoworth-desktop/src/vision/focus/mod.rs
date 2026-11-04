//! Whether the game is the window the player is looking at, and where it is.
//!
//! In-game prompts are drawn only over a focused game. "The game is running"
//! is not the same answer: a player reading a browser on a second screen must
//! get an ordinary notification, not a card over a game they cannot see.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GameWindow {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GameFocus {
    /// The game owns the foreground window, at this screen rectangle.
    Focused(GameWindow),
    /// Something else is in front.
    Background,
    /// This desktop cannot say (no X11 display, no active-window hint).
    /// Callers treat it as background: a card the player cannot see is worse
    /// than a notification they can.
    Unknown,
}

/// Is the foreground window (`pid`, `title`) the game's? Only the game's own
/// process or its exact title counts: the capture fallback "title contains
/// warframe" would match a browser open on warframe.market.
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn is_game_window(pid: Option<u32>, title: &str, game_pid: Option<u32>) -> bool {
    use super::capture::{pick_game_window, WindowRule};
    matches!(
        pick_game_window(&[(pid, title.to_string())], game_pid),
        Some((_, WindowRule::GamePid | WindowRule::ExactTitle))
    )
}

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub(crate) use windows::game_focus;
#[cfg(target_os = "linux")]
mod x11;
#[cfg(target_os = "linux")]
pub(crate) use x11::game_focus;

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub(crate) fn game_focus() -> GameFocus {
    GameFocus::Unknown
}

#[cfg(all(test, any(target_os = "windows", target_os = "linux")))]
mod tests {
    use super::is_game_window;

    #[test]
    fn the_game_process_or_its_exact_title_is_the_game() {
        assert!(is_game_window(Some(42), "anything", Some(42)));
        assert!(is_game_window(None, "Warframe", None));
        assert!(is_game_window(Some(7), " warframe ", Some(42)));
    }

    #[test]
    fn a_browser_on_warframe_market_is_not_the_game() {
        assert!(!is_game_window(
            Some(7),
            "Warframe Market - Mozilla Firefox",
            Some(42)
        ));
        assert!(!is_game_window(None, "warframe.market", None));
        assert!(!is_game_window(None, "", None));
    }
}
