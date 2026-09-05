# Relic OCR Windows test runbook

Use a physical, locally viewed Windows 11 desktop. RDP, virtual machines, and
streaming sessions are outside this baseline because they can change capture
behavior.

## Building locally over SSH

The physical Windows host can compile and test through its standard-user
SSH account. It does not run GitHub Actions jobs. Local results do not remove
required GitHub checks or replace testing capture in the gaming desktop.

Verified on 2026-09-04: Windows 11 Pro 25H2, MSVC 14.44.35207, Windows SDK
10.0.26100.0, Rust/Cargo 1.98.1, Bun 1.4.1, Tauri CLI 2.11.4, and Tesseract
5.5.2. vcpkg revision was `04a9d8e5212d01ee1dd9478eadd9caade4f8b0d4`.
Recheck versions and disk space when resuming; these are observations, not
project version pins.

- Persist `VCPKG_ROOT`, `VCPKG_DEFAULT_TRIPLET=x64-windows-static-md`, and
  `LIBCLANG_PATH` in the build user's environment. The verified libclang
  directory is `C:\Program Files\LLVM\bin`.
- Windows PowerShell may select a blocked `bun.ps1` or `npm.ps1` shim. Use
  `bun.cmd` and `npm.cmd` without changing execution policy.
- Suppress PowerShell download progress with
  `$ProgressPreference = 'SilentlyContinue'` for readable SSH output. Use
  `[System.IO.DriveInfo]::new('C').AvailableFreeSpace` to check disk space;
  `Get-PSDrive` reported an incorrect zero in the SSH session.
- Download `eng.traineddata` from the URL and verify the SHA-256 recorded in
  the Windows workflow before building. Build `prototype/dist-desktop`
  before running plain Cargo builds.
- For the unsigned isolated installer, set
  `$env:TENNOWORTH_OCR_TEST_BUILD = '1'` and run
  `cargo tauri build --config tauri.ocr-test.conf.json --bundles nsis` from
  `companion/tennoworth-desktop`. This flag disables ordinary updater support.

The transferred reward-regression snapshot passed 121 desktop Rust tests,
four overlay component tests, Svelte checking, and the release build on
Windows. Its real three-reward OCR regression passed native 1440p, scaled
1080p/720p, ultrawide, and 16:10 variants. Fixture recognition does not prove
live capture, trigger timing, or overlay presentation.

## Probe and log-access lessons

The original installed OCR boot probe wrote
`OCR_BOOT_PROBE_OK backend=windows-window`, but did not exit within 120 seconds
under SSH. Lifecycle tracing reached `ExitRequested` without reaching `Exit`.
Moving the asynchronous exit request to `Ready` did not resolve it. The probe
now waits for `Ready`, calls Tauri's `cleanup_before_exit`, and terminates
explicitly after setup returns. Three consecutive installed Windows launches
passed with exit code 0 in 0.31, 0.05, and 0.05 seconds on 2026-09-05; the Linux
OCR probe also passed. Normal application shutdown is unchanged. The workflow
requires both success evidence and a clean exit, with a 120-second guard.
This probe returns before creating the webview, so the original timeout was
not evidence of WebView2 initialization failure.
Record the exact launched PID and inspect evidence even after a timeout.
Never stop another TennoWorth process based on its name alone: an inaccessible
process in interactive session 1 remained after this probe was stopped.

Incomplete OCR rows now preserve their missing-reward positions and suppress
best-pick marks. Complete centered two-reward and solo rows must omit the
unoccupied outer positions of the four- and three-column detection grids;
those are not missing rewards. Dedicated assembly tests cover this distinction
both with and without an expected slot count. Browser checks cover partial
two-, three-, and four-card rows. These checks do not establish the cause of
the originally reported live Windows run. A subsequent inspection of the
available Windows log found a four-reward batch followed by a three-reward
batch. Both had the usual ready marker, the corresponding slot-marker count
within two milliseconds, and a close marker about 15 seconds later. The
three-reward batch therefore uses the event format already handled by the
overlay. This does not prove the application received the events or identify
the failed capture/OCR stage; application diagnostics and physical gameplay
evidence are still needed to establish that cause.

For log access, grant the dedicated account inheritable `(OI)(CI)(RX)` on
the gaming user's `AppData\Local\Warframe` and the relevant TennoWorth cache
directory, preserving ownership and existing permissions. Scope access to
`relic-overlay-diagnostics` when that directory exists; granting the cache
parent also makes future diagnostics inherit access. Production uses
`app.tennoworth.desktop`; the isolated installer uses `app.tennoworth.ocr-test`.
Do not grant access across the entire gaming profile.

Verify from the SSH account by opening `EE.log` with read access and
`FileShare.ReadWrite | FileShare.Delete`, then closing without reading bytes.
Directory enumeration and metadata suffice to verify permissions. A missing
diagnostics directory is expected until diagnostics are enabled and created.
System-wide CIM process enumeration can be denied to this standard account;
use narrowly scoped process checks instead of broadening its privileges.

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
