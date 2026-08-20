# GitHub rulesets — reviewable snapshots

These files are exports of the repository's live rulesets, kept here so a
change to branch/tag protection goes through a PR like any other change.

They are **snapshots, not the source of truth**. The GitHub API adds fields
that are not captured here (`require_extra_approval_for_unattributed_changes`,
`allowed_merge_methods`, `do_not_enforce_on_create`, `dismissal_restriction`,
…), so re-importing one of these files verbatim with `PUT /rulesets/{id}`
resets those to defaults. To change a ruleset: edit the live one (UI or
`gh api -X PUT` on a full `GET` export), then refresh the file here with
`gh api repos/tennoworth/tennoworth/rulesets/<id>` and trim to the same keys.

| File | Live ruleset | Purpose |
|---|---|---|
| `desktop-release-tags.json` | Desktop release tags | `desktop-v*` tags can be neither deleted nor moved. No bypass actors: a wedged release is recovered by re-running its `publish` job, never by retagging. |
| `develop-integration.json` | Integration branch (develop) | Integration branch rules. |
| `main-production.json` | Production branch (main) | PR + approval + required checks (`bun-audit`, `cargo-audit`, `cargo-test`, `panic-site-gate`, `version-pins`, `probe-smoke`). |
