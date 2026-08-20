# Release pipeline overhaul — 2026-08

Status: **in progress.** Phase 1 shipped in `release-pipeline/phase-1-quick-fixes`;
phases 2–4 stack on top of it.

This document is the decision record for the desktop release pipeline rework.
It exists because `release-desktop.yml` grew one fix at a time — every comment
in it is a real incident — and the result publishes a release in four
independent, non-atomic writes. The individual fixes were right; the shape they
add up to is not.

Scope is the **desktop release stream only** (`desktop-v*` tags,
`.github/workflows/release-desktop.yml`, `packaging/aur/`, the updater
manifest). The rolling `web-latest` / `scrape-latest` streams are untouched.

---

## Verified defects

Each of these was confirmed against the live repo and the published releases,
not inferred from the workflow text.

1. **Publishing is not atomic.** The `build` (Windows) job publishes versioned
   assets with `softprops/action-gh-release@v2` at `draft: false`, and
   `linux-binary` publishes to the same release independently. Whichever
   finishes first makes the release public with a *partial* asset set, and the
   `aur` job can still fail afterwards — which is exactly what happened for
   0.3.5 (run 30806615370). A release is currently "published" several times,
   in a racing order, with no point at which the whole thing is either complete
   or absent.

2. **`workflow_dispatch` can overwrite the updater manifest from any ref.** The
   version guard and every versioned job are gated on
   `startsWith(github.ref, 'refs/tags/desktop-v')`, but the
   `tauri-apps/tauri-action` step that publishes to the rolling
   `desktop-latest` tag — the release every installed Windows client reads
   `latest.json` from — has **no guard at all**. A dispatch from any branch
   rebuilds and overwrites the live updater artifacts. Nothing malicious is
   needed; one "let me just re-run the build" ships whatever that branch
   contains to every install.

3. **Nothing checks that a `desktop-v*` tag points at `main`.** The tag is the
   only input to the release, and it can be cut anywhere.

4. **`SECURITY.md` promised things the pipeline does not do.**
   - It claimed release binaries are "reproducibly built in public CI".
     They are not reproducible: the toolchain floats (`dtolnay/rust-toolchain@stable`),
     dependency resolution for the frontend and the bundler's own inputs are not
     pinned to a byte-identical output, and nothing verifies a rebuild matches.
     What is true, and what we actually commit to, is that the build is
     *publicly auditable*: the workflow, the source commit at the tag and the
     full build logs are public.
   - "How to verify a release" told Windows users to compare the installer
     against a `.sha256` file on the release. **No such file existed** — 0.3.7
     and 0.3.8 shipped `.exe`/`.msi` with only a `.sig`. The Linux artifacts had
     `.sha256` sidecars all along; Windows never did.
   - The "Windows example" code block ran `sha256sum -c` against the **`.deb`**.

5. **The version is pinned in six places.**
   `companion/tennoworth-desktop/Cargo.toml`,
   `companion/tennoworth-desktop/tauri.conf.json`, `companion/Cargo.lock`,
   `packaging/aur/tennoworth/PKGBUILD`, `packaging/aur/tennoworth-bin/PKGBUILD`,
   `prototype/package.json` — plus the tag itself, which is what the workflow
   guard compares against. Four of the six are checked at release time only,
   and `prototype/package.json`'s copy is read by nothing at runtime.

6. **Doc rot.**
   - `docs/signing-runbook.md` (maintainer-only, gitignored) documents
     `.github/workflows/release-companion.yml` and the standalone CLI. Both were
     removed.
   - `packaging/aur/README.md` says the AppImage "was dropped". The workflow
     builds, strips, repacks, signs and publishes `TennoWorth-x86_64.AppImage`,
     and `update.rs` uses it for Linux self-update when running *as* an
     AppImage. The workflow's own header comment ("No AppImage, deliberately")
     is stale in the same way.

7. **Supply-chain hygiene.** Every third-party action floats on a mutable tag
   (`@v4`, `@v2`, `@v0`, `@stable`); `permissions: contents: write` is granted
   workflow-wide including to build jobs that only need `read`; there is no
   `concurrency` group (two releases can interleave on the same rolling tag);
   and there is no protected environment gating the publish.

---

## What is NOT changing

These are settled decisions, re-affirmed rather than revisited:

- The `desktop-v*` tag prefix.
- A static Tauri updater manifest (`latest.json` on a release asset) — no
  update server.
- Updater signatures (minisign, key generated offline, never in the repo).
- The glibc-floor gate on the Linux binary (ubuntu-22.04, floor 2.35).
- The frozen-`Cargo.lock` check (`cargo fetch --locked`), which is what the AUR
  source package's `--frozen` build depends on.
- Native Windows + Linux builds (no cross-compilation).
- Immutable, version-specific AUR source URLs.
- `develop` → `main` promotion.
- The apt / dnf / AUR distribution channels.
- `desktop-latest` stays, as the **legacy updater bridge**: the endpoint is
  baked into every shipped binary, so the tag can never be renamed or deleted
  without stranding installs.

