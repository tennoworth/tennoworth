# AGENTS.md - project overview

Warframe market tool for **Windows + Linux**, equal first-class targets. It
exists to be **measurably the best answer to "what to sell right now"**; a
feature that does not sharpen that answer needs a strong reason. The hosted
site is informational and takes no inventory. The desktop app reads the
inventory locally (PC-only by nature), and its longer-term direction is a
quiet, read-only in-game helper: it observes game events and suggests, and
never injects input.

Detailed rules live in per-domain files. Read the one for the area you are about
to edit **before** you edit it, and update it in the same change that
invalidates it:

- [`rust/AGENTS.md`](rust/AGENTS.md) - Rust workspace: crate boundaries,
  cross-crate invariants, Rust hygiene.
- [`rust/wfm-scrape/AGENTS.md`](rust/wfm-scrape/AGENTS.md) - DE ingest and the
  host market pipeline.
- [`rust/wfm-trading/AGENTS.md`](rust/wfm-trading/AGENTS.md) - authentication,
  order semantics, pending-plan files.
- [`rust/game-acquisition/AGENTS.md`](rust/game-acquisition/AGENTS.md) - process
  detection and the memory scan.
- [`rust/wfm-client/AGENTS.md`](rust/wfm-client/AGENTS.md) - WFM transport:
  headers, user agent, request budgets, retry and backoff.
- [`frontend/AGENTS.md`](frontend/AGENTS.md) - Svelte 5 + Vite frontend
  (desktop, overlay, and informational site).
- [`scripts/AGENTS.md`](scripts/AGENTS.md) - release, CSP, probe and
  deployment tools, and how these instruction files are maintained.

`AGENTS.local.md` is a gitignored, machine-local overlay (host names, paths,
environment quirks). Read it explicitly after the applicable instruction files
when it exists; a worktree reads the main checkout's copy. Never put that kind
of detail in a tracked file.

Maintainer material - knowledge, procedures, plans - lives in the **maintainer
home**: the `.planning/` directory of the main checkout, a local Git repository
of its own that this repository ignores. Locate it from any worktree with
`"$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")/.planning"`
and read `knowledge/INDEX.md` there before maintainer work. Procedures are
`skills/<task>/SKILL.md`; if a requested skill is missing, say so rather than
substituting another. A contributor checkout has no maintainer home, and
nothing here depends on it.

## Required design reference for UI work

Before changing UI markup, styles, layout, or interaction states, read
[`docs/design-system.md`](docs/design-system.md) completely. It governs the
desktop app, hosted site, shared components, and in-game overlay variant; its
rules take precedence over generic design suggestions. Verify affected surfaces
in both themes at narrow, short, and wide sizes, and update the reference and
living styleguide together when extending a pattern. This gate is repeated in
`rust/AGENTS.md` and `frontend/AGENTS.md` on purpose, because each is read where
UI work starts; it is the only rule these files repeat.

## Repository is the public surface

The GitHub repo carries source, user documentation, packaging recipes and CI -
nothing else. Research, audits, plans, and maintainer or host operations
runbooks live in the maintainer home, never in `docs/` or another tracked path.
A procedure CONTRIBUTING tells a contributor to follow belongs here; a note only
the maintainer needs does not. `bun scripts/check-public-surface.ts` is the
gate: it fails when ignored internal material is tracked, and when a `docs/`
document is linked from nowhere outside `docs/`. If maintainer material reaches
a commit, remove the commits and rebuild the branch - a removal commit leaves
the text in history.

## Where things live

[`docs/architecture.md`](docs/architecture.md) owns the repository map, runtime
shape, dependency direction, native responsibilities, and stored identifiers.
[`docs/ci.md`](docs/ci.md) owns the workflow inventory and cache policy.

## Delivery and integration

[CONTRIBUTING.md](CONTRIBUTING.md) owns the workflow, contribution tiers,
required evidence and check commands. Use focused checks while editing and the
complete applicable set before pushing.

`develop` is the integration branch; `github` is the authoritative remote for
maintainer checkouts and the only one to push from them. Branch off `develop`,
open the PR against `develop`, and fetch before branching or pushing. `main` is
production and auto-deploys: promotion requires explicit authorization and the
path in [docs/releasing.md](docs/releasing.md). Hotfixes branch from `main`,
merge to `main`, then `main` merges back into `develop` immediately. Fetching is
not verification: confirm divergence with
`git rev-list --left-right --count develop...github/develop`. Live ruleset IDs
are in [docs/github-rulesets/](docs/github-rulesets/); check live settings when
a task depends on them.

