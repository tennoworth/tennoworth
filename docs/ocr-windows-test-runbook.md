# Relic OCR Windows test runbook

Use a physical, locally viewed Windows 11 desktop. RDP, virtual machines, and
streaming sessions are outside this baseline because they can change capture
behavior.

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
