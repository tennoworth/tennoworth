use super::CapturedFrame;
use image::RgbaImage;

#[allow(
    clippy::indexing_slicing,
    reason = "the reply length is checked against expected and chunks_exact guarantees each pixel has the validated 2, 3, or 4-byte width"
)]
pub(in crate::overlay) fn capture_warframe() -> Result<CapturedFrame, String> {
    use xcb::{
        x::{
            Atom, Drawable, GetGeometry, GetImage, GetProperty, ImageFormat, ImageOrder,
            InternAtom, TranslateCoordinates, Window, ATOM_NONE, ATOM_STRING, ATOM_WM_NAME,
        },
        Connection,
    };

    pub(in crate::overlay) fn atom(connection: &Connection, name: &[u8]) -> Result<Atom, String> {
        let cookie = connection.send_request(&InternAtom {
            only_if_exists: false,
            name,
        });
        connection
            .wait_for_reply(cookie)
            .map(|reply| reply.atom())
            .map_err(|error| format!("capture_failed: resolving X11 atom: {error}"))
    }

    pub(in crate::overlay) fn property(
        connection: &Connection,
        window: Window,
        property: Atom,
        property_type: Atom,
    ) -> Result<xcb::x::GetPropertyReply, String> {
        let cookie = connection.send_request(&GetProperty {
            delete: false,
            window,
            property,
            r#type: property_type,
            long_offset: 0,
            long_length: 4096,
        });
        connection
            .wait_for_reply(cookie)
            .map_err(|error| format!("capture_failed: reading X11 window property: {error}"))
    }

    let (connection, _) = Connection::connect(None)
        .map_err(|error| format!("capture_failed: connecting to X11/XWayland: {error}"))?;
    let client_list = atom(&connection, b"_NET_CLIENT_LIST_STACKING")?;
    let net_wm_name = atom(&connection, b"_NET_WM_NAME")?;
    let utf8_string = atom(&connection, b"UTF8_STRING")?;

    let mut warframe = None;
    for screen in connection.get_setup().roots() {
        let reply = match property(&connection, screen.root(), client_list, ATOM_NONE) {
            Ok(reply) => reply,
            Err(_) => continue,
        };
        for &window in reply.value::<Window>().iter().rev() {
            let mut title = property(&connection, window, net_wm_name, utf8_string)
                .ok()
                .map(|reply| String::from_utf8_lossy(reply.value()).into_owned())
                .unwrap_or_default();
            if title.is_empty() {
                title = property(&connection, window, ATOM_WM_NAME, ATOM_STRING)
                    .ok()
                    .map(|reply| String::from_utf8_lossy(reply.value()).into_owned())
                    .unwrap_or_default();
            }
            if title.to_ascii_lowercase().contains("warframe") {
                warframe = Some(window);
                break;
            }
        }
        if warframe.is_some() {
            break;
        }
    }

    let window = warframe.ok_or_else(|| {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            "window_not_found: Warframe window not found - capture runs through XWayland for now; run Warframe borderless/windowed with XWayland enabled"
                .to_string()
        } else {
            "window_not_found: Warframe window not found; use borderless or windowed mode".to_string()
        }
    })?;

    let geometry_cookie = connection.send_request(&GetGeometry {
        drawable: Drawable::Window(window),
    });
    let geometry = connection
        .wait_for_reply(geometry_cookie)
        .map_err(|error| format!("capture_failed: reading Warframe geometry: {error}"))?;
    let width = u32::from(geometry.width());
    let height = u32::from(geometry.height());
    if width < 640 || height < 360 {
        return Err(format!(
            "capture_failed: Warframe capture is too small ({width}×{height})"
        ));
    }

    let position_cookie = connection.send_request(&TranslateCoordinates {
        src_window: window,
        dst_window: geometry.root(),
        src_x: 0,
        src_y: 0,
    });
    let position = connection
        .wait_for_reply(position_cookie)
        .map_err(|error| format!("capture_failed: reading Warframe position: {error}"))?;

    let image_cookie = connection.send_request(&GetImage {
        format: ImageFormat::ZPixmap,
        drawable: Drawable::Window(window),
        x: 0,
        y: 0,
        width: geometry.width(),
        height: geometry.height(),
        plane_mask: u32::MAX,
    });
    let reply = connection.wait_for_reply(image_cookie).map_err(|error| {
        format!("capture_failed: capturing Warframe (capture may be denied): {error}")
    })?;
    let format = connection
        .get_setup()
        .pixmap_formats()
        .iter()
        .find(|format| format.depth() == reply.depth())
        .ok_or_else(|| {
            format!(
                "capture_failed: unsupported X11 pixel depth {}",
                reply.depth()
            )
        })?;
    let bits_per_pixel = u32::from(format.bits_per_pixel());
    if !matches!(bits_per_pixel, 16 | 24 | 32) {
        return Err(format!(
            "capture_failed: unsupported X11 pixel format ({bits_per_pixel} bits per pixel)"
        ));
    }
    let bytes_per_pixel = (bits_per_pixel / 8) as usize;
    let expected = width as usize * height as usize * bytes_per_pixel;
    if reply.data().len() < expected {
        return Err("capture_failed: X11 returned an incomplete Warframe frame".into());
    }

    let lsb_first = connection.get_setup().bitmap_format_bit_order() == ImageOrder::LsbFirst;
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for pixel in reply.data()[..expected].chunks_exact(bytes_per_pixel) {
        let (red, green, blue) = if bits_per_pixel == 16 {
            let packed = if lsb_first {
                u16::from_le_bytes([pixel[0], pixel[1]])
            } else {
                u16::from_be_bytes([pixel[0], pixel[1]])
            };
            (
                (((packed >> 11) as u32) * 255 / 31) as u8,
                ((((packed >> 5) & 63) as u32) * 255 / 63) as u8,
                (((packed & 31) as u32) * 255 / 31) as u8,
            )
        } else if lsb_first {
            (pixel[2], pixel[1], pixel[0])
        } else {
            (pixel[0], pixel[1], pixel[2])
        };
        rgba.extend_from_slice(&[red, green, blue, 255]);
    }
    let image = RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| "capture_failed: constructing the X11 Warframe frame".to_string())?;

    Ok(CapturedFrame {
        image,
        x: i32::from(position.dst_x()),
        y: i32::from(position.dst_y()),
        width,
        height,
    })
}
