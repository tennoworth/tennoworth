# AGENTS.md — project overview

Cross-platform **Windows + Linux** Warframe inventory + market dashboard
— the no-Overwolf alternative to AlecaFrame. Windows and Linux are equal,
first-class targets (not Linux-first). Inventory is acquired by a local
memory-scan companion — PC-only by nature. Overlaps with browse.wf and
warframe.me on inventory display — must be **measurably better at
"what to sell right now"** to justify existing.

Detailed rules live in per-domain files. Read the one for the area
you're editing **before** you start writing code there:

- [`companion/AGENTS.md`](companion/AGENTS.md) — Rust workspace (Tauri
  desktop app + wfm-core + host pipeline). Memory scan, JWT crypto, WFM
  API map, cross-platform gotchas.
- [`prototype/AGENTS.md`](prototype/AGENTS.md) — Svelte 5 + Vite
  browser app (the informational site). Svelte 5 reactivity gotchas,
  storage choices, CSP.
- [`scripts/AGENTS.md`](scripts/AGENTS.md) — the three TypeScript tools
  (sync-csp, check-panic-sites, check-probe-report) and the shared fixtures.

---

## What lives where

```
companion/       Rust workspace — tennoworth-desktop (the product, Tauri)
                 drives wfm-core over IPC; wfm-scrape is the host pipeline
prototype/       Svelte 5 + Vite informational site, deployed as static
prototype/public/market.json    central artifact: the WFM snapshot
scripts/         three TypeScript tools (sync-csp.ts, check-panic-sites.ts,
                 check-probe-report.ts); Python retired 2026-08
packaging/aur/   AUR recipes (tennoworth, tennoworth-bin) + their .install hooks
deploy/          self-host kit for the production LXC: Caddyfile, setup, scrape/web-pull units, plus the signed apt+dnf repo publisher (setup-repo.sh, pull-packages.sh)
tests/           shared cross-language fixtures (Rust↔TS parity); no pytest
.github/workflows/  refresh-market (cron), release-desktop (tag → desktop +
                     AUR), build-web, audit
.github/actions/    shared composite actions (setup-rust,
                     publish-rolling-release) the workflows above call into
SECURITY.md      threat model + what we do and don't commit to
```

## Components at a glance

```
┌─ Warframe game ──────────────────────────────────┐
│   /proc/<pid>/mem  or  ReadProcessMemory         │
└────────────────────────┬─────────────────────────┘
                         │ scrape accountId+nonce
                         ▼
        ┌── desktop app (tennoworth-desktop, Tauri) ──┐
        │  same-origin webview (the SPA)              │
        │  scan_inventory → wfm-core → IPC            │
        │  wfm_login → wfm-jwt.enc (AES, Rust-side)   │
        │  listing / orders → wfm-core → WFM          │
        └────────────────────────┬────────────────────┘
                                 │
                 ┌───────────────┴──────────────────┐
                 ▼                                 ▼
       ┌── informational site (prototype/) ──┐    market.json
       │  market browse + desktop showcase    │    (refreshed on the box)
       │  no accounts, no files, no scan      │
       └──────────────────────────────────────┘
                            ▲
                            │ GET market.json
              ┌─────────────┴────────────────────────────┐
              │  wfm-scrape scrape  (Rust)               │
              │  → CSV → wfm-scrape build                │
              │  (systemd timer on the box, 2h;          │
              │   GH cron refreshes repo copy)           │
              └──────────────────────────────────────────┘
```

The standalone companion CLI (`wfm-fetch-inventory` with `fetch`/`login`/
`serve`) was removed on 2026-08-02: the desktop app is the only interactive
product, and the site is informational-only. `wfm-core` keeps the assistant
relay dormant — no UI surfaces it.

---

## Branching

