# AGENTS.md - project overview

Cross-platform **Windows + Linux** Warframe inventory + market dashboard
- the no-Overwolf alternative to AlecaFrame. Windows and Linux are equal,
first-class targets (not Linux-first). Inventory is acquired by a local
memory-scan companion - PC-only by nature. Overlaps with browse.wf and
warframe.me on inventory display - must be **measurably better at
"what to sell right now"** to justify existing.

Detailed rules live in per-domain files. Read the one for the area
you're editing **before** you start writing code there:

- [`rust/AGENTS.md`](rust/AGENTS.md) - Rust workspace (Tauri
  desktop app + wfm-core + host pipeline). Memory scan, JWT crypto, WFM
  API map, cross-platform gotchas.
- [`frontend/AGENTS.md`](frontend/AGENTS.md) - Svelte 5 + Vite
  frontend (desktop, overlay, and informational site). Svelte 5 reactivity gotchas,
  storage choices, CSP.
- [`scripts/AGENTS.md`](scripts/AGENTS.md) - release, CSP, probe, and deployment tools and shared fixtures.

## Required design reference for UI work

Before changing UI markup, styles, layout, or interaction states, read
[`docs/design-system.md`](docs/design-system.md) completely. It governs the
desktop app, hosted site, shared components, and in-game overlay variant.
Reuse the established tokens and patterns; project-specific visual rules take
precedence over generic design suggestions. Preserve feature behavior and
unfinished edits, document intentional exceptions, and verify affected surfaces
in both themes at narrow, short, and wide sizes. Update the reference and living
styleguide together when extending a pattern.

### Local instruction setup

The root and per-domain instruction files are intentionally untracked. Before
starting UI work in a fresh clone or worktree, copy the current root instructions
and applicable domain instructions from the maintainer's designated checkout,
preserving their relative paths. Read and compare them with the source before
editing; do not overwrite differing local instructions without reviewing them.
Verify that `docs/design-system.md` exists in the target revision. If instructions
or the reference are missing, resolve setup before UI edits, not by inventing
replacement rules. Repeat this check for each new worktree; do not assume Git
copied ignored files or an already-running session has reread changed guidance.
Keep these instruction files ignored; the design reference is public source
documentation. Do not change another active worktree's instructions in place.

An instruction-file edit cannot travel through a worktree or a pull request:
these files are ignored, so a fresh worktree does not contain them at all
(`ls AGENTS.md` fails there). Edit them in the maintainer's checkout, copy the
file aside first, and show the diff - there is no version control to undo a
mistake. Other clones, including the Windows build host, hold their own copies
and need the same edit applied by hand.

---

## What lives where

```
rust/       Rust workspace - tennoworth-desktop (the product, Tauri)
                 drives wfm-core over IPC; wfm-scrape is the host pipeline
frontend/       Svelte 5 + Vite desktop/overlay UI and static informational site
frontend/public/market.json    central artifact: the WFM snapshot
scripts/         release, CSP, probe, deployment, and fixture tools
deploy/          self-host kit for the production LXC: Caddyfile, setup, and
                 scrape/app/web-pull units
tests/           shared cross-language fixtures (Rust↔TS parity); no pytest
.github/workflows/  release-desktop (desktop artifacts), build-web,
                     build-scrape, build-usage, audit, ui-smoke, and the
                     on-demand ocr-windows-test and publish-wfm-policy
.github/actions/    shared composite actions (setup-rust,
                     setup-windows-ocr, publish-rolling-release) the
                     workflows above call into
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
       ┌── informational site (frontend/) ──┐    market.json
       │  market browse + desktop showcase    │    (refreshed on the box)
       │  no accounts, no files, no scan      │
       └──────────────────────────────────────┘
                            ▲
                            │ GET market.json
              ┌─────────────┴────────────────────────────┐
              │  wfm-scrape scrape  (Rust)               │
              │  → CSV → wfm-scrape build                │
              │  (systemd timer on the box, 2h;          │
              │   release prep refreshes repo copy)      │
              └──────────────────────────────────────────┘
```

The standalone companion CLI (`wfm-fetch-inventory` with `fetch`/`login`/
`serve`) was removed on 2026-08-02: the desktop app is the only interactive
product, and the site is informational-only. The dormant advisor command and
implementation were subsequently removed.

---

## Delivery and verification

