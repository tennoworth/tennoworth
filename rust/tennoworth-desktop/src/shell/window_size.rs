//! The main window's opening and minimum size, in logical pixels.
//!
//! The opening height shows the whole sidebar, every view and the foot, so a
//! new window never hides a view behind a scroll. People can still make it
//! smaller. `tests/fixtures/main-window.json` holds the same numbers for the
//! browser check that the sidebar fits at this size.

use tauri::{LogicalSize, WebviewWindow};

pub(crate) const WIDTH: f64 = 1200.0;
pub(crate) const HEIGHT: f64 = 880.0;
/// Half a 1920px screen, so snapping still works. The status strip fits one
/// line at this width in every state; narrower, its fullest state would clip.
pub(crate) const MIN_WIDTH: f64 = 1200.0;
pub(crate) const MIN_HEIGHT: f64 = 480.0;

/// The opening height on a screen whose usable area leaves `available`
/// logical pixels for the page: the full height where it fits, never below
/// the minimum.
fn opening_height(available: f64) -> f64 {
    HEIGHT.min(available).max(MIN_HEIGHT)
}

/// Shrinks a window that opened taller than the screen's usable area (a 768px
/// laptop, a tall panel), so its foot is not drawn off screen.
pub(crate) fn fit_to_screen(window: &WebviewWindow) {
    let Ok(Some(monitor)) = window.current_monitor() else { return };
    let scale = monitor.scale_factor();
    let area = monitor.work_area().size.to_logical::<f64>(scale);
    let chrome = match (window.outer_size(), window.inner_size()) {
        (Ok(outer), Ok(inner)) => f64::from(outer.height.saturating_sub(inner.height)) / scale,
        _ => 0.0,
    };
    let height = opening_height(area.height - chrome);
    if height < HEIGHT {
        let _ = window.set_size(LogicalSize::new(WIDTH, height));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_match_the_browser_fixture() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../../tests/fixtures/main-window.json")).unwrap();
        for (key, value) in [("width", WIDTH), ("height", HEIGHT), ("min_width", MIN_WIDTH), ("min_height", MIN_HEIGHT)] {
            assert_eq!(fixture[key].as_f64(), Some(value), "{key}");
        }
    }

    #[test]
    fn opening_height_fits_the_screen_but_not_below_the_minimum() {
        assert_eq!(opening_height(1400.0), HEIGHT);
        assert_eq!(opening_height(700.0), 700.0);
        assert_eq!(opening_height(300.0), MIN_HEIGHT);
    }
}
