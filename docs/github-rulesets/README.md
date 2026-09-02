# GitHub rulesets - reviewable snapshots

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
| `develop-integration.json` | Integration branch (develop) | PR-only integration with required completion gates (`audit-gate`, `ui-gate`). The snapshot is intentionally ahead of the live ruleset until the market-refresh writer is migrated as described below. |
| `main-production.json` | Production branch (main) | PR + 1 approval + required completion gates (`audit-gate`, `ui-gate`). Each gate verifies its applicable checks and accepts intentionally skipped work, so documentation-only changes do not consume desktop or Rust runners. Repository admins may bypass: the maintainer cannot approve their own promotion PRs, while contributors' PRs still need the maintainer's review. |

## Applying the develop ruleset

The live develop ruleset currently protects only deletion and non-fast-forward
updates. Do not apply `develop-integration.json` until `refresh-market.yml` has
a compatible write path: it currently commits generated data directly to
develop with `GITHUB_TOKEN`, and the proposed PR rule will reject that push.

Move refreshes through pull requests before applying the proposed rule. Do not
add a generic GitHub Actions bypass: that would let every workflow with a write
token bypass the same protection and deepen the repository's dependence on
GitHub-specific actors. If direct refresh pushes remain, leave the live develop
ruleset unchanged. After migrating the writer, apply the JSON with the rulesets
API and refresh the snapshot from the live rule.