[CONTRIBUTING.md](CONTRIBUTING.md) owns the implementation workflow, contribution
risk tiers, required evidence, and executable check commands. Use focused checks
during editing and the complete applicable set before pushing. Documentation-only
changes do not require application builds. Do not duplicate the check matrix here.
[docs/architecture.md](docs/architecture.md) owns module boundaries;
[docs/releasing.md](docs/releasing.md) owns desktop publication and recovery.
Update those references with changes to the corresponding workflow or architecture.

### CI caches are branch-scoped, and pull requests must not save

A cache only counts for a pull request when it lives on the default branch
(`develop`). A `pull_request` run writes its caches to `refs/pull/<n>/merge`,
which no other pull request and no branch may restore, so PR-written caches are
stranded as soon as that PR closes. Pull requests are therefore restore-only
(`save-if` in `setup-rust`, `actions/cache/restore` in `setup-windows-ocr`), and
`audit`/`ui-smoke` seed `develop` on pushes that touch a file which rotates a
key.

Measured 2026-09-13: with only a weekly cron seeding, every pull request logged
`No cache found` while 9.46 GiB of the repository's 11.29 GiB sat unreadable on
merge refs against a 10 GB ceiling; Rust job time on one unchanged commit went
12m10s cold to 4m32s warm once the seed existed. A cold pull request is fixed by
seeding the default branch, never by letting PRs save again - that upload
traffic is what evicts the seed in the first place. History and measurements:
[docs/rust-toolkit-2026-lessons.md](docs/rust-toolkit-2026-lessons.md).

## Branching

`develop` = integration (branch features off it, merge back with review);
`main` = production (auto-deploys; promote with `git merge --ff-only develop`).
Hotfixes branch off `main`, then merge `main` back into `develop`. Desktop
`desktop-v*` tags are cut on `main` only.

**A production PR does not make GitHub's merge button safe.** Previous
promotions made merge commits on `main` that were never merged back, so
`main` stopped being an ancestor of `develop` and the documented ff-only
promotion became impossible. Before opening a production PR, require
`git merge-base --is-ancestor main develop`. If it fails, merge `main` back
into `develop` first and verify the ancestry merge changes no files when it
contains promotion commits only (`git diff <old-develop> <sync-commit>` must
be empty). After the PR's checks and required approval pass, advance `main`
with an explicit non-forced `develop:main` push; that records the PR as merged
without adding another production merge commit. Never use this path to bypass
review unless the user explicitly authorizes the production merge.

`github` is the authoritative remote and the only remote to push. Do not mirror branches to any other remote. Always fetch GitHub before branching or pushing,
then check with
`git rev-list --left-right --count develop...github/develop`.

### Maintainer merge policy (verified 2026-09-05)

The maintainer works solo and prefers routine integration without repeated
protection changes or approval stops. Repository auto-merge is enabled. For
ordinary authorized `develop` merges, use auto-merge while applicable checks
finish. If the maintainer explicitly asks to merge despite pending checks,
use the existing PR bypass; do not edit rulesets or ask again for the same
already-authorized override.

- Integration ruleset `21067958` requires a PR and `audit-gate`/`ui-gate`, with
  zero required approvals. Only `PedroAmorimP` (user ID `40967190`) has a
  `pull_request` bypass. Other contributors have no bypass.
- History ruleset `22310181` blocks deletion and non-fast-forward updates to
  `develop` and `main`, with no bypass actors. A maintainer integration override
  cannot bypass those protections.
- Production ruleset `21067966` and desktop-tag ruleset `21067950` were retained.
  Production still declares one approval and both gates, with its pre-existing
  administrator bypass for the documented non-forced fast-forward promotion.
  Production promotion still requires explicit authorization.
- `audit` and `ui-smoke` run on PRs, schedules, manual dispatch, and pushes to
  `develop` that touch a file which rotates a CI cache key (Cargo manifests and
  lockfiles, `frontend/bun.lock`, the setup actions). The `develop` trigger
  seeds the default-branch caches pull requests restore from; it is not a
  substitute for the PR run and it does not cover `main`. Verify the applicable
  checks on the production PR before promotion; do not assume pushing to `main`
  reruns them.
- Ruleset bypass applies to the whole ruleset. Keep history protections
  separate from bypassable merge requirements. Snapshot and verify settings
  before/after changes; do not broaden bypass to all admins or contributors.

Reviewable snapshots and rollback instructions are in `docs/github-rulesets/`.
PR #77 records the applied policy. Treat the dated snapshot as context; check
live settings when a task depends on them. Original snapshots exist at
commit `088b27ac5e91c7553d2efc91003f0c0a2cccfbe4`.

