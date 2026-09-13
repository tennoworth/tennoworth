# rust/ - Rust workspace: desktop app + wfm-core + host pipeline

## Required design reference

Before changes affecting desktop UI, window layout, or overlay presentation,
read [`../docs/design-system.md`](../docs/design-system.md) completely. Follow
its shared visual contract and explicit overlay variant; preserve transparency,
placement, focus, and Windows/Linux behavior. Webview edits also require the
frontend instructions. Verify native behavior in the relevant runtime rather
than inferring it from browser tests. Follow the root instruction setup rule
for fresh worktrees; this file is not tracked.

Cargo WORKSPACE with seven members (target/ shared). The standalone player
CLI (`wfm-fetch-inventory` with fetch/login/serve) was removed on 2026-08-02 -
the desktop app is the only adapter:
- `wfm-core/` - the reusable core: process detection + memory scan (with a
  single-flight scan guard), DE inventory fetch, WFM auth + encrypted-JWT
  storage, the listing/order service, pending-plan persistence, and recovery. **No interactive terminal I/O** - the
  desktop shell hands the passphrase in as a parameter over IPC.
- `market-math/` - pure market-data heuristics. No I/O, no deps, no clocks -
  keep it that way. (Ported from the retired wfm_demand.py; its tests were
  1:1 ports of tests/test_wfm_demand.py, which died with Python in 2026-08.)
  The sell-priority scoring mirrors the SPA's canonical `sell-priority.ts`
  and is parity-gated against it on a shared fixture - see the next bullet.
- `wfm-scrape/` - host-only pipeline binary; the ONLY market pipeline (Python
  retired 2026-08). `scrape` runs the full WFM scrape to CSV; `build` renders
  `market.json` + `wfstat-catalog.json`. Fixture regression gates in its
  `tests/` dir shell the freshly-built binary (`env!("CARGO_BIN_EXE_wfm-scrape")`)
  against the frozen fixtures in tests/fixtures/{scrape,convert} - cargo
  rebuilds the binary before they run, so a stale one cannot green them.
  Production runs it via deploy/run-scrape.sh; the box pulls the CI-published
  `scrape-latest` binary, never building Rust itself.
- `wfm-client/` - shared WFM transport primitives: descriptive user agents, headers,
  request budgets, policy enforcement, envelope unwrapping, and retry backoff. Share primitives only - do not grow
  it into an abstraction that swallows authed order mutation.
- `tennoworth-desktop/` - Tauri v2 desktop shell; the app users actually
  install on Linux and Windows. Drives wfm-core over IPC, so it has no HTTP
  server, no session token and no browser Local-Network-Access step. The
  passphrase arrives from the webview - which is why wfm-core must stay free
  of interactive terminal I/O.

- `market-domain/` - native decision contracts, inventory normalization, scoring,
  and planners; shared fixtures and generated frontend declarations.
- `tennoworth-usage/` - opt-in installation-count service.

Use ../CONTRIBUTING.md for builds and required checks, ../docs/architecture.md
for module boundaries, and ../docs/wfm-access.md for WFM request policy.

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

## Digital Extremes ingest (`wfm-scrape/src/de.rs`, `de_extract.rs`)

DE publishes no documented API, no keys and no rate limit. These are the
endpoints the game and the official Companion app run on; a decade of
community use has been tolerated, not licensed. Fetch from the box once per
cycle, with the descriptive UA, and never from a visitor's browser.

- **Every DE URL lives in `de.rs`.** Five of the paths the community wiki
  documents are already dead - including the worldState URL most guides still
  quote. When the next one moves, one file changes.
- **The index is LZMA-*alone*, not xz.** `lzma_rs::lzma_decompress`, not
  `xz_decompress`. And the content hash is **part of the manifest path** -
  the bare filename 404s.
- **Manifests are not single-key.** `ExportWeapons_en.json` carries both
  `ExportWeapons` (837) and `ExportRailjackWeapons` (143). Use
  `manifest_rows_for(doc, basename)`; "the first array" silently reads the
  railjack rows (it did, until the live test caught it).
- **`primeSellingPrice` keys on the RECIPE**, not its `resultType` - Nova
  Prime Blueprint is 45 ducats; the frame it builds has no ducat value.
- **Relic refinement odds are ours, not DE's.** The export ships four variants
  per relic (Bronze/Silver/Gold/Platinum) whose reward lists are identical;
  refinement changes the odds and DE does not publish them. The table lives in
  `de_extract::REFINEMENT_CHANCE` - re-check it on major updates.
