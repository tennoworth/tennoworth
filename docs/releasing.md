# Desktop release policy

This is the operating policy for releasing the TennoWorth desktop app. The
implementation details and exact command live in
[`release-desktop.yml`](../.github/workflows/release-desktop.yml).

## One rule

A desktop release is an intentional, versioned shipment. It is not a side
effect of pushing a commit to GitHub or promoting `develop` to `main`.

When a release is published, it is available to every supported desktop client
immediately. The app checks for it at launch and every 30 minutes while it is
open. It never installs without the user's confirmation, and applies the
selected update after restart.

## What ships independently

The website, live market-data snapshot, scrape pipeline, deployment
configuration, documentation, and CI-only changes deploy through their own
pipelines. They do not require a desktop version bump or desktop release. The
repository's bootstrap pair is different: it is deliberately refreshed during
desktop release preparation so every build carries the same recent fallback.

## When to cut a desktop release

Cut one immediately for a security issue; a crash; a scan, login, listing, or
updater failure; or another defect that materially prevents people using the
desktop product.

For normal user-visible features and fixes, cut a release when a coherent set
of changes is ready. Small changes may be batched into a short release train;
do not hold a finished, valuable fix just to meet an arbitrary calendar date.

Do not cut a desktop release for a change that has no effect on the shipped
desktop app. If a `main` promotion includes both desktop and non-desktop work,
the desktop release contains the complete, tested `main` commit.

## Release procedure

1. Merge the release's reviewed product work into `develop`.
2. Create a release-preparation branch from the latest `develop`. Wait for the
   production scrape to finish, then copy its locked snapshot pair:

   ```bash
   bun scripts/release.ts snapshot
   ```

   The default SSH host is `wfm`; use `--host <host>` when the production box
   has a different SSH name. The command refuses to overwrite local snapshot
   edits, rejects malformed or older-than-24-hour data, and writes the catalog
   before `market.json` so the latter remains the generation anchor.
3. On the same branch, prepare the next SemVer version and changelog entry:

   ```bash
   bun scripts/release.ts prepare <major|minor|patch|X.Y.Z>
   ```

   Complete the generated release-note scaffold before committing it. Notes
   use the same compact, user-first structure on every desktop release:

   ```markdown
   # 🐧 TennoWorth Desktop X.Y.Z

   TennoWorth Desktop X.Y.Z is ready.

   One short paragraph explaining who benefits and why the release matters.

   ## Changelog (2)

   ### Linux

   - One user-visible change.
   - Another user-visible change.

   ## Updating

   TennoWorth checks for updates automatically. Downloads are available below.
   ```

   Put exactly one contextual emoji in the release title and nowhere else in
   the entry. Use plain category headings. `🐧` fits Linux/Wayland, `🪟` fits
   Windows, `🔒` fits security, and `🎨` fits appearance; choose the single
   symbol that best describes a mixed release. The number in `Changelog (N)`
   must equal its top-level bullet count. The audit PR gate validates the
   contract when the version advances, and release preflight validates it
   again before either platform builds.

   Commit the resulting snapshot diff, version pins, and completed changelog
   entry together. One file may be byte-identical to the previous copy; the
   command still captured and validated the pair. Open the normal
   release-preparation PR into `develop`.
4. After its required checks pass, merge the release-preparation PR and promote
   `develop` to `main` using the repository's documented fast-forward procedure.
5. From `main`, run **Actions → release-desktop** with that version. Its
   preflight rejects a repository snapshot older than 24 hours, so skipping the
   explicit refresh is visible before either platform starts compiling.
6. Let the workflow build, sign, verify, publish the immutable `desktop-vX.Y.Z`
   release, and refresh the `desktop-latest` updater feed. Do not replace these
   steps with a hand-created tag or GitHub Release.
7. Verify the public release assets and updater manifest. For an urgent
   regression, ship a newer hotfix; never mutate a published versioned release.

## Channels

`desktop-latest` is the stable updater feed and must contain only tested stable
releases. The repository does not yet have a beta feed. Until it does, do not
publish prerelease builds to the stable endpoint.

When beta testing needs to scale beyond maintainers, add a separate signed beta
endpoint and opt-in beta builds. Promote the exact tested commit from beta to
stable; do not turn every `main` push into a stable update.
