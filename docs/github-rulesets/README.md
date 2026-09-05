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
| `develop-integration.json` | Integration branch (develop), ID `21067958` | PR-only integration, no required approval, and completion gates (`audit-gate`, `ui-gate`). Only `PedroAmorimP` (user ID `40967190`) can bypass through a PR. |
| `protected-branch-history.json` | Protected branch history, ID `22310181` | Blocks deletion and non-fast-forward updates on both `develop` and `main`, with no bypass. |
| `main-production.json` | Production branch (main) | PR + 1 approval + required completion gates (`audit-gate`, `ui-gate`). Each gate verifies its applicable checks and accepts intentionally skipped work, so documentation-only changes do not consume desktop or Rust runners. Repository admins may bypass: the maintainer cannot approve their own promotion PRs, while contributors' PRs still need the maintainer's review. |

## Maintainer workflow

Applied and read back from GitHub on 2026-09-05. Repository-level
`allow_auto_merge` is enabled; it is a repository setting rather than part of
these ruleset exports. Required gate names match the workflow jobs. Both
workflows run on PRs, on their schedules, and on demand, not on branch pushes.

For routine integration, open a PR into `develop` and use auto-merge to wait
for the applicable checks. When intentionally overriding a failing or pending
gate, the maintainer can use the PR bypass without editing repository settings.
The bypass is attached to one user, not every administrator or contributor,
and it does not permit direct pushes through the integration ruleset.
Contributors have no bypass; fork contributions remain PRs for the maintainer
to review and merge. No additional approval is required on `develop`.

Bypass applies to an entire ruleset. Keeping history protection in a separate
no-bypass ruleset means a maintainer PR override cannot delete or rewrite the
long-lived branches. Create that ruleset before removing those rules from the
integration ruleset. The existing history rules in the production ruleset are
retained as well.

The production ruleset and release-tag ruleset were not changed. Production
still declares one approval and both completion gates, with its pre-existing
administrator bypass. That bypass permits the documented non-forced
fast-forward promotion after the production PR has been checked and approved;
the new history rule also allows a fast-forward push. Do not use GitHub's merge
button to create a divergent production merge commit. Repository policy still
requires explicit authorization to promote production. Checks must be verified
on the production PR; pushing directly to `main` does not rerun these workflows.

## Rollback

The original snapshots are preserved at commit
`088b27ac5e91c7553d2efc91003f0c0a2cccfbe4`. The change-time live backup is also
saved locally at `/tmp/tennoworth-rulesets-before-20260905.json`; use the git
snapshots if that temporary file is no longer available.

1. Restore `develop-integration.json` from that commit and PUT it to ruleset
   `21067958`. Verify its deletion/non-fast-forward rules and empty bypass list
   before proceeding.
2. Delete only the new history ruleset `22310181` to return to the exact prior
   protection layout. The unchanged production ruleset retains its original
   history rules and administrator bypass.
3. PATCH the repository setting `allow_auto_merge` back to `false`.
4. Read back the three original rulesets and compare with their original
   snapshots; verify the repository setting and effective branch rules too.

The original production and tag rulesets require no restoration. Retaining the
new history ruleset is also possible if reverting only the relaxed integration
workflow rather than reproducing the entire previous configuration.
