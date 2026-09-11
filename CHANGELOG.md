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
version - a change confined to `frontend/` reaches tennoworth.app on
promotion and needs no desktop release. Cut one when desktop users have a
reason to update.

`bun scripts/release.ts prepare <bump>` opens a deliberately incomplete,
structured section here. Before merging the bump, replace every placeholder:
the release body starts with exactly one contextual emoji in
`# <emoji> TennoWorth Desktop X.Y.Z`, follows with an announcement and a
separate plain-language summary, groups top-level bullets below plain-text
category headings under `## Changelog (N)`, and ends with `## Updating`.
`N` must equal the number of top-level bullets. No other emoji belongs in the
entry. Use `🐧` for Linux/Wayland, `🪟` for Windows, `🔒` for security, `🎨`
for appearance, or another single symbol that honestly describes the release.

`bun scripts/release.ts check` enforces this contract as soon as the desktop
version moves ahead of the latest published tag. The release workflow repeats
the same validation through `bun scripts/release.ts notes --release` before
building either platform. Historical entries are intentionally not backfilled.

Releases up to and including 0.3.8 predate this file, and their notes live on
the GitHub releases themselves.

App summaries are generated from annotated bullets in this file. For releases
newer than 0.7.1, include at least one plain-English bullet in this form:
`- **What the person can do** A short explanation. <!-- app-note {"id":"stable-change-id","kind":"improved"} -->`
Kinds are `improved`, `fixed`, and `action`; optional `platforms` restricts a note
to `windows` or `linux`. Stable IDs deduplicate repeated notes. Optional
`supersedes` explicitly replaces earlier non-action IDs; action notices cannot
be hidden this way. Keep titles under 100 and explanations under 600 characters.
The annotation is omitted from published release bodies.

After editing, run `bun scripts/release.ts app-notes`. Release checks and frontend
builds reject a stale bundle. Historical releases through 0.7.1 keep their original
format; the app explicitly identifies incomplete coverage for older upgrades.

## 0.7.104 - 2026-09-11

# 🧭 TennoWorth Desktop 0.7.104

TennoWorth Desktop 0.7.104 makes inventory advice useful sooner and safer to act on.

See protected opportunities before connecting WFM, recover without losing sight of your last good scan, and review exactly what changed after an update. Daily installation counting remains off unless you choose to enable it.

## Changelog (5)

### Selling

- **Get protected estimates before connecting WFM** Scan or restore inventory to see estimated selling and ducat opportunities that honor your keep rules. The new What I’m keeping summary makes quantities and item rules easier to review. <!-- app-note {"id":"protected-estimates","kind":"improved"} -->
- **Post only from the inventory you reviewed** Listing changes and recovered batches now require the matching current game scan. If your inventory changed, TennoWorth keeps your edits and tells you whether to scan or recheck quantities. <!-- app-note {"id":"listing-scan-identity","kind":"fixed"} -->

### Recovery

- **Keep your last good inventory after a scan problem** Failed scans and scans with no tradeable items leave the last successful inventory visible with its real age. Retry, update, Settings, and reporting actions remain available. <!-- app-note {"id":"inventory-recovery","kind":"fixed"} -->

### Privacy

- **Choose whether to share a daily installation count** Sharing is off by default. If enabled in Settings, TennoWorth sends a rotating daily token without account, inventory, hardware, version, or activity details. <!-- app-note {"id":"daily-installation-count","kind":"improved"} -->

### Updates

- **Read what changed after an update** TennoWorth brings together the bundled notes for every release you skipped and lets you reopen them from Settings → Updates → What’s new. <!-- app-note {"id":"installed-update-notes","kind":"improved"} -->

## Updating

TennoWorth checks for updates automatically at launch and every 30 minutes while it is open. Downloads for Windows and Linux are available in the assets below.

## 0.7.103 - 2026-09-08

# 🔧 TennoWorth Desktop 0.7.103

TennoWorth Desktop 0.7.103 fixes inventory scanning and update downloads.

Scan your inventory without game metadata blocking the result, and update directly from older supported versions without a cached download disappearing after another release.

## Changelog (2)

### Inventory

- **More reliable inventory scans** Fixed a scan error caused by game data that was not needed to read your inventory. <!-- app-note {"id":"scan-metadata","kind":"fixed"} -->

### Updates

- **Updates work when you have missed a release** Older update links keep working after a newer version comes out. <!-- app-note {"id":"update-downloads","kind":"fixed"} -->

## Updating

TennoWorth checks for updates automatically. Downloads are available below.

## 0.7.102 - 2026-09-08

# 💱 TennoWorth Desktop 0.7.102

TennoWorth Desktop 0.7.102 is ready.

