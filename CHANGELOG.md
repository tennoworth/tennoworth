# Changelog

Desktop releases. Versions are `desktop-v<version>` tags.

**Pre-1.0 the minor digit is spent sparingly, because 1.0 has to mean
something.** It is not a counter of how much work happened.

- **patch** — the default, and where nearly everything belongs: bug fixes,
  dependency bumps, internal work, and ordinary user-facing features and UI
  work. A new view or a reworked screen is a patch.
- **minor** — reserved for changes to *what the product is or what it is
  compatible with*: a distribution channel added or removed, a persisted
  database / inventory / export format change, an updater or package-identity
  change, a whole-product overhaul, or anything that breaks compatibility.
  If you cannot name the thing that changed shape, it is a patch.
- **major** — 1.0 only, and only when the compatibility contract below is one
  the maintainer is willing to stand behind: database migrations, export
  formats, updater continuity, package identity, supported operating systems.

**Not every change needs a release at all.** The web app is continuously
deployed from `main` and identifies itself by build commit, not by this
version — a change confined to `prototype/` reaches tennoworth.app on
promotion and needs no desktop release. Cut one when desktop users have a
reason to update.

`bun scripts/release.ts prepare <bump>` opens a section here; fill it in
before merging the bump. `bun scripts/release.ts notes` reads it back for the
release body.

Nothing is backfilled: releases up to and including 0.3.8 predate this file,
and their notes live on the GitHub releases themselves.

## 0.6.0 — 2026-08-22

**Linux is now distributed as an AppImage, and only as an AppImage.** If you
installed from the apt or dnf repository, or from the AUR, read this.

- **The `.deb` and `.rpm` are gone, and so are the two AUR packages.** The
  AppImage is the only Linux build that can update itself — Tauri's updater
  never supported the others — so every Linux user was choosing between a
  package that goes stale silently and one that doesn't. One channel that
  works beats four that half-work.
- **Nothing you installed will break.** The apt and dnf repositories stay
  online and keep serving 0.5.0; the AUR packages still build 0.5.0. They
  simply stop receiving new versions. **To keep getting updates, download
  `TennoWorth-x86_64.AppImage` from this release** — it self-updates from
  then on.
- **The memory-scan permission hint was wrong for AppImage users** and now
  isn't. `setcap` cannot work there: the AppImage runs from a temporary mount
  that ignores file capabilities, and the path changes every launch. The app
  now tells you to allow same-user ptrace instead, with the line to make it
  survive a reboot. It also no longer suggests a command the kernel refuses
  outright when `ptrace_scope` is 3.

## 0.5.0 — 2026-08-21

A visual overhaul: TennoWorth now has one theme in two modes, and the theme
control has moved somewhere sensible.

- **One theme, light and dark.** The paper-and-ink look is now the whole
  design — square corners, dotted rules, no glow, and an active state that
  inverts rather than changing colour. Its dark mode is new: a deep warm
  brown-black that keeps the same character with the lights off.
- **Corpus, Vitruvian and Baseline are gone.** Four half-finished looks made
  every screen a compromise; one look done properly is better. If you were on
  one of them, you'll land on the new theme automatically.
- **Theme selection moved to Settings → Appearance**, along with a new
  Settings view. Light, Dark, or System — System follows your OS and changes
  with it. It used to sit in the sidebar and on the landing page, which is not
  where a preference belongs.
- **The app is lighter.** Dropping the retired looks removed three token sets,
  their structural rules, and five bundled fonts.

## 0.4.0 — 2026-08-21

The app itself is unchanged since 0.3.8 — this release is about how releases
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
  and made public once — so a release you can see is a release that is
  complete.
- **The verification instructions in `SECURITY.md` were corrected**, including
  a stale claim that builds were reproducible. They are publicly auditable CI
  builds; the docs now say so.

