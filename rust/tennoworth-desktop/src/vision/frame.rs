//! Cropping and encoding captured frames for recognition.

use image::{imageops::FilterType, DynamicImage, ImageFormat, RgbaImage};
use serde::Serialize;
use std::io::Cursor;

#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct NormalizedRect {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

pub(crate) fn pixel_rect(image: &RgbaImage, rect: NormalizedRect) -> (u32, u32, u32, u32) {
    let x = (rect.x * image.width() as f64)
        .round()
        .clamp(0.0, image.width().saturating_sub(1) as f64) as u32;
    let y = (rect.y * image.height() as f64)
        .round()
        .clamp(0.0, image.height().saturating_sub(1) as f64) as u32;
    let width = (rect.width * image.width() as f64)
        .round()
        .clamp(1.0, image.width().saturating_sub(x) as f64) as u32;
    let height = (rect.height * image.height() as f64)
        .round()
        .clamp(1.0, image.height().saturating_sub(y) as f64) as u32;
    (x, y, width, height)
}

/// The crop scaled to `target_width` and reduced to luminance, as a PGM.
pub(crate) fn encode_crop(
    image: &RgbaImage,
    rect: NormalizedRect,
    target_width: u32,
) -> Result<Vec<u8>, String> {
    let (x, y, width, height) = pixel_rect(image, rect);
    let mut crop = image::imageops::crop_imm(image, x, y, width, height).to_image();
    // Give Tesseract roughly the same glyph size at 720p, 1080p, 1440p and 4K.
    // This also bounds the amount of pixel data sent through Leptonica at 4K.
    let target_height =
        ((height as f64 * target_width as f64 / width as f64).round() as u32).max(1);
    if crop.width() != target_width {
        crop = image::imageops::resize(&crop, target_width, target_height, FilterType::Triangle);
    }
    // Per-channel thresholding fragments antialiased white glyphs into colored
    // edges on the teal reward cards. Preserve their luminance for Tesseract.
    for pixel in crop.pixels_mut() {
        let luminance = ((u32::from(pixel.0[0]) * 77
            + u32::from(pixel.0[1]) * 150
            + u32::from(pixel.0[2]) * 29
            + 128)
            >> 8) as u8;
        pixel.0[..3].fill(luminance);
        pixel.0[3] = 255;
    }
    let header = format!("P5\n{} {}\n255\n", crop.width(), crop.height());
    let mut pgm = Vec::with_capacity(header.len() + crop.width() as usize * crop.height() as usize);
    pgm.extend_from_slice(header.as_bytes());
    pgm.extend(crop.pixels().map(|pixel| pixel.0[0]));
    Ok(pgm)
}

pub(crate) fn encode_frame(image: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut cursor = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image.clone())
        .write_to(&mut cursor, ImageFormat::Png)
        .map_err(|e| format!("encoding Warframe capture: {e}"))?;
    Ok(cursor.into_inner())
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, reason = "tests index known fixtures")]
mod tests {
    use super::*;

    #[test]
    fn crops_are_scaled_and_reduced_to_luminance() {
        let image = RgbaImage::from_pixel(512, 100, image::Rgba([240, 120, 60, 80]));
        let pgm = encode_crop(
            &image,
            NormalizedRect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            256,
        )
        .unwrap();
        let header = b"P5\n256 50\n255\n";
        assert!(pgm.starts_with(header));
        let pixels = &pgm[header.len()..];
        assert_eq!(pixels.len(), 256 * 50);
        assert!(pixels.iter().all(|pixel| *pixel == 149));
    }
}
