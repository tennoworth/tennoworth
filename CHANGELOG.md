# Changelog

Desktop releases. Versions are `desktop-v<version>` tags.

**Pre-1.0 the minor digit is spent sparingly, because 1.0 has to mean
something.** It is not a counter of how much work happened.

- **patch** - the default, and where nearly everything belongs: bug fixes,
  dependency bumps, internal work, and ordinary user-facing features and UI
  work. A new view or a reworked screen is a patch.
- **minor** - reserved for changes to *what the product is or what it is
  compatible with*: a distribution channel added or removed, a persisted
  database / inventory / export format change, an updater or package-identity
  change, a whole-product overhaul, or anything that breaks compatibility.
  If you cannot name the thing that changed shape, it is a patch.
- **major** - 1.0 only, and only when the compatibility contract below is one
  the maintainer is willing to stand behind: database migrations, export
  formats, updater continuity, package identity, supported operating systems.

**Not every change needs a release at all.** The web app is continuously
deployed from `main` and identifies itself by build commit, not by this
version - a change confined to `prototype/` reaches tennoworth.app on
promotion and needs no desktop release. Cut one when desktop users have a
reason to update.

`bun scripts/release.ts prepare <bump>` opens a section here; fill it in
before merging the bump. `bun scripts/release.ts notes` reads it back for the
release body.

Nothing is backfilled: releases up to and including 0.3.8 predate this file,
and their notes live on the GitHub releases themselves.

## 0.6.5 - 2026-08-30

- On Wayland, relic reward results now use a native layer-shell overlay that
  stays above borderless-fullscreen Warframe without taking focus or
  intercepting clicks. The existing desktop window remains available as a
  compatibility fallback.
- Overlay placement now maps XWayland capture geometry onto Wayland outputs,
  including mixed-DPI and fractional-scale layouts. Settings reports capture
  and presentation backends separately for clearer diagnostics.

## 0.6.4 - 2026-08-29

- TennoWorth now uses a cleaner transparent application icon with corrected
  honeycomb geometry.

## 0.6.3 - 2026-08-28

**Desktop recovery and account controls are more dependable.**

- Trade detection no longer guesses which warframe.market listing to adjust
  when EE.log omits a mod rank or item subtype, or when multiple listings share
  an item slug. Unambiguous untiered sales continue to reduce or close their
  matching listing automatically.
- Desktop event subscriptions now detach when panels, dialogs, update banners,
  or the relic overlay unmount, preventing duplicate notifications and progress
  updates after repeatedly opening those surfaces.
- The desktop market snapshot now refreshes after network reconnection and
  every 30 minutes, allowing offline launches to recover without an app restart.
- Settings now shows the warframe.market session state and provides a confirmed
  logout that removes saved credentials and interrupted listing batches from
  the device.

**Updates and inventory restores fail more safely.**

- Manual update checks now explain when a Linux install cannot self-update and
  needs the AppImage, instead of incorrectly reporting that install as current.
- Restoring an encrypted inventory backup now uses an in-app review step and
  cannot decrypt or replace a non-empty inventory before explicit confirmation.

**Small interface fixes round out the release.**

- Relic-overlay document styles are now isolated from the hosted site, restoring
  normal page scrolling while preserving the transparent overlay surface.
- GitHub and Ko-fi links are now available from the hosted status bar and the
  desktop sidebar.

## 0.6.2 - 2026-08-28

- Settings now includes a manual **Check for updates** action on Windows and
  Linux AppImage builds. TennoWorth also checks the signed updater manifest
  every 30 minutes while running; downloads and installation still require
  explicit confirmation.

## 0.6.1 - 2026-08-28

**Windows is now one installer, and the release carries one checksum file.**

- **The `.msi` is retired.** Every release shipped two Windows installers that
  did the same job; the NSIS `.exe` is the one the in-app updater uses, and it
  is the smaller of the two. Nothing is lost by dropping the other. If you
  installed from the MSI, you keep receiving updates - the updater falls back
  to the generic Windows entry on its own, and they arrive as the `.exe`
  installer.
- **Windows `.sha256` sidecars are gone.** `SHA256SUMS` on the release lists
  the same hash. Verify with `sha256sum --ignore-missing -c SHA256SUMS` - the
  `--ignore-missing` is what lets you check one downloaded file against a list
  naming all of them. The AppImage keeps its own `.sha256` sidecar, because it
  is also served unversioned from the rolling updater release.
- Together these take a release from twelve assets down to seven, of which two
  are the source archives GitHub attaches on its own.

**Relic reward recognition is available as an opt-in local overlay.**

