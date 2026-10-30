# Windows native testing

Use a physical, locally viewed Windows 11 desktop. RDP, virtual machines, and
streaming sessions are outside this baseline because they can change capture
behavior.

Prerequisites and environment variables are in
[CONTRIBUTING.md](../CONTRIBUTING.md#native-desktop-development). Local builds
never replace the required GitHub checks or capture testing on the gaming
desktop.

## Probe isolation and evidence

The full `TENNOWORTH_PROBE` run modifies notification preferences and imports
fictional inventory into application storage. A new `WEBVIEW2_USER_DATA_FOLDER`
isolates browser data, but does not isolate the native SQLite database. Repeated
runs under the same app identity can therefore fail the notification-defaults
check because a previous probe saved different preferences. For a fresh-default
run, build with `cargo tauri build --debug --no-bundle --config <probe-config>`
and give that temporary JSON configuration a unique per-run `identifier`.
Preserve existing application data; do not clear a gaming profile to reset a
probe.

Run Rust tests before the final isolated build. A later plain Cargo command can
rebuild the executable with the default configuration and replace its isolated
identity. Build with the probe configuration last, confirm the intended source
and unique identifier, and launch that executable without another Cargo command.

Check the final report and a clean process exit, not just a checkpoint. The
workflow requires both success evidence and a clean exit, with a 120-second
guard. Record the exact launched PID and inspect evidence even after a timeout.
Never stop another TennoWorth process based on its name alone: an inaccessible
process in an interactive session can outlive a probe that was stopped.

Incomplete OCR rows preserve their missing-reward positions and suppress
best-pick marks. Complete centered two-reward and solo rows must omit the
unoccupied outer positions of the four- and three-column detection grids;
those are not missing rewards. Dedicated assembly tests cover this distinction
both with and without an expected slot count. Browser checks cover partial
two-, three-, and four-card rows.

That recognition behavior does not establish the cause of a reported live
Windows failure. Application diagnostics and physical gameplay evidence are
what settle that; a log or a fixture alone does not prove the application
received the events or which capture or OCR stage failed.

## Physical gameplay baseline

1. Open the feature PR's successful **OCR Windows test installer** workflow,
   download `tennoworth-ocr-test-windows`, and unzip it.
2. Run the unsigned NSIS installer. The SmartScreen warning is expected for
   this test-only identity. It installs as **TennoWorth OCR Test** and does not
   replace production TennoWorth.
3. Close WFInfo, AlecaFrame, production TennoWorth, and other overlays so they
   cannot intercept the shortcut or affect capture.
4. Configure Warframe for the baseline: English, 1920×1080, 100% Windows
   scaling, default UI scale/theme, SDR, Borderless Fullscreen, and Item Labels
   enabled.
5. Launch **TennoWorth OCR Test** before entering the fissure. In **Settings →
   Relic reward overlay**, enable recognition, automatic detection, and local
   diagnostics. Initially disable live prices and owned counts. Confirm the
   status reads `watching · windows-window · OCR ready`.
6. Run five solo and five four-player fissure reward screens. For each screen,
   verify automatic appearance, every visible name, card alignment,
   click-through behavior, no focus theft, and that uncertain names are never
   recommended. Use `Ctrl+Shift+O` once to test manual fallback. Verify the
   overlay dismisses when the reward screen closes and after its 20-second
   timeout.
7. Enable live prices and owned counts and repeat two runs. Confirm cached
   values render first, followed by live updates.
8. Use **Open diagnostics**, review the captures, and share only selected
   failed or representative run directories. Captures may contain player/game
   information; nothing is uploaded automatically. **Clear diagnostics**
   removes all locally retained runs.

The baseline passes only at 10/10 detected screens, all visible slots named
correctly, and zero incorrect best-pick marks. After that, expand across
1440p/4K or supported window sizes, 125%/150% Windows scaling, alternate UI
scales/themes, 1–4 slots, and automatic/manual triggers. Test HDR only on a
monitor that supports it.

For the release corpus, record at least 100 screens and require at least 98%
reward-screen detection, 95% visible-slot recognition, zero wrong best-pick
recommendations, and cached-display p95 below 800 ms from trigger receipt.