## Multi-agent / delegation rules

These exist because each has already gone wrong once:

- **Repo-mutating agents run in isolated worktrees** under `.worktrees/`. If an
  agent dies mid-task, launch a FRESH agent with worktree isolation - resuming
  the dead one can silently drop it into the primary checkout. Verify
  `git rev-parse --show-toplevel` before the first git write. Scratch (analysis
  output, CI logs, helper binaries, tool caches) belongs in the worktree too.
- **Never background a git merge/push that assumes HEAD.** Guard with
  `[ "$(git branch --show-current)" = "develop" ] && …` or use explicit refs.
- **Merging into develop from a checkout you don't control:** use a throwaway
  `git worktree add <tmp-path> develop`, merge there, verify
  (`git diff <feature-tip> <merge-commit>` is empty), then remove it.
- **`git branch -d` checks ancestry against HEAD, not `develop`.** Confirm with
  `git merge-base --is-ancestor <branch> develop` first.
- **Clean up as you go**, following CONTRIBUTING's integration cleanup:
  delete merged branches when authorized, remove their worktrees, prune.
  Preserve uncommitted work and any checkout still in use.
- **Verify the combined result in isolation** before integrating a large change,
  and carry verified commits through Git rather than copying files.
- **Keep large Rust gates off small temporary filesystems.** A workspace target
  takes 4-9 GiB and `cargo test` writes SQLite files through `tempfile`; a full
  tmpfs fails anywhere from ENOSPC to linker SIGBUS. Set `CARGO_TARGET_DIR` and
  `TMPDIR` to task-specific directories on a large disk, reuse them within the
  task only, and remove them afterwards (check with `du` and `df` - a later
  Cargo command once recreated a "cleaned" target).
- **Classify sandbox failures before changing code.** Mock HTTP servers need
  localhost binds, which a sandbox rejects with `PermissionDenied`. Rerun with
  loopback permission; do not weaken tests or treat EPERM/ENOSPC as product
  failures.

## Cross-cutting hygiene rules (apply everywhere)

- **Cross-language logic duplication needs a parity fixture, not a comment.**
  When the same heuristic or constant must exist in Rust and TypeScript, put
  the cases and expected output in `tests/fixtures/` and have a test on **each**
  side read them (`sell-priority/cases.json` is the reference; a value that only
  has to match can be one JSON file, like `pacing.json`). Prefer one
  implementation: the desktop preview runs the native planners as WebAssembly.
- **A gate must test CURRENT code.** A runner that only checks a built artifact
  exists reports green against a stale one, so a gate that shells an artifact
  includes its build (the pipeline gates shell `env!("CARGO_BIN_EXE_…")`).
- **Ask what a value's gate can actually SEE.** A constant inside the stubbed
  part of a test is unguarded however thorough the suite looks; the
  `pacing.json` fixture exists because both sides stubbed the sleeper.
- **A test you have never watched fail is not a gate.** Remove the behaviour it
  protects and confirm it fails, then restore.
- **Walk the failure paths**: empty input, network error, malformed JSON,
  missing key, permission denied. Test the public contract, not the internals.
- **Check the current tree before ranking work.** Logs and audits show
  symptoms; read the source to see whether the cause is still there.
- **Use the actual dead-code gates**: knip for frontend exports, cargo shear for
  Rust dependencies. Never add `@ts-nocheck` to production components.
- **Confirm a pinned action SHA declares every input you pass.** An undeclared
  input is silently ignored; read `action.yml` at the pinned commit.
- **When measuring CI, drop skipped jobs** and compare the best run with the
  median: a wide spread is usually a cache hit versus a miss.
- **No comments that restate the code.** Comments explain *why*.
- **No backwards-compat shims** for code that hasn't shipped. Shipped stored
  data is different: see "Stored and host identifiers" in architecture.md.
- **Edit existing files** in preference to creating new ones, and **match the
  scope of the request.**
- **Verify in the actual runtime.** Browser changes → the dev server or
  Playwright; native changes → run the app. Type checks and test suites verify
  code, not features.
- **Automated assistance is invisible in project artifacts.** Never mention
  an LLM, coding agent, assistant, model, vendor, or automated generation in
  commit messages, authorship or co-author trailers, pull requests, issues,
  reviews, changelogs, release notes, code comments, generated files, or any
  other repository or collaboration artifact. Write every artifact solely in
  terms of the change and its rationale, with ordinary human authorship. This
  forbids provenance and self-attribution, not discussion of AI where AI is the
  subject. `scripts/check-attribution.ts` gates commit messages and pull
  request bodies.
