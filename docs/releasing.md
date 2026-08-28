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

The website, market-data snapshot, scrape pipeline, deployment configuration,
documentation, and CI-only changes deploy through their own pipelines. They do
not require a desktop version bump or desktop release.

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

1. Merge reviewed work into `develop`, then promote it to `main` using the
   repository's documented branch procedure.
2. Decide that the `main` commit is release-ready; ensure the full required
   checks passed and that the release notes describe user-visible changes.
3. Prepare the next SemVer version and changelog entry using the repository's
   release tooling.
4. From `main`, run **Actions → release-desktop** with that version.
5. Let the workflow build, sign, verify, publish the immutable `desktop-vX.Y.Z`
   release, and refresh the `desktop-latest` updater feed. Do not replace these
   steps with a hand-created tag or GitHub Release.
6. Verify the public release assets and updater manifest. For an urgent
   regression, ship a newer hotfix; never mutate a published versioned release.

## Channels

`desktop-latest` is the stable updater feed and must contain only tested stable
releases. The repository does not yet have a beta feed. Until it does, do not
publish prerelease builds to the stable endpoint.

When beta testing needs to scale beyond maintainers, add a separate signed beta
endpoint and opt-in beta builds. Promote the exact tested commit from beta to
stable; do not turn every `main` push into a stable update.
