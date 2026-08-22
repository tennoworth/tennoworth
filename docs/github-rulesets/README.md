# GitHub rulesets — reviewable snapshots

These files are exports of the repository's live rulesets, kept here so a
change to branch/tag protection goes through a PR like any other change.

The live ruleset is the source of truth; these files are projections of it
onto the keys `PUT /repos/{owner}/{repo}/rulesets/{id}` accepts, so they can
be re-applied verbatim. Refresh one after changing a ruleset (UI or API):

```sh
gh api repos/tennoworth/tennoworth/rulesets/<id> \
  | jq '{name,target,enforcement,bypass_actors,conditions,rules}' \
  > docs/github-rulesets/<file>.json
```

Ids: `gh api repos/tennoworth/tennoworth/rulesets --jq '.[]|"\(.id) \(.name)"'`.

| File | Live ruleset | Purpose |
|---|---|---|
| `desktop-release-tags.json` | Desktop release tags | `desktop-v*` tags can be neither deleted nor moved. No bypass actors: a wedged release is recovered by re-running its `publish` job, never by retagging. |
| `develop-integration.json` | Integration branch (develop) | Integration branch rules. |
| `main-production.json` | Production branch (main) | PR + 1 approval + required checks (`bun-audit`, `cargo-audit`, `cargo-test`, `panic-site-gate`, `version-pins`, `probe-smoke`). Repository admins may bypass: the maintainer cannot approve their own promotion PRs, while contributors' PRs still need the maintainer's review. |