`develop` = integration (branch features off it, merge back with review);
`main` = production (auto-deploys; promote with `git merge --ff-only develop`).
Hotfixes branch off `main`, then merge `main` back into `develop`. Desktop
`desktop-v*` tags are cut on `main` only.

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
- **Full-gate worktree verification for large changes.** Before applying a
  multi-file change to the primary checkout, run the complete gate stack
  (check + tests + knip + cargo check) in a throwaway worktree and copy the
  verified result across. Individual tests passing is not the same as the
  set integrating — and pair-reviewing each change independently catches
  what a single reviewer (or `bun run check` in a `@ts-nocheck` file)
  cannot.

## Cross-cutting hygiene rules (apply everywhere)

- **Cross-language logic duplication needs a parity fixture, not just a
  comment.** This repo is Rust + TypeScript by necessity (companion, browser
  app) — the same heuristic sometimes has to exist in both (name/slug
  resolution, scoring, shared constants like a KDF iteration count or a
  category list). A `// keep these in sync` comment is not a gate;
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
  test as a disguise. `tests/conftest.py` used to rebuild before comparing and
  let Cargo decide whether that's a no-op. When you add a gate that shells a
  built artifact, the build is part of the gate. Ask what the gate does when
  the artifact is stale, not just when it's missing. (The Rust fixture gates
  in `companion/wfm-scrape/tests/` shell `env!("CARGO_BIN_EXE_wfm-scrape")`,
  which cargo rebuilds — the same guarantee, without the Python harness.)
- **Ask what a value's gate can actually SEE.** `REQUEST_DELAY` sat duplicated
  in Python and Rust while both parity suites stubbed the sleeper, so nothing
  on either side could observe it — the `pacing.json` fixture is now the only
  thing that catches a one-sided bump. A constant inside the mocked-out part
  of a test is unguarded no matter how thorough the suite around it looks.
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
6. **Named-vs-default imports in `@ts-nocheck` files.** A `import X from`
   on a named-only export compiles (the file is untyped) and binds
   `undefined`, silently no-op'ing the feature. 2026-08-03: a default
   `LIQUID_VOL` import shipped past `bun run check` and the pick-tag just
   never rendered. Type-check can't see these — only a browser/runtime
   check (or a named-import convention) can.

---

## Quick reference

```fish
# Dev server (browser app)
cd prototype && bun install && bun run dev   # http://127.0.0.1:5173

# Rebuild static market.json from the existing CSV (~10 s).
# `wfm-scrape build` is the ONLY generator of the full shape
# (set_to_parts / relic_rewards / vault_status) AND wfstat-catalog.json.
# Always finish a scrape with it.
companion/target/release/wfm-scrape build

# Full WFM scrape (~45 min, 3 req/s) → CSV only, then rebuild the snapshot.
# deploy/run-scrape.sh drives both steps and carries the truncation guard,
# so prefer it over calling these by hand.
companion/target/release/wfm-scrape scrape --filter "" --exclude "" \
  --min-volume 1 --out wfm_results.csv
companion/target/release/wfm-scrape build

# Companion subcommands. Grant ptrace once so the desktop scan needs no
# sudo — re-run after every `cargo build --release`, which wipes the
# capability. The desktop app needs the same grant on Linux:
sudo setcap cap_sys_ptrace=eip companion/target/release/tennoworth-desktop

# Test sweeps — run ALL of these before pushing; every one is local.
cd prototype && bun run test
cd prototype && bun run knip                  # dead exports; tsconfig can't see them
cd companion && cargo test                    # includes the wfm-scrape fixture gates
cd companion && cargo audit --deny warnings   # same flags as audit.yml
bun scripts/sync-csp.ts --check              # three CSP copies must agree
bash scripts/probe-smoke-linux.sh             # real app under xvfb + probe gate (needs xvfb-run)

# The only CI gates that can't run here are the ones ABOUT the CI host:
# the glibc-2.35 floor check (this machine's glibc is far newer, so a local
# pass proves nothing) and the Windows build + Windows probe smoke (ui-smoke.yml).

# Companion rebuild
cd companion && cargo build --release
```