Plan sales around what you want to keep, compare complete sets with individual parts, and see how much of your stack visible buyers can cover.

## Changelog (4)

### Selling

- **Choose what you want to keep** Set aside copies or save the parts for a set you are building. Your selling and ducat suggestions take those choices into account. <!-- app-note {"id":"keep-plan","kind":"improved"} -->
- **Plan sales for complete sets** Add complete sets to Trade Session without counting their parts again in another sale. <!-- app-note {"id":"complete-sets","kind":"improved"} -->
- **Find buyers for larger stacks** See how much of your stack the listed buyers want, and compare their offers with your asking price. <!-- app-note {"id":"buyer-stacks","kind":"improved"} -->

### Reliability

- **Your latest inventory stays in view** An older calculation finishing late will no longer replace your newer scan, import, or plan. <!-- app-note {"id":"newest-inventory","kind":"fixed"} -->

## Updating

TennoWorth checks for updates automatically. Downloads are available below.

## 0.7.101 - 2026-09-08

# 🐧 TennoWorth Desktop 0.7.101

TennoWorth Desktop 0.7.101 is a small Linux hotfix.

Feedback forms and other external links now open your browser from the AppImage on systems affected by bundled library conflicts.

## Changelog (1)

### Linux

- **Links open your browser again** Report a bug, suggest an improvement, or open an item on warframe.market from the Linux AppImage. <!-- app-note {"id":"appimage-links","kind":"fixed","platforms":["linux"]} -->

## Updating

TennoWorth checks for updates automatically at launch and every 30 minutes while it is open. Downloads for Windows and Linux are available in the assets below.

## 0.7.1 - 2026-09-08

# 🎨 TennoWorth Desktop 0.7.1

TennoWorth Desktop 0.7.1 is ready.

This update makes Settings and the trading dashboard easier to scan, with consistent spacing, clearer labels, and more room for item names and actions. It also fixes the enlarged, blurred background pattern seen in WebKit.

## Changelog (6)

### Interface

- Align Settings and notification preferences in one column, with separate detection, reward-card, and diagnostics groups.
- Use consistent page headings, section spacing, and labelled controls across the trading and management views.
- Label Top picks facts and give item names, riven comparisons, and table actions enough room to remain readable.
- Separate watch creation from monitoring and ledger totals from listing automation and trade history.

### Fixes

- Keep the background grid and diagonal hatching sharp and evenly spaced on WebKit surfaces.

### Market data

- Refresh the bundled market snapshot and item catalog used when a live refresh is unavailable.

## Updating

TennoWorth checks for updates automatically. Downloads are available below.

## 0.7.0 - 2026-09-08

# 🎯 TennoWorth Desktop 0.7.0

TennoWorth Desktop 0.7.0 is ready.

Plan your next trading session, keep track of completed sales, and choose which market opportunities deserve an alert. This release brings Trade Session and a persistent notification inbox to Windows and Linux, alongside clearer layouts and safer listing review.

## Changelog (10)

### Trading

- Plan a Trade Session with Fast Cash, Plat per Trade, Clear Inventory, or Max Value, using a trade budget and optional platinum target.
- Review existing and proposed listings together, preserve edits during refresh, and recheck quantities, allowance, and changed orders before submission.
- Compare bulk orders using per-item prices and submit the reviewed lot total, with consistent pricing across live quotes and streamed price alerts.
- Protect global and per-item reserves in session planning and sell digests, and require a fresh scan before recommending items given away since the last scan.

### Notifications

- Keep completed trades, price-watch matches, Baro reminders, relevant calendar events, and daily sell opportunities in a persistent inbox.
- Select which notification categories are enabled and which may show desktop popups; preferences survive restarts.
- See listing follow-up when a completed sale needs attention, with replay protection that prevents duplicate trade handling.

### Desktop

- Use clearer light and dark layouts with responsive tables, readable long names, and review controls that remain reachable in narrow or short windows.
- Open bug reports and improvement suggestions from the app, and retain partially recognized relic rewards in their correct positions.

### Windows

- Run release and snapshot verification from Windows checkout paths, including paths containing spaces and URL-sensitive characters.

## Updating

TennoWorth checks for updates automatically. Use the in-app updater or download the Windows installer or Linux AppImage below.

Finish or discard pending Trade Session listing batches before downgrading; older versions do not understand their explicit lot sizes.

## 0.6.6 - 2026-09-03

- Relic reward recognition on Linux now follows the reward row's
  screen-height-scaled geometry and uses OCR-friendly grayscale crops, fixing
  reward screens that opened correctly but returned no recognized choices.
- Recognition now combines stable partial results across capture retries, so
  one temporarily unreadable reward slot no longer discards the other choices.

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