- TennoWorth can watch `EE.log` for reward-screen timing, capture the Warframe
  window, recognize one to four English reward names locally with Tesseract,
  and place market and owned-count context over the choices. A manual
  `Ctrl+Shift+O` fallback is available when the log marker is missing or late.
- Recognition now reports capture, OCR, layout, and catalog-match failures
  separately. Uncertain matches may be shown, but are never marked as the best
  pick. Cached results appear first while optional live prices refresh.
- Optional diagnostics retain only the newest ten local runs and can include
  the captured reward area, name crops, OCR output, layout, results, and stage
  timings. Nothing is uploaded automatically, and diagnostics stay off by
  default because captures may contain player or game information.
- The initial supported capture paths are native window capture on Windows and
  X11, including XWayland sessions. Native Wayland capture and exclusive
  fullscreen are not supported yet.

**Inventory recommendations and desktop behavior are more accurate.**

- Set recipes preserve required duplicate components, fixing recommendations
  such as Kogake Prime needing two boots and two gauntlets. Set economics now
  distinguish current asks from instant-sale bids and explain the comparison.
- Desktop links open through the operating system with an HTTPS host allowlist.
- Riven cards no longer present unsupported values; comparisons are limited to
  the stat information supplied by the source data.
- The desktop workspace scrolls independently again without breaking the
  document-scrolling layout used on narrow screens.

## Unreleased

*Fold this into the next version's section when you run `release.ts prepare`.*

## 0.6.0 - 2026-08-22

**Linux is now distributed as an AppImage, and only as an AppImage.** If you
installed from the apt or dnf repository, or from the AUR, read this.

- **The `.deb` and `.rpm` builds are gone, and the two AUR packages are frozen
  at 0.5.0.** The AppImage is the only Linux build that can update itself -
  Tauri's updater never supported the others - so every Linux user was
  choosing between a package that goes stale silently and one that doesn't.
  One channel that works beats four that half-work.
- **Nothing you installed will break.** The apt and dnf repositories stay
  online and keep serving 0.5.0; the AUR packages still build 0.5.0. They
  simply stop receiving new versions. **To keep getting updates, download
  `TennoWorth-x86_64.AppImage` from this release** - it self-updates from
  then on.

  **Update, 2026-09-01:** the frozen apt and dnf archives have now been
  removed. Their original 0.5.0 package files remain attached to the GitHub
  release for archival use. The two AUR package names remain held by the
  project maintainer, but are still frozen and unsupported.
- **The memory-scan permission hint was wrong for AppImage users** and now
  isn't. `setcap` cannot work there: the AppImage runs from a temporary mount
  that ignores file capabilities, and the path changes every launch. The app
  now tells you to allow same-user ptrace instead, with the line to make it
  survive a reboot. It also no longer suggests a command the kernel refuses
  outright when `ptrace_scope` is 3.

## 0.5.0 - 2026-08-21

A visual overhaul: TennoWorth now has one theme in two modes, and the theme
control has moved somewhere sensible.

- **One theme, light and dark.** The paper-and-ink look is now the whole
  design - square corners, dotted rules, no glow, and an active state that
  inverts rather than changing colour. Its dark mode is new: a deep warm
  brown-black that keeps the same character with the lights off.
- **Corpus, Vitruvian and Baseline are gone.** Four half-finished looks made
  every screen a compromise; one look done properly is better. If you were on
  one of them, you'll land on the new theme automatically.
- **Theme selection moved to Settings → Appearance**, along with a new
  Settings view. Light, Dark, or System - System follows your OS and changes
  with it. It used to sit in the sidebar and on the landing page, which is not
  where a preference belongs.
- **The app is lighter.** Dropping the retired looks removed three token sets,
  their structural rules, and five bundled fonts.

## 0.4.0 - 2026-08-21

The app itself is unchanged since 0.3.8 - this release is about how releases
are built and how you can verify them.

- **Windows installers now ship `.sha256` files.** `SECURITY.md` has told you
  to check them for a while; until now they only existed for the Linux
  artifacts. The `.exe` and `.msi` both have one, in the same `sha256sum`
  format as everything else.
- **Every release carries a `SHA256SUMS`** listing all of its assets, so one
  `sha256sum -c SHA256SUMS` covers the lot.
- **Releases are now published in one step.** A release used to become public
  while parts of it were still uploading, and could go out with an asset
  missing. Now everything is built, verified and attached to a draft first,
  and made public once - so a release you can see is a release that is
  complete.
- **The verification instructions in `SECURITY.md` were corrected**, including
  a stale claim that builds were reproducible. They are publicly auditable CI
  builds; the docs now say so.
