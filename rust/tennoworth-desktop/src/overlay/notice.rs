//! In-game notices: a short card over the focused game for a notification the
//! player opted to see there.
//!
//! The card shares the overlay's guarantees (click-through, never focused,
//! never injects input) but not its surface: the reward card and the notice
//! each own a window/layer, so their hide timers cannot cancel one another.
//! A reward screen always wins - presenting a reward card hides the notice,
//! and no notice is shown while a reward card is up.
//!
//! There is no queue. A newer notice replaces the one on screen, and one that
//! cannot be shown now (reward card up, hourly cap reached, overlay failed) is
//! delivered the ordinary way instead, so nothing is shown late or stale.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl, WebviewWindowBuilder,
};

use super::OverlayState;
use crate::services::notifications::InGameRequest;
use crate::vision::focus::{GameFocus, GameWindow};

/// The card's design size; both renderers lay out on it and scale.
pub const NOTICE_WIDTH: f64 = 420.0;
pub const NOTICE_HEIGHT: f64 = 88.0;
/// Long enough to read a name and a price, short enough not to linger over play.
const SHOW_FOR: Duration = Duration::from_secs(10);
/// A busy market must not turn the overlay into a ticker.
pub const MAX_PER_HOUR: usize = 20;
const WINDOW_LABEL: &str = "game-notice";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ts_rs::TS)]
pub struct GameNotice {
    pub id: String,
    /// The kind of notice, e.g. "Price watch".
    pub title: String,
    /// The evidence, e.g. "Ash Prime Set is listed at 110p by PlayerX".
    pub detail: String,
    /// What to do next, e.g. "Whisper copied - press Ctrl+V in chat".
    pub hint: Option<String>,
}

#[derive(Default)]
pub struct NoticeState {
    current: Mutex<Option<GameNotice>>,
    generation: AtomicU64,
    shown: Mutex<VecDeque<Instant>>,
}

/// Why a notice was not shown in game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotShown {
    RewardCard,
    HourlyCap,
    Failed(String),
}

/// Count this showing against the hourly cap, or refuse it.
fn take_slot(shown: &mut VecDeque<Instant>, now: Instant) -> bool {
    while shown
        .front()
        .is_some_and(|at| now.saturating_duration_since(*at) >= Duration::from_secs(3600))
    {
        shown.pop_front();
    }
    if shown.len() >= MAX_PER_HOUR {
        return false;
    }
    shown.push_back(now);
    true
}

/// Where the card goes over the game: the top-right corner, below the area
/// the game uses for mission information, sized relative to a 1080p window.
pub(crate) fn card_geometry(game: GameWindow) -> (i32, i32, u32, u32) {
    let scale = (f64::from(game.height) / 1080.0).clamp(0.75, 3.0);
    let width = (NOTICE_WIDTH * scale).round().min(f64::from(game.width)) as u32;
    let height = (NOTICE_HEIGHT * scale).round().min(f64::from(game.height)) as u32;
    let margin = (24.0 * scale).round() as i32;
    let right = game
        .x
        .saturating_add(i32::try_from(game.width).unwrap_or(i32::MAX));
    let x = right
        .saturating_sub(i32::try_from(width).unwrap_or(0))
        .saturating_sub(margin)
        .max(game.x);
    let y = game
        .y
        .saturating_add((f64::from(game.height) * 0.12).round() as i32);
    (x, y, width, height)
}

/// In-game delivery for a notification: shown only while the game is the
/// focused window. False means "deliver it the ordinary way": the game is not
/// focused, this desktop cannot tell, a reward card is up, or the card failed.
pub(crate) fn deliver(app: &AppHandle, request: &InGameRequest<'_>) -> bool {
    let GameFocus::Focused(game) = crate::vision::focus::game_focus() else {
        return false;
    };
    let shown = present(app, game, || GameNotice {
        id: format!("notification-{}", request.id),
        title: request.title.into(),
        detail: request.detail.into(),
        hint: request.whisper.map(|whisper| {
            whisper_hint(request.copy_whisper && crate::services::clipboard::copy(whisper).is_ok())
        }),
    });
    match shown {
        Ok(()) => true,
        Err(reason) => {
            eprintln!("tennoworth: in-game notice not shown ({reason:?}); delivering normally");
            false
        }
    }
}

/// What the card tells the player to do with the whisper.
fn whisper_hint(copied: bool) -> String {
    if copied {
        "Whisper copied · press Ctrl+V in chat".into()
    } else {
        "Switch to TennoWorth to copy the whisper".into()
    }
}

/// Show a notice over the focused game window. `build` runs only once the card
/// is cleared to appear, so its side effects (copying a whisper) never happen
/// for a card the player will not see.
pub(crate) fn present(
    app: &AppHandle,
    game: GameWindow,
    build: impl FnOnce() -> GameNotice,
) -> Result<(), NotShown> {
    let relic_up = app.try_state::<OverlayState>().is_some_and(|state| {
        state
            .current_result
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    });
    if relic_up {
        return Err(NotShown::RewardCard);
    }
    let state = app.state::<NoticeState>();
    if !take_slot(
        &mut state.shown.lock().unwrap_or_else(|e| e.into_inner()),
        Instant::now(),
    ) {
        return Err(NotShown::HourlyCap);
    }
    let notice = build();
    let (x, y, width, height) = card_geometry(game);
    show(app, &notice, x, y, width, height).map_err(NotShown::Failed)?;
    *state.current.lock().unwrap_or_else(|e| e.into_inner()) = Some(notice);
    let generation = state.generation.fetch_add(1, Ordering::AcqRel) + 1;
    let timer_app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("game-notice-hide".into())
        .spawn(move || {
            std::thread::sleep(SHOW_FOR);
            let state = timer_app.state::<NoticeState>();
            // A newer notice (or an explicit hide) owns the surface now.
            if state.generation.load(Ordering::Acquire) == generation {
                hide(&timer_app);
            }
        });
    // A card nothing will take down must not stay over the game.
    if let Err(error) = spawned {
        hide(app);
        return Err(NotShown::Failed(format!(
            "starting the notice timer: {error}"
        )));
    }
    Ok(())
}