## Multi-agent / delegation rules

These exist because each has already gone wrong once:

- **Repo-mutating agents run in isolated worktrees.** If an agent dies
  mid-task, launch a FRESH agent with worktree isolation - resuming the dead
  one can silently drop it into the primary checkout. Every agent verifies
  `git rev-parse --show-toplevel` before its first git write. Scratch belongs in
  the worktree too - analysis output, downloaded CI logs, helper binaries, tool
  caches. Untracked scratch left in the primary checkout shows up in
  `git status` and competes with whichever session is using it.
- **Never background a git merge/push that assumes HEAD.** A concurrent
  agent can move the checkout. Guard with
  `[ "$(git branch --show-current)" = "develop" ] && …` or use explicit refs.
- **Merging into develop from a checkout you don't control:** don't
  `git checkout develop` in place if another session might be using the
  primary checkout. Use a throwaway `git worktree add <tmp-path> develop`,
  merge there, verify (`git diff <feature-tip> <merge-commit>` should be
  empty), `git worktree remove` - leave the shared checkout's branch alone.
- **`git branch -d` checks ancestry against current HEAD, not `develop`** -
  on a shared checkout sitting on an unrelated branch it will falsely
  refuse a fully-merged branch. Confirm with
  `git merge-base --is-ancestor <branch> develop` first.
- **Clean up as you go.** After a feature branch merges: delete it locally
  and on GitHub when authorized, remove its worktree, and prune. Preserve
  uncommitted work and any checkout still in use. Follow the integration cleanup
  procedure in CONTRIBUTING.md; no branch name is a permanent cleanup exemption.
- **Verify integrated changes in isolation.** For large changes, use an isolated
  worktree and run the complete applicable checks from CONTRIBUTING.md before
  integration. Verify the combined result, not only its independent patches.
  Carry the verified commits through Git rather than copying files into another
  active checkout. Documentation-only edits do not require native or UI gates.
- **Keep large Rust gates off small temporary filesystems.** A workspace Cargo
  target can
  consume 4–9 GiB, and `cargo test` also creates SQLite/files through
  `tempfile`; once full, failures range from honest ENOSPC to linker SIGBUS.
  For a final gate, set both `CARGO_TARGET_DIR` and `TMPDIR` to unique
  task-specific directories on the home filesystem. Reuse that target across
  checks and revisions within the same task, never across concurrent tasks.
  `probe-smoke-linux.sh`
  honors `CARGO_TARGET_DIR` directly; no target symlink is needed.
  Afterwards, clean the exact Cargo target, remove the task temp dir,
  and verify with `du`, `df`, and `git status` - an earlier "cleaned" target
  was silently recreated by a later Cargo command.
- **Classify sandbox failures before changing code.** The desktop test suite's
  mock HTTP servers need localhost binds, which the filesystem/network sandbox
  rejects with `PermissionDenied`. Rerun the same command with loopback
  permission and the task-specific `TMPDIR`; do not weaken the tests or treat
  EPERM/ENOSPC as product failures.

## Cross-cutting hygiene rules (apply everywhere)

- **Cross-language logic duplication needs a parity fixture, not just a
  comment.** This repo is Rust + TypeScript by necessity (companion, browser
  app) - the same heuristic sometimes has to exist in both (name/slug
  resolution, scoring, shared constants like a KDF iteration count or a
  category list). A `// keep these in sync` comment is not a gate;
  it silently rots. Instead: put the shared cases + expected output in
  `tests/fixtures/<name>/`, and add a test on **each** side that reads the
  same fixture and asserts against it (`sell-priority/cases.json` and
  `name-guess/cases.json` are the reference examples - grep their consuming
  tests for the pattern). If the same value only needs to match, not compute
  anything, the fixture can be a single JSON file both sides parse directly
  (see `jwt-kdf.json`, `tradeable-categories.json`, `pacing.json`,
  `limits.json`). A 2026-07 sweep found several places where the "just a
  comment" version had already drifted silently - one of them (a slug-guessing
  fallback) was a live, if narrow, bug.
