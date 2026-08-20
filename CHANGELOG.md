# Changelog

Desktop releases. Versions are `desktop-v<version>` tags. Pre-1.0: patch =
fixes, dependency bumps and internal work; minor = user-facing features, new
distribution channels and compatibility breaks; major is reserved for 1.0.

`bun scripts/release.ts prepare <bump>` opens a section here; fill it in
before merging the bump. `bun scripts/release.ts notes` reads it back for the
release body.

Nothing is backfilled: releases up to and including 0.3.8 predate this file,
and their notes live on the GitHub releases themselves.

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

