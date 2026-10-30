# rust/ - Rust workspace

## Required design reference

Before changes affecting desktop UI, window layout, or overlay presentation,
read [`../docs/design-system.md`](../docs/design-system.md) completely and
preserve the overlay's transparency, placement and focus on both platforms.
Webview edits also follow the frontend instructions. Verify native behavior in
the native runtime, not from browser tests.

## Crate boundaries

The crate map is [docs/architecture.md](../docs/architecture.md#repository-map);
build and check commands are in ../CONTRIBUTING.md. Crate-specific rules:
[`wfm-scrape/AGENTS.md`](wfm-scrape/AGENTS.md) (DE ingest, host pipeline),
[`wfm-trading/AGENTS.md`](wfm-trading/AGENTS.md) (auth, orders, pending plans),
[`game-acquisition/AGENTS.md`](game-acquisition/AGENTS.md) (memory scan), [`wfm-client/AGENTS.md`](wfm-client/AGENTS.md) (WFM transport).

- `game-acquisition` and `wfm-trading` stay usable without a terminal or
  Tauri: **no interactive terminal I/O** - the passphrase arrives from the
  webview over IPC. `game-acquisition` never talks to warframe.market.
- `market-domain::math` stays pure: no I/O, nothing outside `std`, no clock.
- `wfm-client` shares transport primitives only; authed order mutation and its
  reconciliation stay in `wfm-trading`.
- `wfm-scrape` is the ONLY market pipeline and runs only on the host.
- `market-domain` holds the market heuristics, decision contracts and
  computations, free of Tauri and the network; `market-domain-wasm` exposes it to the browser preview and never
  ships.
- `tennoworth-publisher` has no Tauri dependency and no player account
  operations; the collectors (`tennoworth-collectors`' two binaries) stay
  separate processes with their own databases.

## Cross-crate invariants

These bind every crate here, including callers:

- **Every WFM HTTP attempt, including retries, goes through
  `wfm-client::transport`**, under the policy in
  [../docs/wfm-access.md](../docs/wfm-access.md); the retry layers are listed
  in [wfm-client/AGENTS.md](wfm-client/AGENTS.md). Never blindly retry an
  ambiguous create.
- **The listing caps have one home.** `MAX_PLAN_ITEMS`, `MIN_PLATINUM` and
  `MAX_PLATINUM` are defined once in `market-domain/src/limits.rs` and imported
  by every enforcement point (listing plans and order edits in `wfm-trading`, the
  desktop's `commands/listing.rs`, trade-session selection). Never restate a
  value; the frontend's pre-flight copy is gated by `tests/fixtures/limits.json`.
- **Nullable auxiliary market maps are not a broken snapshot.** Desktop cache
  loading (`tennoworth-desktop/src/services/sellables.rs`) treats explicit
  `null` for optional `catalog`, `path_to_info`, `usage` and `set_to_parts` as
  empty while keeping `items` strict; otherwise one optional surface discards
  current rows and silently falls back to the bundled snapshot. Keep a
  load-level test with a current-only item. Every runtime reader (cache write,
  cache read, webview) accepts exactly the contract in
  `tests/fixtures/market-contract`: an RFC 3339 `updated_at` and non-empty
  `items`. The local clock never rejects a snapshot; it only stops a stamp far
  in the future from blocking a newer one.
- **The app never prints secrets.** `accountId` and `nonce` are session secrets
  while a play session is live; the JWT is a multi-month bearer credential.
  Audit every new log line.
- **A background scan must not run during an interactive listing flow.** A scan
  that changes the inventory records a new snapshot, and
  `services::protection::validate_snapshot` and `Db::session_allowance` require
  a submitted listing's snapshot to be the latest, so a scan landing mid-review
  throws the user's review away. The webview holds `services/auto_scan.rs` while
  `listing.listingOpen` or the Trade Session view is active, and the SPA defers
  adoption. Keep that hold, and route any new background scan path through it.
- **The desktop build embeds `frontend/dist-desktop`.** Run
  `bun run build:desktop` first and Cargo after, never in parallel, or the app
  embeds a stale SPA.
- **The desktop version lives in two places.** `tennoworth-desktop/Cargo.toml`
  is authoritative, `rust/Cargo.lock` is derived, and both must equal the
  `desktop-v*` tag; commit the lock with the bump (`cargo fetch --locked`
  gates it in audit.yml and release-desktop.yml).
- **Build on the oldest glibc you support.** CI builds on `ubuntu-22.04`
  (glibc 2.35) deliberately; a newer runner silently drops older distributions.

## Gotchas

- `setcap` grants are wiped whenever the binary is replaced, so every rebuild
  removes `cap_sys_ptrace`; say so in any run instructions.
- Linux `/proc/<pid>/comm` truncates at 15 chars (`Warframe.x64.ex`); match the
  prefix in `matches_warframe()`.
- On Linux without a Secret Service daemon, "remember on device" falls back to a
  per-launch passphrase and logs `keyring read failed (falling back to
  passphrase)` - the diagnostic, not a bug.
- `regex` with `default-features = false` cannot compile `\d` or `\b`; keep
  default features.

## Rust hygiene

- **Panics are a clippy lint.** `[workspace.lints.clippy]` denies the crash
  family (unwrap/expect/panic/indexing/…); `clippy.toml` re-allows them in
  tests. A site unreachable by construction carries
  `#[allow(<lint>, reason = "...")]` beside it. Tauri's `#[tauri::command]`
  and `generate_context!()` expand to `unreachable!()`/`process::exit`, which
  the command files and `main()` allow locally.
- **Dead code is denied** (`[workspace.lints.rust] dead_code = "deny"`) because
  both platforms compile the desktop crate: a helper with only `#[cfg]`-gated
  callers must carry the cfg itself.
- **New dependencies get `cargo info <crate>` first** (license, `rust-version`,
  features). Prefer rustls stacks so Windows stays OpenSSL-free, and record the
  why in the diff. Shared versions live once in `[workspace.dependencies]`;
  member crates use `{ workspace = true }`. `cargo shear` catches dead ones.
- Atomic writes use `tmp` + `fs::rename`. Anything holding a secret or partial
  pending-plan state uses `write_restricted()` (0600 from the first syscall).
- Use focused crate tests while editing, then the applicable native checks.
  Live endpoint tests stay opt-in.