- **A parity gate must test CURRENT code, or it is worse than no gate.** The
  two Rust-parity runners checked `RUST_BINARY.exists()` and then ran whatever
  was there. On 2026-08-01 that meant five green parity tests in 0.08 s against
  a binary eight days old and 17 KB different from what the source produced -
  the exact stale-green shape as the 2026-07-20 incident, wearing a passing
  test as a disguise. `tests/conftest.py` used to rebuild before comparing and
  let Cargo decide whether that's a no-op. When you add a gate that shells a
  built artifact, the build is part of the gate. Ask what the gate does when
  the artifact is stale, not just when it's missing. (The Rust fixture gates
  in `rust/wfm-scrape/tests/` shell `env!("CARGO_BIN_EXE_wfm-scrape")`,
  which cargo rebuilds - the same guarantee, without the Python harness.)
- **Ask what a value's gate can actually SEE.** `REQUEST_DELAY` sat duplicated
  in Python and Rust while both parity suites stubbed the sleeper, so nothing
  on either side could observe it - the `pacing.json` fixture is now the only
  thing that catches a one-sided bump. A constant inside the mocked-out part
  of a test is unguarded no matter how thorough the suite around it looks.
- **Use the actual dead-code gates.** Frontend exports are checked by knip;
  unused Rust dependencies by cargo shear. Type checking does not replace them.
  Do not introduce `@ts-nocheck` to suppress production component diagnostics.
- **Confirm a pinned action SHA declares every input you pass.** An undeclared
  input is not an error: the runner warns and ignores it, so the step silently
  does not do what it says. 2026-09-13: `save-if` was confirmed present in the
  pinned rust-cache v2.9.2, and `cache-hit`/`cache-primary-key` in the pinned
  cache action, before either was relied on. Read the action's own `action.yml`
  at the pinned commit, not from its default branch.
- **When measuring CI, drop skipped jobs.** A job that was never applicable
  reports a near-zero duration, and including those pulls the median toward
  zero and hides real cost. Compare the best run against the median as well: a
  wide spread is the signature of a cache hit versus a cache miss, and that
  spread is usually the finding.
- **No comments that restate the code.** Comments explain *why* - the
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
- **Automated assistance is invisible in project artifacts.** Never mention
  an LLM, coding agent, assistant, model, vendor, or automated generation in
  commit messages, authorship or co-author trailers, pull requests, issues,
  reviews, changelogs, release notes, code comments, generated files, or any
  other repository or collaboration artifact. Write every artifact solely in
  terms of the change and its rationale, with ordinary human authorship. This
  forbids provenance and self-attribution; it does not forbid product or
  security documentation from discussing AI when AI is itself the subject.

## AI-written code - failure modes to catch in your own output

1. **Architectural misalignment** - does the new code follow patterns
   already in the repo, or invent a parallel approach?
2. **Happy-path bias** - error paths are ~2× less likely to be
   correct in AI-written code. Walk the failure modes explicitly:
   empty input, network error, malformed JSON, missing key,
   permission denied.
3. **Tests that pin implementation** - do they exercise the public
   contract or hard-code the current internals? The second is
   worthless.
4. **Quietly broken edge cases** - Maps with no entries, dates near
   year boundaries, sudo vs. non-sudo, empty filter strings.
5. **Verification before claiming done.** For UI changes, drive the
   browser and look at the result. "Build succeeded" ≠ "feature works."
6. **Named-vs-default imports in `@ts-nocheck` files.** A `import X from`
   on a named-only export compiles (the file is untyped) and binds
   `undefined`, silently no-op'ing the feature. 2026-08-03: a default
   `LIQUID_VOL` import shipped past `bun run check` and the pick-tag just
   never rendered. Type-check can't see these - only a browser/runtime
   check (or a named-import convention) can.
7. **A test you have never watched fail is not a gate.** After adding one,
   remove the behaviour it claims to protect and confirm it fails. 2026-09-13:
   the probe watchdog test was checked this way - with the watchdog removed no
   report is written at all and the test fails loudly. A test that passes both
   with and without the fix restates the code instead of guarding it.
8. **Check the current tree before ranking the work.** A run-history audit shows
   symptoms; it does not show whether the cause is still present. 2026-09-13:
   the "flaky five-minute probe hang" was ranked the highest-value fix, and
   `3a8f759` had already fixed both the product fault and the harness fault
   behind the timeout. Read the source of the thing being prioritised, not only
   its logs.

---

## Commands and runtime setup

Use [CONTRIBUTING.md](CONTRIBUTING.md) for setup, builds, and applicable checks.
Use [docs/releasing.md](docs/releasing.md) for snapshot preparation and releases.
For host operations, `deploy/run-scrape.sh` drives scrape and build together;
`wfm-scrape build` remains the complete snapshot/catalog generator.
The Windows runbook documents platform build and probe requirements.
