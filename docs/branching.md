# Branching schema

Two long-lived branches, promotion model — not full git-flow (no release
branches, no version-bump merges). Adopted 2026-07-20.

```
feature/fix branches ──▶ develop ──(promotion)──▶ main ──▶ production
        ▲                   │                       │
   agents work here     full CI (audit)       deploy workflows fire:
   in worktrees                               build-web → web-latest →
                                              box pulls within 15 min;
                                              build-scrape → scrape-latest
```

## Why this exists

`main` auto-deploys: `build-web` publishes `web-latest` on every push
touching `prototype/`, and the box pulls it on a 15-minute timer. With
AI agents producing merges at high velocity, integration and deployment
need to be **separate decisions**. `develop` is where work integrates and
CI runs; promoting to `main` is the deliberate "this goes live" act.

## Rules

- **Feature/fix branches** branch off `develop`, merge back into `develop`
  (`--no-ff`) after review. Delete after merge.
- **Promotion** `develop` → `main` is fast-forward only
  (`git checkout main && git merge --ff-only develop && git push`), so
  `main` is always an exact prefix of `develop`'s history — no
  merge-commit drift, no back-merge needed.
- **Hotfix**: branch off `main`, merge to `main`, then merge `main` back
  into `develop` immediately.
- **Companion releases** (`v*` tags) are cut on `main` only — a tag must
  point at deployed-quality code.
- **Data-bot commits** (market refresh) land on `develop` (the default
  branch) and ride the next promotion. That's fine: the repo's market.json
  is only a bootstrap snapshot — production freshness comes from the box's
  own systemd scrape, not the repo. Keeping bots off `main` is what makes
  ff-only promotion possible.
- Promote in small batches. If `develop` sits ahead of `main` for more
  than a couple of days, either promote or explain why in the PR/commit.

## Where CI runs

| Workflow | Trigger |
|---|---|
| `audit` (tests + advisories) | push to `main` + `develop`, all PRs |
| `build-web` → deploy | push to `main` (prototype/**) only |
| `build-scrape` → box shadow | push to `main` (scrape crates) only |
| `release-companion` | `v*` tags only |