---

## Amendments and policy

- **0.3.8 stands.** It published cleanly on 2026-08-20 and is not re-cut. The
  next feature release is **0.4.0**.
- **`prototype/package.json` is already `"private": true`**, so its `version`
  field is not consumed by a registry either. Nothing reads it (see
  `prototype/vite.config.ts` — the footer shows the build commit, and there is
  deliberately no `__APP_VERSION__` define). It is removed in phase 3 rather
  than kept in step by hand.
- **SemVer policy, pre-1.0:**
  - `patch` (0.3.x) — bug fixes, dependency bumps, internal refactors, packaging
    or CI changes with no user-visible behavior change.
  - `minor` (0.x.0) — user-facing features, new distribution channels, and
    compatibility breaks. Pre-1.0, a break is a minor bump; that is the
    contract until 1.0 defines a stricter one.
  - `major` — reserved for 1.0. Not used before then.
- **Prereleases** (`0.4.0-beta.1` and friends) are **not** cut until a separate
  beta updater endpoint exists. A prerelease on the current single
  `desktop-latest` endpoint would push a beta to every stable install, because
  the updater compares versions and semver orders `0.4.0-beta.1` above `0.3.8`.
- **`Cargo.toml` is the authoritative version** going forward. Every other pin
  is derived from it or validated against it.

### Deferred, not dropped

- **semantic-release / Release Please.** Both derive the version from commit
  messages; this repo does not enforce a commit convention, so they would derive
  the wrong version silently. Revisit once conventional commits are enforced in
  CI.
- **Channel pruning** (do we need apt *and* dnf *and* AUR *and* AppImage *and*
  a raw tarball?). That is a product decision about who we are willing to leave
  behind, not a pipeline cleanup, and it gets its own pass.
- **The 1.0 contract.** What we promise to keep stable (scan behavior, on-disk
  formats, the updater endpoint, the CLI-less command surface) has to be written
  down before a 1.0 tag means anything.
- **Content-addressed `web-<sha>` / `scrape-<sha>` artifacts**, then repo-wide
  release immutability. Today's rolling `-latest` tags are mutable by design;
  making the versioned artifacts content-addressed first is the prerequisite for
  turning immutability on repo-wide without breaking the pullers on the box.

---

## Phase 1 — honor the published promises, close the dispatch hole

Small, low-risk, shippable before the pipeline rewrite. Branch:
`release-pipeline/phase-1-quick-fixes`.

- **Windows `.sha256` sidecars.** The `build` job now emits a `.sha256` next to
  every `.exe` and `.msi` it archives, and they upload with the rest of
  `dist/*`. This is the file `SECURITY.md` has been telling users to check.
- **`SECURITY.md` corrections.** "reproducibly built" → publicly-auditable-CI
  phrasing; the Windows verification example now checks the Windows installer's
  own `.sha256` instead of the `.deb`.
- **The dispatch hole, minimally closed.** `tagName` on the `desktop-latest`
  `tauri-action` step is now computed: the rolling tag on a `desktop-v*` tag
  ref, and the empty string otherwise. `tauri-action` treats an absent/empty
  `tagName` as "build, do not publish" (that is already how the `linux-binary`
  job uses it), so a `workflow_dispatch` becomes a build-only smoke test that
  cannot touch the updater manifest. A separate guard step additionally refuses
  a dispatch from any ref other than `main`, so the intent is explicit rather
  than implied by a template expression.
- **Rulesets.** `docs/github-rulesets/` is committed as the reviewable source
  of what the repo enforces. All three were **already applied** when this work
  started — `Desktop release tags` (21067950), `Integration branch (develop)`
  (21067958) and `Production branch (main)` (21067966), all `active`, each
  matching the committed JSON. The tag ruleset is the load-bearing one: a
  published release tag must never move or vanish under a `PKGBUILD` that pins
  a checksum against the artifacts it names.

  One drift to close by hand: phase 3 adds a `version-pins` status check, and
  the committed `main-production.json` lists it. The **live** ruleset does not
  yet — add it in repo settings, or `PATCH` the ruleset, once phase 3 merges.

The tag→main check (defect 3) is deliberately *not* patched into the tag-driven
flow, because phase 2 removes the tag trigger entirely and makes the ref
question moot.

## Phase 2 — one atomic release

Branch: `release-pipeline/phase-2-atomic-release`, stacked on phase 1.

`release-desktop.yml` is rewritten around a single rule: **the release becomes
public exactly once, and only after every artifact exists and has been
verified.**

- **Trigger: `workflow_dispatch` only.** The `push: tags:` trigger is removed.
  The tag is no longer the *input* to the release — it is an *output*, created
  by the workflow at the SHA it actually built. That kills defect 3 (a tag can
  no longer point anywhere but the `main` commit the run captured) and makes
  "re-run the release" a well-defined operation.
  - Input: `version` (`X.Y.Z`).
  - The run refuses any ref other than `refs/heads/main`.