- **Never resolve a `/Lotus/...` path by guessing.** `resolve_path` walks the
  `/Lotus/StoreItems` alias and then `ExportRecipes.resultType`, and returns
  `None` otherwise. ~87% of relic reward refs resolve; the rest (Forma, Kuva,
  Exilus adapters) are genuinely untradeable and *should* stay unresolved. A
  wrong price is worse than no price.
- **The live contract test is opt-in**, and is the thing that tells us when an
  endpoint moves:
  `cargo test -p wfm-scrape -- --ignored de_endpoints_are_still_alive`
- **Observation state comes from evidence, not output size.** DE surfaces use
  distinct unavailable, unchanged, invalid, usable, and authoritative-empty
  outcomes. Only a literal, schema-valid empty container may clear prior data;
  a missing key, malformed sibling, truncated join, or failed request carries
  the prior child and its prior data timestamp. Stamp independently fetched
  world-state/riven children independently - one fresh sibling must not make a
  carried sibling look fresh.
- **A shared child timestamp forbids partial truth.** Event Goals are either a
  wholly valid child or invalid/preserved; merging valid rows beside malformed
  siblings and then stamping the map fresh makes carried rows lie about age.
  Reward containers preserve item rewards and credits separately, distinguish
  unsupported-but-dated rewards as `unknown`, accept an explicit supported
  zero (`credits: 0`, empty supported arrays), and reject an unrecognized `{}`.
- **Annual usage has two contracts.** `usage_history` stores compact immutable
  year maps; current `usage` stores the newest rich MR curve used by scoring.
  Validate them separately. A valid compact latest year must still be
  selectively refetched when rich usage is missing, older, has an invalid
  `peak_mr`, or has an empty/non-finite `by_mr`; preserve compact history during
  repair and prove the following warm run skips the fetch. Missing years remain
  absent and retry - never zero-fill them or infer publication from the current
  calendar year.
- **Nullable auxiliary market maps are not a broken snapshot.** Desktop cache
  loading treats explicit `null` for optional `catalog`, `path_to_info`,
  `usage`, and `set_to_parts` as empty/neutral while keeping `items` strict.
  Otherwise one optional surface can discard current market rows and silently
  fall back to the bundled snapshot. Keep a load-level test with a current-only
  item; a serde field test alone cannot prove fallback did not happen.

---

## Pacing, caps and pending-plan recovery

These live in `wfm-core` (shared) and are driven by the desktop's IPC commands
(`submit_plan` / `get_pending_plan` / `resume_pending_plan` / …):

- Every WFM HTTP attempt, including retries, goes through
  `wfm-client::transport`. Use the current request-budget and reconciliation
  policy in ../docs/wfm-access.md; do not add independent sleeps or retry loops.
- `MAX_PLAN_ITEMS = 50`, `MIN_PLATINUM = 5`, `MAX_PLATINUM = 3000`
  (the WFM UI cap - maxed arcanes legitimately trade 1500-2500p; an
  earlier 999 cap silently blocked those listings). The edit-order
  command enforces the same cap.
  Slug-mismatch guard: refuse listings priced ≥ 3× below the
  reference `low_sell`.
- Pending-plan recovery: every plan is persisted to
  `~/.config/wfminv/pending_plan.json` (atomic tmp+rename) before the
  first POST, updated after each item, and deleted on clean
  completion. `get_pending_plan` / `resume_pending_plan` /
  `discard_pending_plan` expose this to the webview.

## Cross-platform memory access

`scan_session(pid)` is implemented twice, gated by
`#[cfg(target_os = …)]`:

- **Linux**: parse `/proc/<pid>/maps` → seek+read `/proc/<pid>/mem` in
  chunks with a small overlap so cross-chunk pattern matches don't escape.
  AppImage runs cannot retain file capabilities on their temporary nosuid
  mount, so `ptrace_open_error()` directs them to adjust Yama's ptrace scope.
  A local binary can instead use the tighter one-time
  `setcap cap_sys_ptrace=eip` grant. Scope 3 disables ptrace even for a
  capable binary.
- **Windows**: `VirtualQueryEx` to walk regions, `ReadProcessMemory`
  to read, filtering on `MEM_COMMIT` and excluding `PAGE_NOACCESS` /
  `PAGE_GUARD`. No elevation needed if running as the user that
  launched WF.

Patterns scanned (`regex::bytes::Regex`):
- `accountId=([0-9a-fA-F]{24})&nonce=([0-9]{6,})` - session creds
- `"BuildLabel":"([0-9.]+)/[A-Za-z0-9]+` - game build → appVersion
- `&ct=([A-Z]{2,4})\b` - platform tag

---

## WFM API quirks (May 2026, v1 ↔ v2 migration in progress)

