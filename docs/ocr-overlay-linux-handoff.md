# Relic OCR overlay: Windows findings and Linux handoff

This document hands the `feat/relic-ocr-windows` work to an agent testing and
finishing the feature on Linux. It records what was learned from physical
Warframe testing on Windows, including failures that are easy to reproduce if
the build identity or coordinate system is wrong.

## Branch and PR

- Stacked PR: `fix/issue-26-feedback` <- `feat/relic-ocr-windows` (PR #28).
- Latest Windows code commit: `0b7b35f Fit relic OCR to narrow Warframe windows`.
- Keep the PR stacked on `fix/issue-26-feedback`; do not rebase it onto `main`
  merely to run Linux tests.
- `companion/tennoworth-desktop/resources/tessdata/eng.traineddata` may appear
  as an untracked local test artifact. The workflows download the pinned model;
  do not commit a locally downloaded copy.

The Windows implementation sequence, newest last, is:

1. `c024854` - make the installed OCR boot probe exit cleanly.
2. `f561952` - remove the Windows verbatim-path prefix before Tesseract opens
   bundled tessdata.
3. `015c409` - recover fragmented reward screens.
4. `c7dc37d` - make centered slot crops the fast path and reserve sparse
   full-frame OCR for the final compatibility fallback.
5. `a4b576c` - deliver overlay data directly to the WebView as well as through
   the Tauri event, and reassert topmost/click-through state after showing it.
6. `ab268ab` - detect the horizontal card grid, use height-relative geometry,
   and normalize title crops before OCR.
7. `0b7b35f` - fit coordinates into Warframe's centered 16:9 design viewport
   when the game window is narrower than 16:9.

## What was physically confirmed on Windows

The test machine ran Windows 11, English Warframe, item labels enabled, local
diagnostics enabled, and live prices/owned counts disabled for the OCR pass.

- A maximized/windowed capture at 1898x1024 recognized all four rewards and
  drew one overlay card over each Warframe card.
- The overlay was click-through, did not steal focus, and disappeared when the
  reward screen closed.
- A representative successful maximized run displayed cached results in
  429 ms (230 ms capture, 139 ms slot OCR, 44 ms matching).
- A later automatic EE.log run recognized all four rewards and displayed in
  605 ms. Its diagnostics were named with source `eelog`; no hotkey run was
  recorded. Pressing `Ctrl+Shift+O` while that pass was busy made the automatic
  result look as if the hotkey caused it. Use the diagnostics `triggerSource`
  rather than visual timing to distinguish the two.
- Manual `Ctrl+Shift+O` capture also worked.
- Recognition correctly resolved exact and slightly damaged text, including a
  `Kompressa Prime lueprint` OCR result to `Kompressa Prime Blueprint`.
- Best-value marking used only matches at or above the recommendation
  confidence threshold.

The observed automatic run resolved:

| Slot | OCR/catalog result | Cached value | Ducats |
| --- | --- | ---: | ---: |
| 0 | Braton Prime Barrel | 3p | 15 |
| 1 | Vauban Prime Blueprint | 18p | 65 |
| 2 | Kompressa Prime Blueprint | 2p | 15 |
| 3 | Braton Prime Barrel | 3p | 15 |

Vauban was the only platinum and ducat recommendation, as expected.

## Resolution and window geometry

Warframe does not always lay out this UI against the entire captured window.
The working model is a centered design viewport:

```text
design_height = min(capture_height, capture_width / (16 / 9))
design_top    = (capture_height - design_height) / 2
```

Horizontal reward spacing and card width scale from `design_height`. Vertical
coordinates are offset by `design_top` and scale from the same height.

This distinction came from a real 795x632 capture. The four cards were about
99 px apart. Scaling from the full 632 px height incorrectly predicted about
140 px and OCR read unrelated artwork. Scaling from the centered 16:9 viewport
predicts about 99 px and places the title crop over the visible labels.

The 795x632 transform is covered by unit tests, but should still receive
another physical reward-screen pass. The 1898x1024 path was physically
confirmed after the changes.

Title crops are resized proportionally to 256 px wide before thresholding and
OCR. This enlarges small-window glyphs and bounds Leptonica input at 1440p/4K.
The card-edge projection can adjust spacing without invoking OCR. If it does
not have enough confidence, the centered design-viewport geometry is used.

## Recognition and overlay flow

The main implementation is
[`companion/tennoworth-desktop/src/overlay.rs`](../companion/tennoworth-desktop/src/overlay.rs).

1. The EE.log `Got rewards` marker starts an automatic pass. `Missing icon
   data!` lines provide the expected slot count, and the snapshot path can
   recover a complete active batch.
2. Capture uses `xcap::Window` and selects a title containing `Warframe`.
3. The hot path makes centered/adaptive title crops, thresholds them, and runs
   Tesseract per slot.
4. It retries three captures to tolerate the UI drawing transition. Sparse
   full-frame TSV OCR runs only on the final attempt.
5. Catalog matching allows guarded fuzzy recognition, but recommendations
   require at least 0.90 confidence.
6. Overlay geometry is reduced to a compact window covering only recognized
   slots.
7. The overlay is shown topmost, non-activating, and click-through. Result data
   is sent through the Tauri event and a direct WebView bridge; the bridge is
   important because a listener-registration race previously produced a
   visible window with no rendered cards on Windows.
8. EE.log's reward-close marker hides the overlay. A 20-second timeout is the
   backstop.

## Build identity pitfall

Do not replace an OCR-test installation with the output of a plain production
`cargo build`. The default `tauri.conf.json` identifier is
`app.tennoworth.desktop`; the isolated OCR build uses
`app.tennoworth.ocr-test`. A production-identity binary reads a different
database, where the overlay may be disabled, and therefore does not register
the shortcut or automatic capture. This happened during local Windows testing
and initially looked like an OCR regression.

CI gets this right through `tauri.ocr-test.conf.json`. For a direct Cargo build
without the Tauri CLI, the equivalent merge used on Windows was:

```powershell
$env:TENNOWORTH_OCR_TEST_BUILD = '1'
$env:TAURI_CONFIG = '{"productName":"TennoWorth OCR Test","identifier":"app.tennoworth.ocr-test","bundle":{"createUpdaterArtifacts":false,"targets":["nsis"]}}'
cargo build --release -p tennoworth-desktop
```

On Linux, use the normal Tauri build command/configuration whenever possible
rather than reproducing that PowerShell workaround.

## Verification already run

With the local vcpkg root exposed, all overlay tests passed:

```powershell
$env:VCPKG_ROOT = 'C:\path\to\vcpkg'
cargo test -p tennoworth-desktop overlay::tests::
```

Result: 25 passed, 0 failed. The suite covers matching, recommendation guards,
dynamic layout recovery, compact overlay placement, normalized OCR crops,
edge-based grid recovery, 16:9, ultrawide, and the 795x632 centered-viewport
case.

The release binary also passed:

```text
OCR_BOOT_PROBE_OK backend=windows-window
```

The Windows installer workflow and physical procedure remain documented in
[`docs/ocr-windows-test-runbook.md`](ocr-windows-test-runbook.md).

## Linux work to do

1. Check out `feat/relic-ocr-windows` and run the overlay tests before making
   platform changes.
2. Confirm the Linux bundle contains both the Tesseract/Leptonica runtime and
   `tessdata/eng.traineddata`, and that the OCR boot probe initializes from the
   bundled resource path.
3. Test an X11 session first. Capture currently uses `xcap`, so native Wayland
   portal/PipeWire capture is not implemented.
4. Under Wayland, run Warframe through XWayland in borderless/windowed mode and
   verify that `xcap::Window::all()` can see and capture it. The current error
   message explicitly describes this limitation.
5. Verify the GTK overlay remains non-activating (`set_accept_focus(false)`),
   topmost, transparent, click-through, and correctly positioned with desktop
   scaling and multi-monitor offsets.
6. Physically test automatic EE.log triggering separately from the hotkey. Use
   the diagnostics run name/context (`eelog`, `eelog-snapshot`, or `hotkey`) as
   evidence.
7. Cover at least 1920x1080 and one small/narrow window. If possible, add
   1440p/4K, 16:10, ultrawide, and non-100% desktop scaling.
8. Confirm close-marker dismissal and the 20-second timeout.
9. Do not weaken catalog or recommendation confidence merely to accept noisy
   Linux captures. Fix capture color/scale/crop preprocessing first.

For every failure, enable diagnostics and retain the run's `context.json`,
`timings.json`, per-attempt `warframe.png`, crop PNGs, OCR text, and layout
JSON. Captures can contain player names and game information, so do not upload
them without the user's explicit selection.

## Current known boundaries

- Native Wayland capture remains future work; XWayland is the supported Linux
  test route for this branch.
- HDR was not tested.
- 125%/150% Windows display scaling, alternate Warframe UI scales/themes,
  1440p/4K, and ultrawide are modeled or listed in the test matrix but were not
  all physically exercised.
- Small-window geometry is unit-tested from the real 795x632 failure capture
  and still needs a clean physical confirmation.
- Automatic display can complete in roughly 0.6 seconds. A hotkey pressed
  during that busy pass is rejected, so wait about one second before deciding
  that automatic detection failed.

## Linux bundle validation (2026-08-28)

The Windows-tested tip was rebuilt on a rolling Linux host with Tesseract
5.5.3 and Leptonica 1.87.0. All 25 overlay tests passed. The generated AppDir
contained the pinned `eng.traineddata`, `libtesseract.so.5`, and
`libleptonica.so.6`; `ldd` resolved both OCR libraries from the AppDir rather
than the host. Running that extracted bundle against the active desktop
completed the installed-resource probe with:

```text
OCR_BOOT_PROBE_OK backend=wayland-xwayland
```

Local AppImage compression could not finish because linuxdeploy's embedded
older `strip` cannot parse the rolling distribution's RELR sections. This is a
host-tool compatibility issue after the AppDir is populated, not an OCR bundle
failure; production and test workflows build on Ubuntu 22.04. The branch-only
`OCR test installers` workflow now has an Ubuntu 22.04 Linux job that builds
the isolated AppImage, asserts all three OCR resources, runs the extracted
bundle under Xvfb, and uploads `tennoworth-ocr-test-linux` for physical testing.

No Warframe process or EE.log installation was present on this Linux host, so
physical XWayland capture, focus/click-through behavior, automatic triggering,
and reward-screen geometry remain to be tested with the uploaded Linux bundle.
