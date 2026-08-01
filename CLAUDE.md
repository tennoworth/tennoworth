# CLAUDE.md — project overview

Cross-platform **Windows + Linux** Warframe inventory + market dashboard
— the no-Overwolf alternative to AlecaFrame. Windows and Linux are equal,
first-class targets (not Linux-first). Inventory is acquired by a local
memory-scan companion — PC-only by nature. Overlaps with browse.wf and
warframe.me on inventory display — must be **measurably better at
"what to sell right now"** to justify existing.

Detailed rules live in per-domain files. Read the one for the area
you're editing **before** you start writing code there:

- [`companion/CLAUDE.md`](companion/CLAUDE.md) — Rust CLI + loopback
  HTTP server. Memory scan, JWT crypto, WFM API map, cross-platform
  gotchas.
- [`prototype/CLAUDE.md`](prototype/CLAUDE.md) — Svelte 5 + Vite
  browser app. Svelte 5 reactivity gotchas, storage choices, CSP.
- [`scripts/CLAUDE.md`](scripts/CLAUDE.md) — Python utilities and
  scrapers. Atomic writes, flush rules, UA requirements.

---

## What lives where

```
companion/       Rust binary — fetch / login / serve subcommands
prototype/       Svelte 5 + Vite app, deployed as static
prototype/public/market.json    central artifact: the WFM snapshot
scripts/         one-shot Python utilities
wfm_demand.py    the ORIGINAL WFM scraper. Still the rollback path and the
                 parity reference; production now runs the Rust port
packaging/aur/   AUR recipes (tennoworth, tennoworth-bin) + their .install hooks
deploy/          self-host kit for the production LXC: Caddyfile, setup, scrape/web-pull units, plus the signed apt+dnf repo publisher (setup-repo.sh, pull-packages.sh)
tests/           pytest tests for the Python side
.github/workflows/  refresh-market (cron), release-companion (tag), audit
.github/actions/    shared composite actions (setup-rust, setup-python,
                     publish-rolling-release) the workflows above call into
.claude/         Claude Code local config + agent worktrees
SECURITY.md      threat model + what we do and don't commit to
```

## Components at a glance

```
┌─ Warframe game ──────────────────────────────────┐
│   /proc/<pid>/mem  or  ReadProcessMemory         │
└────────────────────────┬─────────────────────────┘
                         │ scrape accountId+nonce
                         ▼
            ┌──────── companion CLI ────────┐
            │  fetch  → inventory.json      │
            │  login  → ~/.config/wfminv/   │
            │           wfm-jwt.enc (AES)   │
            │  serve  → 127.0.0.1:RAND      │
            └──────┬────────────┬───────────┘
                   │            │  ↑   X-Session-Token
                   │            │  │   plan / pending / orders
                   ▼            ▼
        inventory.json    ┌── browser app (prototype/) ──┐
        (drop into UI)   ─┤  joins inv × market.json      │
                          │  no backend, no accounts      │
                          └───────────────────────────────┘
                                       ▲
                                       │ GET market.json
                                       │ (refreshed on the box)
                            ┌──────────┴─────────────────────────────┐
                            │  wfm-scrape scrape  (Rust)              │
                            │  → CSV → csv_to_market_json.py          │
                            │  (systemd timer on the box, 2h;         │
                            │   GH cron refreshes repo copy)          │
                            │  wfm_demand.py is the rollback:         │
                            │   WFM_SCRAPER=python in the unit        │
                            └─────────────────────────────────────────┘
```

---

## Branching

`develop` = integration (branch features off it, merge back with review);
`main` = production (auto-deploys; promote with `git merge --ff-only develop`).
Hotfixes branch off `main`, then merge `main` back into `develop`. Companion
`v*` tags are cut on `main` only.

Two remotes: `github` (public, runs CI) and `origin` (self-hosted Gitea) —
push both. A GH cron commits market refreshes **straight to `develop`**, so a
local `develop` goes stale within a day. Always fetch and merge before pushing;
check with `git rev-list --left-right --count develop...github/develop`.

## Multi-agent / delegation rules

These exist because each has already gone wrong once:

- **Repo-mutating agents run in isolated worktrees.** If an agent dies
  mid-task, launch a FRESH agent with worktree isolation — resuming the dead
  one can silently drop it into the primary checkout. Every agent verifies
  `git rev-parse --show-toplevel` before its first git write.
- **Never background a git merge/push that assumes HEAD.** A concurrent
  agent can move the checkout. Guard with
  `[ "$(git branch --show-current)" = "develop" ] && …` or use explicit refs.
- **Merging into develop from a checkout you don't control:** don't
  `git checkout develop` in place if another session might be using the
  primary checkout. Use a throwaway `git worktree add <tmp-path> develop`,
  merge there, verify (`git diff <feature-tip> <merge-commit>` should be
  empty), `git worktree remove` — leave the shared checkout's branch alone.
- **`git branch -d` checks ancestry against current HEAD, not `develop`** —
  on a shared checkout sitting on an unrelated branch it will falsely
  refuse a fully-merged branch. Confirm with
  `git merge-base --is-ancestor <branch> develop` first.
- **Clean up as you go.** After a feature branch merges: delete it locally
  and on both remotes, `git worktree remove` its worktree, prune. Keep
  `wip/unshipped-local` and any branch with a live agent.

## Cross-cutting hygiene rules (apply everywhere)

