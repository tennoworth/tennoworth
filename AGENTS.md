# AGENTS.md - project overview

Cross-platform **Windows + Linux** Warframe inventory + market dashboard
- the no-Overwolf alternative to AlecaFrame. Windows and Linux are equal,
first-class targets (not Linux-first). Inventory is acquired by a local
memory-scan companion - PC-only by nature. Overlaps with browse.wf and
warframe.me on inventory display - must be **measurably better at
"what to sell right now"** to justify existing.

Detailed rules live in per-domain files. Read the one for the area you are about
to edit **before** you edit it:

- [`rust/AGENTS.md`](rust/AGENTS.md) - Rust workspace: crate map, cross-crate
  invariants, Rust hygiene.
- [`rust/wfm-scrape/AGENTS.md`](rust/wfm-scrape/AGENTS.md) - DE ingest and the
  host market pipeline.
- [`rust/wfm-core/AGENTS.md`](rust/wfm-core/AGENTS.md) - authentication, order
  semantics, pending-plan recovery, memory scan.
- [`rust/wfm-client/AGENTS.md`](rust/wfm-client/AGENTS.md) - WFM transport:
  headers, user agent, request budgets, retry and backoff.
- [`frontend/AGENTS.md`](frontend/AGENTS.md) - Svelte 5 + Vite frontend
  (desktop, overlay, and informational site).
- [`scripts/AGENTS.md`](scripts/AGENTS.md) - release, CSP, probe, and
  deployment tools and shared fixtures.

`AGENTS.local.md` is a gitignored, machine-local overlay loaded after these
files. If it exists, read it and follow it - it carries host, path and
environment details that do not belong in the repository.

## Required design reference for UI work

Before changing UI markup, styles, layout, or interaction states, read
[`docs/design-system.md`](docs/design-system.md) completely. It governs the
desktop app, hosted site, shared components, and in-game overlay variant.
Reuse the established tokens and patterns; project-specific visual rules take
precedence over generic design suggestions. Preserve feature behavior and
unfinished edits, document intentional exceptions, and verify affected surfaces
in both themes at narrow, short, and wide sizes. Update the reference and living
styleguide together when extending a pattern.

This gate is repeated in `rust/AGENTS.md` and `frontend/AGENTS.md` on purpose:
missing it is expensive, and each copy is read where UI work starts. Concise
shared invariants are the permitted kind of repetition - what these files must
not do is restate a *specification* that already has an owner, or let a copy
drift from it.

### Local instruction setup

The instruction files are tracked project policy. Read the applicable file
before editing in its area, and update it in the same change that invalidates
it.

Machine-specific detail - host names, local paths, environment quirks - belongs
in the gitignored `AGENTS.local.md` overlay, never in a tracked file. Copy
[`AGENTS.local.md.example`](AGENTS.local.md.example) and fill in what applies to
that checkout; each clone, including the Windows build host, keeps its own.

A fresh worktree or clone does not receive `AGENTS.local.md`: it is ignored, so
Git does not copy it. Create it from the example when the work needs
host-specific detail. The instruction loader used here reads the overlay
automatically after the base files; other tools may not, so treat it as
configuration the reader has to be told about rather than something guaranteed
to have been loaded.

`docs/design-system.md` is public source documentation and must exist in the
revision you are working in. If it, or an applicable instruction file, is
missing, resolve that before UI edits rather than inventing replacement rules.

---

## Where things live

[`docs/architecture.md`](docs/architecture.md) owns this: the repository map,
runtime shape, dependency direction, native responsibilities, and the CI
inventory. Read it there rather than maintaining a second copy here.

## Delivery, integration and CI

[CONTRIBUTING.md](CONTRIBUTING.md) owns the implementation workflow,
contribution risk tiers, required evidence, and executable check commands. Use
focused checks during editing and the complete applicable set before pushing.
Documentation-only changes do not require application builds. Do not duplicate
the check matrix here.

`develop` is the integration branch; `github` is the authoritative remote for
maintainer checkouts and the only one to push from them. Branch off `develop`,
open the PR against `develop`, and fetch before branching or pushing. Fork
contributors work from their own remote and upstream as described in
CONTRIBUTING.md. `main` is production and auto-deploys: promotion requires
explicit authorization and the checked, approved path in
[docs/releasing.md](docs/releasing.md). Live ruleset IDs, bypass scope, and
rollback are in [docs/github-rulesets/](docs/github-rulesets/) - check live
settings when a task depends on them rather than trusting a dated snapshot.

Hotfixes are the one exception to branching off `develop`: branch them from
`main`, merge to `main`, then merge `main` back into `develop` immediately.
Fetching is not verification - after a fetch, confirm divergence with
`git rev-list --left-right --count develop...github/develop` instead of assuming
the branches are level.

CI caches are branch-scoped and pull requests are restore-only: a cache only
counts for a PR when it lives on the default branch, so a cold PR is fixed by
seeding `develop`, never by letting PRs save again. Measurements, trigger paths
and the rationale are in
[docs/rust-toolkit-2026-lessons.md](docs/rust-toolkit-2026-lessons.md).

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
  active checkout.
- **Keep large Rust gates off small temporary filesystems.** A workspace Cargo
  target can consume 4-9 GiB, and `cargo test` also creates SQLite files through
  `tempfile`; once the temporary filesystem fills, failures range from honest
  ENOSPC to linker SIGBUS. For a final gate, set both `CARGO_TARGET_DIR` and
  `TMPDIR` to unique task-specific directories on a large local filesystem.
  Reuse that target across checks and revisions within the same task, never
  across concurrent tasks. `probe-smoke-linux.sh` honors `CARGO_TARGET_DIR`
  directly; no target symlink is needed. Afterwards, clean the exact Cargo
  target, remove the task temp dir, and verify with `du`, `df`, and
  `git status` - an earlier "cleaned" target was silently recreated by a later
  Cargo command.
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
  test as a disguise. When you add a gate that shells a built artifact, the
  build is part of the gate. Ask what the gate does when the artifact is stale,
  not just when it's missing. (The Rust fixture gates in
  `rust/wfm-scrape/tests/` shell `env!("CARGO_BIN_EXE_wfm-scrape")`, which cargo
  rebuilds - the same guarantee, without the retired Python harness.)
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
  forbids provenance and self-attribution: it is about how a change is
  attributed, not about hiding that these tools are used. It does not forbid
  product or security documentation from discussing AI when AI is itself the
  subject, nor these governance and instruction files, which define the rule.

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

`bun scripts/check-agent-instructions.ts` (from the repository root) verifies
that these instruction files' links resolve and that every `*/AGENTS.md` is
reachable from the router above.