Auth: `POST /v1/auth/signin` with `{email, password, auth_type:
"cookie"}`. Grab JWT from `Set-Cookie`. v2 endpoints require this
cookie-style JWT - header-style is rejected. CSRF token:
`GET https://warframe.market/auth/signin`, parse
`<meta name="csrf-token">`, send as `X-CSRFToken` on signin POST.

Every **api.warframe.market** call needs `Crossplay: true` + `Platform: pc` +
`Language: en` - that's what `wfm_client::wfm_headers()` sends. Signin is the
exception and is NOT a bug: it POSTs to `warframe.market/v1/auth/signin` with
`Platform` + `Language` + `auth_type` + `X-CSRFToken` and no `Crossplay`, which
is what works against the live endpoint. Don't "fix" the omission by adding the
header - it's an auth-host request, not an API call, and it is verified working
as written.
User-Agent: always the descriptive project UA - `wfm_core::user_agent()` in the app,
`wfm_client::user_agent(component, version)` elsewhere. WFM's rules (ToS §11) REQUIRE
it and treat browser spoofing as block-worthy; the old Firefox `BROWSER_UA` is gone
(2026-08-16 - probed: descriptive UA is accepted on every v1/v2 endpoint).

| Action | Method + path | Body / notes |
|---|---|---|
| Sign in | `POST /v1/auth/signin` | `{email, password, auth_type: "cookie"}` |
| Item catalog | `GET /v2/items` | flat `data: [{id, slug, i18n.en.name, …}]` |
| Current user | `GET /v2/me` | needs JWT cookie; `data.slug` = username |
| Create listing | `POST /v2/order` | see body schema below |
| Update listing | `PATCH /v2/order/<id>` | any subset of `{platinum, quantity, visible, rank}` |
| Delete listing | `DELETE /v2/order/<id>` | - |
| List my orders | `GET /v2/orders/user/<username>` | response carries `itemId` only - we enrich with `item.name` via the catalog |

If `/v2/orders/user/<username>` starts returning `item` metadata on
its own, `attach_item_name()` already no-clobbers - but check for
shape drift in the agent that watches WFM endpoints.

### `POST /v2/order` body schema (verified May 2026)

Every 400 response of the form `{"inputs":{"<field>":"<rule>"}}` we've
hit is captured here. The body assembly lives in `build_order_body()`
in `rust/wfm-core/src/trading/plan.rs`; treat that function as the single
source of truth and these notes as the *why*.

| Field | Rule | Notes |
|---|---|---|
| `itemId` | required | NOT `item`. From `/v2/items[].id`. |
| `type` | required, `"sell"` / `"buy"` | NOT `order_type`. |
| `platinum` | required, > 0 | We cap 5 ≤ p ≤ 3000 client-side. |
| `quantity` | required, > 0 | The stack size you're listing. |
| `visible` | required, bool | We default to `false` and let the user toggle later. |
| `perTrade` | required | Must divide `quantity` EVENLY and be ≤ 6 (in-game trade slots). Use `per_trade_for(quantity)` - largest divisor of quantity that's ≤ 6. qty=27 → 3, qty=10 → 5, qty=7 → 1. Rejected with `app.field.tooBig` if > 6; `app.field.orders.perTradeMustDivideQuantity` if not a divisor. |
| `rank` | conditional | **Required for items with `maxRank` in the catalog** (mods, arcanes); **`app.field.notAllowed` for items without it** (relics, sets, parts). Default 0. |
| `subtype` | conditional | **Required for items with `subtypes[]` in the catalog** (relics: `intact/exceptional/flawless/radiant`; veiled rivens: `unrevealed/revealed`). `app.field.required` if missing. Default to the first entry - that's the lowest-value variant. |

When the WFM frontend evolves, add a column here and update
`build_order_body()` + its tests in one go. Don't paper over a new 400
in calling code.

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
  call site.
- Mutation reconciliation stays in trading services. Never blindly retry an
  ambiguous create; preserve pending-plan recovery and classify outcomes using
  the policy in ../docs/wfm-access.md.
- Shared dependency versions (reqwest, serde, serde_json, anyhow,
  base64) live once in the workspace root's `[workspace.dependencies]`.
  A new member crate should inherit via `dep = { workspace = true }`,
  not pin its own version - that's how two of them drifted their
  `reqwest` feature sets before anyone noticed (Cargo was silently
  unifying the build anyway; only the *declaration* had gone stale).
- Cross-compile Linux → Windows works with `mingw-w64-gcc` system
  package + `rustup target add x86_64-pc-windows-gnu`, but CI uses a
  native Windows runner so we don't need to.
