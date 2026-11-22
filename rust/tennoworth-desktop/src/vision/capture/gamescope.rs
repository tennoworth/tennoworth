//! Capture under gamescope (Steam Gaming Mode). The game presents through
//! gamescope's own Vulkan path, so its XWayland window holds no pixels: an
//! X11 `GetImage` of it returns black even while it is in front. gamescope
//! writes a screenshot of just the game plane, at the game's own resolution,
//! when a client sets a property on the root window.
//!
//! The control protocol's screenshot request is not used: gamescope announces
//! those to every control client, and Steam imports each one into the game's
//! screenshot library. The root property is gamescope's own debug request,
//! which Steam does not watch.

use std::path::Path;
use std::time::{Duration, Instant};

use xcb::x::{ChangeProperty, GetProperty, InternAtom, PropMode, ATOM_ANY, ATOM_CARDINAL};

use super::CapturedFrame;

/// Where gamescope writes a root-property screenshot; the path is fixed.
const SCREENSHOT_PATH: &str = "/tmp/gamescope.png";
/// gamescope's `screenshot_type` for the game plane alone, without overlays.
const BASE_PLANE_ONLY: u32 = 1;
/// How long gamescope gets to write the frame; it does so on its next paint.
const SCREENSHOT_TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) fn capture_game() -> Result<CapturedFrame, String> {
    let (connection, screen_index) = xcb::Connection::connect(None)
        .map_err(|error| format!("capture_failed: connecting to gamescope's X server: {error}"))?;
    let root = connection
        .get_setup()
        .roots()
        .nth(screen_index as usize)
        .ok_or_else(|| "capture_failed: gamescope's X server has no screen".to_string())?
        .root();
    let request = connection
        .wait_for_reply(connection.send_request(&InternAtom {
            only_if_exists: false,
            name: b"GAMESCOPECTRL_REQUEST_SCREENSHOT",
        }))
        .map_err(|error| format!("capture_failed: resolving gamescope's screenshot atom: {error}"))?
        .atom();
    let path = Path::new(SCREENSHOT_PATH);
    // A file left by an earlier screenshot must not pass for this one.
    let _ = std::fs::remove_file(path);
    connection.send_request(&ChangeProperty {
        mode: PropMode::Replace,
        window: root,
        property: request,
        r#type: ATOM_CARDINAL,
        data: &[BASE_PLANE_ONLY],
    });
    connection
        .flush()
        .map_err(|error| format!("capture_failed: asking gamescope for a screenshot: {error}"))?;
    // gamescope deletes the property once the file is written.
    let deadline = Instant::now() + SCREENSHOT_TIMEOUT;
    loop {
        let pending = connection
            .wait_for_reply(connection.send_request(&GetProperty {
                delete: false,
                window: root,
                property: request,
                r#type: ATOM_ANY,
                long_offset: 0,
                long_length: 1,
            }))
            .map_err(|error| format!("capture_failed: waiting for gamescope: {error}"))?
            .format()
            != 0;
        if !pending && path.exists() {
            break;
        }
        if Instant::now() >= deadline {
            return Err("capture_failed: gamescope did not write the screenshot in time".into());
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    let decoded = image::ImageReader::open(path)
        .and_then(|reader| reader.with_guessed_format())
        .map_err(|error| format!("capture_failed: reading gamescope's screenshot: {error}"))
        .and_then(|reader| {
            reader.decode().map_err(|error| {
                format!("capture_failed: decoding gamescope's screenshot: {error}")
            })
        });
    let _ = std::fs::remove_file(path);
    let image = decoded?.to_rgba8();
    // The game plane is already at the game's resolution, the space overlay
    // geometry is in; a different size is brought to it.
    let image = match super::warframe_window_size() {
        Some((game_width, game_height))
            if game_width >= 640
                && game_height >= 360
                && image.dimensions() != (game_width, game_height) =>
        {
            image::imageops::resize(
                &image,
                game_width,
                game_height,
                image::imageops::FilterType::Triangle,
            )
        }
        _ => image,
    };
    let (width, height) = image.dimensions();
    if width < 640 || height < 360 {
        return Err(format!(
            "capture_failed: Warframe capture is too small ({width}×{height})"
        ));
    }
    Ok(CapturedFrame {
        image,
        x: 0,
        y: 0,
        width,
        height,
    })
}