- **`preflight`** — capture the `main` SHA and validate everything that can be
  known before a single byte is compiled: the input version matches
  `Cargo.toml` (and, until phase 3 removes them, every other pin); it is strict
  semver; it is strictly greater than the newest published `desktop-v*`; the
  tag does not already exist; `Cargo.lock` is consistent (`cargo fetch --locked`).
- **`build-windows` / `build-linux`** — run in parallel with
  `permissions: contents: read`. They publish *nothing*. Every output
  (installers, `.sig`s, deb, rpm, tarball, AppImage, `.sha256` sidecars, the
  per-target `latest.json` fragments) is uploaded as a workflow artifact. All
  existing build logic is preserved verbatim: signing env, the refuse-without-a-key
  gate, the glibc floor, the libwayland strip / repack / re-sign, the frozen
  lock.
- **`publish`** — `needs` both, `permissions: contents: write`,
  `environment: desktop-release`. Downloads every artifact, verifies the
  inventory is complete and that every artifact that must be signed has its
  `.sig`, assembles a unified `SHA256SUMS`, merges the Windows and AppImage
  updater entries into one `latest.json` with the same platform keys the
  current workflow produces (`windows-x86_64`, `windows-x86_64-nsis`,
  `windows-x86_64-msi`, `linux-x86_64`), creates the annotated tag at the
  captured SHA, creates a **draft** release, uploads every asset, and only then
  runs a single `gh release edit --draft=false`. That last command is the
  atomic moment.
- **`desktop-latest`** — a bridge job after `publish` refreshes the rolling
  release's assets and manifest, exactly as today, for already-installed
  clients.
- **`aur`** — after `publish`, logic unchanged. It reads from the published
  release, so it can no longer race it.
- **Hardening** — `concurrency: { group: desktop-release, cancel-in-progress: false }`,
  job-scoped permissions, and every third-party action pinned to a full commit
  SHA with a `# vX.Y.Z` comment.

The workflow is **untested until it is run.** It should be exercised with a
throwaway dry run (a `0.3.9-rc` style version, or a run against a scratch repo)
before or as part of cutting 0.4.0.

## Phase 3 — one version, derived

`scripts/release.ts` (Bun, no dependencies) becomes the only thing that writes a
version:

- `prepare <minor|patch|major|X.Y.Z>` — bump `Cargo.toml`, refresh
  `Cargo.lock`'s own-package entry, bump both `PKGBUILD` `pkgver`s, open a
  `CHANGELOG.md` section.
- `check [--release X.Y.Z]` — validate that every pin agrees, that the version
  is strict semver, and that it is not **behind** the newest published
  `desktop-v*`. Non-zero exit on drift.

  The monotonic rule is split deliberately. On a PR the repo legitimately sits
  *at* the newest published version between releases, so requiring strictly
  greater there would fail every ordinary PR; only "behind" is drift.
  `--release` is the release run itself, where equal is also wrong — the
  updater only ever offers a strictly greater version, so republishing
  produces a release nobody is offered. Same script, one extra assertion.
- `notes` — emit the changelog section for the release body.

`check` is wired into PR CI as the `version-pins` job in `audit.yml` so drift
fails a PR rather than a release, and `check --release` replaces the bespoke
pin validation in the new workflow's `preflight`.

Two pins are then deleted rather than automated:

- `tauri.conf.json`'s `version` — Tauri v2 falls back to the Cargo package
  version when the field is absent, so the field is pure duplication.
- `prototype/package.json`'s `version` — read by nothing (the package is
  `private`, and `vite.config.ts` bakes the build commit, not a version).

That leaves `Cargo.toml` (authoritative), `Cargo.lock` (derived, machine-written)
and the two `PKGBUILD`s (written by `prepare`, verified by `check`).

## Phase 4 — doc rot

- `docs/signing-runbook.md` — mark the Authenticode half **dormant by
  decision** (no signing certificate is being bought; SmartScreen risk is
  accepted and mitigated through winget / Scoop / AUR / Flatpak), and mark the
  `release-companion.yml` / CLI material historical rather than deleting the
  reasoning. Correct the updater-key section and the Linux-updater statement to
  match AppImage self-update reality.
- `packaging/aur/README.md` — the AppImage is back; "Cutting a release" now
  describes the dispatch flow; the version-sources table reflects `Cargo.toml`
  authority and the two removed pins.
- Any other doc the phases touch.

---

## Rollback

Phase 1 is independently revertable. Phase 2 replaces the workflow wholesale, so
the rollback is `git revert` of that commit plus re-adding the `push: tags:`
trigger — the old flow depends on nothing the new one deletes. Because the new
flow creates the tag itself, a failed release leaves **no tag and no release**
behind, which is the point: retrying is just re-running the dispatch.
