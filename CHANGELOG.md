# Changelog

Desktop releases. Versions are `desktop-v<version>` tags. Pre-1.0: patch =
fixes, dependency bumps and internal work; minor = user-facing features, new
distribution channels and compatibility breaks; major is reserved for 1.0.

`bun scripts/release.ts prepare <bump>` opens a section here; fill it in
before merging the bump. `bun scripts/release.ts notes` reads it back for the
release body.

Nothing is backfilled: releases up to and including 0.3.8 predate this file,
and their notes live on the GitHub releases themselves.

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

