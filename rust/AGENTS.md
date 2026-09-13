# rust/ - Rust workspace: desktop app + wfm-core + host pipeline

## Required design reference

Before changes affecting desktop UI, window layout, or overlay presentation,
read [`../docs/design-system.md`](../docs/design-system.md) completely. Follow
its shared visual contract and explicit overlay variant; preserve transparency,
placement, focus, and Windows/Linux behavior. Webview edits also require the
frontend instructions. Verify native behavior in the relevant runtime rather
than inferring it from browser tests.

This gate is repeated in the root and frontend instruction files on purpose:
missing it is expensive, and each copy is read where UI work starts.

## Workspace map

Cargo WORKSPACE with seven members (`target/` shared). The standalone player CLI
(`wfm-fetch-inventory` with fetch/login/serve) was removed on 2026-08-02 - the
desktop app is the only adapter.

Read the file for the crate you are about to edit:

- [`wfm-scrape/AGENTS.md`](wfm-scrape/AGENTS.md) - DE ingest and the host market
  pipeline.
- [`wfm-core/AGENTS.md`](wfm-core/AGENTS.md) - acquisition, memory scan,
  authentication, order semantics, pending-plan recovery.
- [`wfm-client/AGENTS.md`](wfm-client/AGENTS.md) - WFM transport: headers, user
  agent, request budgets, retry and backoff.

The crates:

- `wfm-core/` - the reusable core: process detection + memory scan (with a
  single-flight scan guard), DE inventory fetch, WFM auth + encrypted-JWT
  storage, the listing/order service, pending-plan persistence, and recovery.
  **No interactive terminal I/O** - the desktop shell hands the passphrase in
  as a parameter over IPC.
- `market-math/` - pure market-data heuristics. No I/O, no deps, no clocks -
  keep it that way. (Ported from the retired wfm_demand.py; its tests were
  1:1 ports of tests/test_wfm_demand.py, which died with Python in 2026-08.)
  The sell-priority scoring mirrors the SPA's canonical `sell-priority.ts`
  and is parity-gated against it on a shared fixture.
- `wfm-scrape/` - host-only pipeline binary; the ONLY market pipeline (Python
  retired 2026-08). `scrape` runs the full WFM scrape to CSV; `build` renders
  `market.json` + `wfstat-catalog.json`. Fixture regression gates in its
  `tests/` dir shell the freshly-built binary (`env!("CARGO_BIN_EXE_wfm-scrape")`)
  against the frozen fixtures in tests/fixtures/{scrape,convert} - cargo
  rebuilds the binary before they run, so a stale one cannot green them.
- `wfm-client/` - shared WFM transport primitives: descriptive user agents,
  headers, request budgets, policy enforcement, envelope unwrapping, and retry
  backoff. Share primitives only - do not grow it into an abstraction that
  swallows authed order mutation.
- `tennoworth-desktop/` - Tauri v2 desktop shell; the app users actually
  install on Linux and Windows. Drives wfm-core over IPC, so it has no HTTP
  server, no session token and no browser Local-Network-Access step. The
  passphrase arrives from the webview - which is why wfm-core must stay free
  of interactive terminal I/O.
- `market-domain/` - native decision contracts, inventory normalization,
  scoring, and planners; shared fixtures and generated frontend declarations.
- `tennoworth-usage/` - opt-in installation-count service.

Use ../CONTRIBUTING.md for builds and required checks, ../docs/architecture.md
for module boundaries, and ../docs/wfm-access.md for WFM request policy.

## Cross-crate invariants

These bind every crate here, including callers:

- **Every WFM HTTP attempt, including retries, goes through
  `wfm-client::transport`.** Do not add independent sleeps or retry loops
  anywhere else. The current request-budget and reconciliation policy is in
  ../docs/wfm-access.md.
- **Mutation reconciliation stays in the trading services.** Never blindly
  retry an ambiguous create; preserve pending-plan recovery and classify
  outcomes using the policy in ../docs/wfm-access.md.
- **The listing caps have one home and two enforcement points.**
  `MAX_PLAN_ITEMS`, `MIN_PLATINUM` and `MAX_PLATINUM` are defined in `wfm-core`,
  and the desktop edit-order command in
  `tennoworth-desktop/src/commands/listing.rs` enforces the same cap. Change
  them together and cover the pair with a test - a comment is not a gate.
  Details in [wfm-core/AGENTS.md](wfm-core/AGENTS.md).
- **Nullable auxiliary market maps are not a broken snapshot.** Desktop cache
  loading - `tennoworth-desktop/src/services/sellables.rs` - treats explicit
  `null` for optional `catalog`, `path_to_info`, `usage`, and `set_to_parts` as
  empty/neutral while keeping `items` strict. Otherwise one optional surface can
  discard current market rows and silently fall back to the bundled snapshot.
  Keep a load-level test with a current-only item; a serde field test alone
  cannot prove fallback did not happen.

  This lives here, not in the pipeline file that writes the snapshot, because
  the rule constrains the desktop reader.

---

## Hard invariants - break these and we ship a regression

### The app never prints secrets
`accountId` and `nonce` are session secrets while a play session is
live. The JWT is a multi-month bearer credential. Keep them out of
stdout/stderr at all costs. If you add a new log line, audit it.

### `setcap` is wiped on file replacement
Linux clears file capabilities whenever the binary is replaced. Every
`cargo build --release` therefore wipes `cap_sys_ptrace`. Document
this in any "how to run the app" instructions you write.

