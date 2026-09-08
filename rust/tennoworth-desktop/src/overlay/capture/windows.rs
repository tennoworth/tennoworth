use super::CapturedFrame;

pub(in crate::overlay) fn capture_warframe() -> Result<CapturedFrame, String> {
    let windows =
        xcap::Window::all().map_err(|e| format!("capture_failed: listing windows: {e}"))?;
    let window = windows
        .into_iter()
        .find(|window| {
            window
                .title()
                .ok()
                .is_some_and(|title| title.to_ascii_lowercase().contains("warframe"))
        })
        .ok_or_else(|| {
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                "window_not_found: Warframe window not found - capture runs through XWayland for now; run Warframe borderless/windowed with XWayland enabled"
                    .to_string()
            } else {
                "window_not_found: Warframe window not found; use borderless or windowed mode".to_string()
            }
        })?;
    let x = window
        .x()
        .map_err(|e| format!("capture_failed: reading Warframe position: {e}"))?;
    let y = window
        .y()
        .map_err(|e| format!("capture_failed: reading Warframe position: {e}"))?;
    let image = window
        .capture_image()
        .map_err(|e| format!("capture_failed: capturing Warframe (capture may be denied): {e}"))?;
    let width = image.width();
    let height = image.height();
    if width < 640 || height < 360 {
        return Err(format!(
            "capture_failed: Warframe capture is too small ({width}×{height})"
        ));
    }
    Ok(CapturedFrame {
        image,
        x,
        y,
        width,
        height,
    })
}