fn show(
    app: &AppHandle,
    notice: &GameNotice,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        let geometry = super::presentation::wayland::OverlayGeometry {
            x,
            y,
            width,
            height,
        };
        match super::presentation::wayland::show_notice(geometry, notice) {
            Ok(()) => return Ok(()),
            Err(error) => {
                eprintln!(
                    "tennoworth: native Wayland notice unavailable, using window fallback: {error}"
                );
            }
        }
    }
    show_window(app, notice, x, y, width, height)
}

/// Hide the notice card, wherever it is drawn. Called by its timer, and before
/// a reward card is presented.
pub(crate) fn hide(app: &AppHandle) {
    let Some(state) = app.try_state::<NoticeState>() else {
        return;
    };
    state.generation.fetch_add(1, Ordering::AcqRel);
    *state.current.lock().unwrap_or_else(|e| e.into_inner()) = None;
    #[cfg(target_os = "linux")]
    super::presentation::wayland::hide_notice();
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        let _ = window.eval("window.__TENNOWORTH_GAME_NOTICE__?.(null)");
        let _ = window.hide();
    }
}

#[tauri::command]
pub fn current_game_notice(state: State<'_, NoticeState>) -> Option<GameNotice> {
    state
        .current
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// The webview fallback (Windows, X11, and Wayland compositors without
/// layer-shell). Mirrors the reward window's passive, click-through setup.
fn show_window(
    app: &AppHandle,
    notice: &GameNotice,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let app_for_window = app.clone();
    let notice = notice.clone();
    let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel(1);
    app.run_on_main_thread(move || {
        let _ = reply_tx.send(show_window_on_main_thread(
            &app_for_window,
            &notice,
            x,
            y,
            width,
            height,
        ));
    })
    .map_err(|e| format!("scheduling notice window: {e}"))?;
    reply_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "timed out showing the notice window".to_string())?
}

fn show_window_on_main_thread(
    app: &AppHandle,
    notice: &GameNotice,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let window = match app.get_webview_window(WINDOW_LABEL) {
        Some(window) => window,
        None => WebviewWindowBuilder::new(
            app,
            WINDOW_LABEL,
            WebviewUrl::App("index.html?surface=game-notice".into()),
        )
        .title("TennoWorth in-game notice")
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
        .map_err(|e| format!("creating notice window: {e}"))?,
    };
    window
        .set_focusable(false)
        .map_err(|e| format!("making notice non-activating: {e}"))?;
    super::presentation::configure_linux_overlay_focus(&window)?;
    window
        .set_size(PhysicalSize::new(width, height))
        .map_err(|e| format!("sizing notice: {e}"))?;
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|e| format!("positioning notice: {e}"))?;
    let payload = serde_json::to_string(notice).map_err(|e| format!("encoding notice: {e}"))?;
    // The bridge may not exist yet on a cold window; the page then reads
    // `current_game_notice` once it mounts.
    let _ = window.eval(format!("window.__TENNOWORTH_GAME_NOTICE__?.({payload})"));
    window.show().map_err(|e| format!("showing notice: {e}"))?;
    window
        .set_always_on_top(true)
        .map_err(|e| format!("raising notice above the game: {e}"))?;
    window
        .set_ignore_cursor_events(true)
        .map_err(|e| format!("making notice click-through: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hourly_cap_refuses_the_next_notice_and_frees_up_after_an_hour() {
        let start = Instant::now();
        let mut shown = VecDeque::new();
        for _ in 0..MAX_PER_HOUR {
            assert!(take_slot(&mut shown, start));
        }
        assert!(!take_slot(&mut shown, start + Duration::from_secs(3599)));
        assert!(take_slot(&mut shown, start + Duration::from_secs(3600)));
    }

    #[test]
    fn the_card_sits_inside_the_game_window_top_right_and_scales_with_it() {
        let game = GameWindow {
            x: 100,
            y: 50,
            width: 1920,
            height: 1080,
        };
        let (x, y, width, height) = card_geometry(game);
        assert_eq!((width, height), (420, 88));
        assert_eq!(x, 100 + 1920 - 420 - 24);
        assert_eq!(y, 50 + 130);
        let big = card_geometry(GameWindow {
            x: 0,
            y: 0,
            width: 3840,
            height: 2160,
        });
        assert_eq!((big.2, big.3), (840, 176));
    }

    #[test]
    fn a_tiny_window_never_pushes_the_card_off_its_left_edge() {
        let (x, _, width, _) = card_geometry(GameWindow {
            x: 10,
            y: 0,
            width: 300,
            height: 200,
        });
        assert!(x >= 10);
        assert!(width <= 300);
    }
}