- **Cross-language logic duplication needs a parity fixture, not just a
  comment.** This repo is Rust + TypeScript + Python by necessity (companion,
  browser app, scraper) — the same heuristic sometimes has to exist in two of
  them (name/slug resolution, scoring, shared constants like a KDF iteration
  count or a category list). A `// keep these in sync` comment is not a gate;
  it silently rots. Instead: put the shared cases + expected output in
  `tests/fixtures/<name>/`, and add a test on **each** side that reads the
  same fixture and asserts against it (`sell-priority/cases.json` and
  `name-guess/cases.json` are the reference examples — grep their consuming
  tests for the pattern). If the same value only needs to match, not compute
  anything, the fixture can be a single JSON file both sides parse directly
  (see `jwt-kdf.json`, `tradeable-categories.json`, `pacing.json`,
  `limits.json`). A 2026-07 sweep found several places where the "just a
  comment" version had already drifted silently — one of them (a slug-guessing
  fallback) was a live, if narrow, bug.
- **A parity gate must test CURRENT code, or it is worse than no gate.** The
  two Rust-parity runners checked `RUST_BINARY.exists()` and then ran whatever
  was there. On 2026-08-01 that meant five green parity tests in 0.08 s against
  a binary eight days old and 17 KB different from what the source produced —
  the exact stale-green shape as the 2026-07-20 incident, wearing a passing
  test as a disguise. `tests/conftest.py` now rebuilds before comparing and
  lets Cargo decide whether that's a no-op. When you add a gate that shells a
  built artifact, the build is part of the gate. Ask what the gate does when
  the artifact is stale, not just when it's missing.
- **Ask what a value's gate can actually SEE.** `REQUEST_DELAY` sat duplicated
  in Python and Rust while both parity suites stubbed the sleeper, so nothing
  on either side could observe it. A constant inside the mocked-out part of a
  test is unguarded no matter how thorough the suite around it looks.
- **Dead exports are caught by `knip`, not by tsconfig.** `bun run knip`
  (prototype/, wired into audit.yml). Do NOT reach for `noUnusedLocals` when
  dead code turns up: it flags unused *locals*, never unused *exports*, and
  every dead-code finding this repo has actually had was an export. Measured
  2026-08-01 — turning it on produced zero diagnostics from both `tsc` and
  `svelte-check` while three dead exports were sitting in `lib/`. Removing
  App.svelte's `@ts-nocheck` to widen coverage costs 61 errors and is not
  worth it. `knip.jsonc` records the two things knip does not cover.
- **No comments that restate the code.** Comments explain *why* — the
  non-obvious constraint, the past bug they prevent. If removing a
  comment wouldn't confuse a reader, delete it.
- **No backwards-compat shims** for code that hasn't shipped yet.
  Renaming a state field? Bump the storage-key version and move on.
- **Edit existing files** in preference to creating new ones.
- **Match the scope of the request.** Don't refactor surrounding code
  while fixing a bug. Don't add features the task didn't ask for.
- **Verify in the actual runtime.** Browser changes → drive the dev
  server or Playwright. Companion changes → run it. Type-checks and
  test suites verify code correctness, not feature correctness.

## AI-written code — failure modes to catch in your own output

1. **Architectural misalignment** — does the new code follow patterns
   already in the repo, or invent a parallel approach?
2. **Happy-path bias** — error paths are ~2× less likely to be
   correct in AI-written code. Walk the failure modes explicitly:
   empty input, network error, malformed JSON, missing key,
   permission denied.
3. **Tests that pin implementation** — do they exercise the public
   contract or hard-code the current internals? The second is
   worthless.
4. **Quietly broken edge cases** — Maps with no entries, dates near
   year boundaries, sudo vs. non-sudo, empty filter strings.
5. **Verification before claiming done.** For UI changes, drive the
   browser and look at the result. "Build succeeded" ≠ "feature works."

---

## Quick reference

```fish
# Dev server (browser app)
cd prototype && bun install && bun run dev   # http://127.0.0.1:5173

# Rebuild static market.json from the existing CSV (~10 s).
# csv_to_market_json.py is the ONLY generator that produces the full
# shape (set_to_parts / relic_rewards / vault_status). Always finish a
# scrape with it — never point wfm_demand.py --json-out at the public
# market.json, it omits those keys and blanks the Sets/Relics/Vaulted
# surfaces.
python3 scripts/csv_to_market_json.py

# Full WFM scrape (~45 min, 3 req/s) → CSV only, then rebuild the snapshot.
# Production runs the Rust port; deploy/run-scrape.sh drives both steps and
# carries the truncation guard, so prefer it over calling these by hand.
companion/target/release/wfm-scrape scrape --filter "" --exclude "" \
  --min-volume 1 --out wfm_results.csv
python3 scripts/csv_to_market_json.py

# The Python scraper is the rollback and the parity reference — identical
# flags, and the two are gated against each other by tests/test_scrape_parity.py.
python3 wfm_demand.py --filter "" --exclude "" --min-volume 1 \
  --out wfm_results.csv

# Companion subcommands (all in the same binary). Grant ptrace once so
# fetch needs no sudo — re-run after every `cargo build --release`, which
# wipes the capability:
sudo setcap cap_sys_ptrace=eip companion/target/release/wfm-fetch-inventory
companion/target/release/wfm-fetch-inventory               # default = fetch inventory.json
companion/target/release/wfm-fetch-inventory login         # interactive WFM signin
companion/target/release/wfm-fetch-inventory serve         # loopback HTTP server

# Test sweeps — run ALL of these before pushing; every one is local.
cd prototype && bun run test
cd prototype && bun run knip                  # dead exports; tsconfig can't see them
pytest tests/                                 # rebuilds wfm-scrape for the parity gates
cd companion && cargo test
cd companion && cargo audit --deny warnings   # same flags as audit.yml
node scripts/sync-csp.mjs --check             # three CSP copies must agree

# The only CI gates that can't run here are the ones ABOUT the CI host:
# the glibc-2.35 floor check (this machine's glibc is far newer, so a local
# pass proves nothing) and the Windows build.

# Companion rebuild
cd companion && cargo build --release
```