### The desktop build embeds `dist-desktop` at compile time
`cargo build` bakes `frontend/dist-desktop` (via `frontendDist`) into the
binary. Rebuild the frontend (`bun run build:desktop`) FIRST and let cargo
run after - running the two in parallel can embed a stale SPA, so the app
looks unchanged after a "rebuild" (2026-08-03 near-miss).

### Keyring-less Linux silently degrades remember-on-device
On a box with no Secret Service daemon (or a locked wallet), the OS-keyring
"remember" path is best-effort: stderr logs
`keyring read failed (falling back to passphrase)` and the app falls back
to a per-launch passphrase prompt. That log line is the diagnostic, not a bug.

### Linux `/proc/<pid>/comm` truncates at 15 chars
`Warframe.x64.exe` (16 chars) arrives as `Warframe.x64.ex`. Match the
unambiguous prefix in `matches_warframe()`, not the full string. Same
applies to any process-name match on Linux.

### Build on the oldest glibc you intend to support
glibc has backward-compat but **no** forward-compat. CI uses
`ubuntu-22.04` (glibc 2.35) deliberately. A binary built on modern
Arch / CachyOS will not run on Ubuntu 20.04. Don't bump the runner
without thinking about who that excludes.

### Desktop releases: the version lives in two places
`tennoworth-desktop/Cargo.toml` is authoritative and `rust/Cargo.lock` is
derived. Both must equal the `desktop-v*` tag. Bump Cargo.toml without updating
the lock and the tag's source tarball names the previous version; a
`cargo build --frozen` then refuses to rewrite it. Nothing in the ordinary
build caught that before 0.3.5 and 0.3.6 shipped with stale locks.
`cargo fetch --locked` now gates it in `audit.yml` and again in
`release-desktop.yml` - commit the lock in the same commit as the bump.

### `StartupWMClass` is `tennoworth-desktop`, not the product name
GTK derives WM_CLASS from `g_get_prgname()` (the binary basename) because
Tauri's `enable_gtk_app_id` defaults to false. Verified by running the app:
`WM_CLASS = "tennoworth-desktop", "Tennoworth-desktop"`. A wrong value breaks
taskbar icon binding *silently*. Confirm with `xprop WM_CLASS`, never by
reasoning from the app name.

### `regex` crate feature flags affect binary size *and* pattern syntax
With `default-features = false`, `\d` and `\b` fail to compile (NFA
error). We accept default features - adds ~150 KB but lets us write
normal regexes. Don't disable them in a "minimize binary size" PR
without checking every regex still compiles.

---

## Rust hygiene

- **Panics are a clippy lint, not a script grep.** `[workspace.lints.clippy]`
  (root Cargo.toml) denies the crash family workspace-wide (unwrap/expect/
  panic/unreachable/unimplemented/todo/exit/indexing_slicing/string_slice/
  panic_in_result_fn/unchecked_time_subtraction); clippy.toml re-allows them
  in tests. Sites unreachable by construction carry
  `#[allow(<lint>, reason = "...")]` next to the code - same
  shrink-don't-grow rule the retired check-panic-sites.ts had.
  `cargo clippy --workspace --all-targets` is a required CI check
  (audit.yml cargo-clippy). Tauri caveats: `#[tauri::command]` injects
  `unreachable!()` into async wrappers (spanned at the fn signature) and
  `generate_context!()` expands to `process::exit` - the desktop
  command files and main() allow those file/statement-locally.
- **New dependencies get `cargo info <crate>` before `cargo add`**:
  license, `rust-version` vs the toolchain floor, and the feature list.
  Prefer rustls-based stacks (reqwest/tungstenite already are) so the
  Windows build stays OpenSSL-free. Record the why in the diff.
- **Dead dependencies are `cargo shear`'s job** - the Rust analogue of
  knip for the frontend; follow the applicable checks in ../CONTRIBUTING.md.
- **Dev loop:** use focused crate tests while editing, then the applicable
  native checks in ../CONTRIBUTING.md. Live endpoint tests remain opt-in.
- Atomic writes via `tmp` + `fs::rename`. The Linux semantics give us
  a torn-file-free read on POSIX FS - the same convention the retired
  Python pipeline used (`os.replace`).
- Use `write_restricted()` (0600 from the first syscall - no
  umask race window) on anything containing a secret or
  partial pending-plan state.
- Network calls go through `wfm_client()` so the `user_agent()` +
  timeout policy applies uniformly, and header-building goes through
  `wfm_client::wfm_headers()` / `wfm_authed_headers()` so the
  Crossplay/Platform/Language (+ Cookie/Origin/Referer for authed calls)
  set stays uniform too - don't hand-roll `.header(...)` chains at a new
  call site. Details in [wfm-client/AGENTS.md](wfm-client/AGENTS.md).
- Shared dependency versions (reqwest, serde, serde_json, anyhow,
  base64) live once in the workspace root's `[workspace.dependencies]`.
  A new member crate should inherit via `dep = { workspace = true }`,
  not pin its own version - that's how two of them drifted their
  `reqwest` feature sets before anyone noticed (Cargo was silently
  unifying the build anyway; only the *declaration* had gone stale).
- Cross-compile Linux → Windows works with `mingw-w64-gcc` system
  package + `rustup target add x86_64-pc-windows-gnu`, but CI uses a
  native Windows runner so we don't need to.
