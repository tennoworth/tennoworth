# OCR model resource

Release builds place `eng.traineddata` from Tesseract's `tessdata_fast` 4.1.0
release in this directory and verify its SHA-256 before bundling it. The model
is Apache-2.0 licensed; its source and license are at
<https://github.com/tesseract-ocr/tessdata_fast/tree/4.1.0>.

Other game languages are not bundled. The overlay downloads the matching
`tessdata_fast` 4.1.0 model on first use into the app-data `tessdata`
directory and loads it only when its size and SHA-256 match the pins in
`src/vision/ocr_models.rs`; anything else is discarded and recognition stays
in English.

The model is deliberately not committed as a multi-megabyte binary. For a
local `cargo tauri build`, download it with the same URL and checksum used in
`.github/workflows/release-desktop.yml`. The overlay reports OCR unavailable
when this bundled file is absent; it does not silently use an unrelated system
model.
