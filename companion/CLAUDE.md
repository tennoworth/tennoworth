# companion/ — Rust workspace: desktop app + wfm-core + host pipeline

Cargo WORKSPACE with five members (target/ shared). The standalone player
CLI (`wfm-fetch-inventory` with fetch/login/serve) was removed on 2026-08-02 —
the desktop app is the only adapter:
- `wfm-core/` — the reusable core: process detection + memory scan (with a
  single-flight scan guard), DE inventory fetch, WFM auth + encrypted-JWT
  storage, the listing/order service, pending-plan persistence, and the
  dormant DeepSeek assistant relay. **No interactive terminal I/O** — the
  desktop shell hands the passphrase in as a parameter over IPC.
- `market-math/` — pure market-data heuristics ported from wfm_demand.py.
  No I/O, no deps, no clocks — keep it that way; its tests are 1:1 ports of
  tests/test_wfm_demand.py. When you change a heuristic, change BOTH
  implementations (Python remains the parity reference) and both test
  suites.
- `wfm-scrape/` — host-only pipeline binary. Both subcommands are
  implemented and parity-gated against their Python originals on frozen
  fixtures: `build` mirrors scripts/csv_to_market_json.py
  (tests/test_convert_parity.py, semantic JSON diff) and `scrape` mirrors
  wfm_demand.py (tests/test_scrape_parity.py, semantic CSV diff). Both gates
  rebuild the binary first — a stale one used to pass them silently.
  The Rust scrape is the default in deploy/run-scrape.sh once the box pulls
  the current checkout (WFM_SCRAPER default `auto` prefers it); keep both
  implementations in sync until the rollback path is confirmed dead.
  "Not implemented" is what this said until 2026-08-01, long after the port
  landed.
- `wfm-client/` — shared WFM transport primitives: the browser UA (the one
  definition; wfm-core re-exports it), the Cloudflare-appeasing header set,
  envelope unwrapping, and retry backoff. Share primitives only — do not grow
  it into an abstraction that swallows authed order mutation.
- `tennoworth-desktop/` — Tauri v2 desktop shell; the app users actually
  install on Linux and Windows. Drives wfm-core over IPC, so it has no HTTP
  server, no session token and no browser Local-Network-Access step. The
  passphrase arrives from the webview — which is why wfm-core must stay free
  of interactive terminal I/O.

Build: `cargo build --release`. Tests: `cargo test`.

---

## Hard invariants — break these and we ship a regression

### The app never prints secrets
`accountId` and `nonce` are session secrets while a play session is
live. The JWT is a multi-month bearer credential. Keep them out of
stdout/stderr at all costs. If you add a new log line, audit it.

### `setcap` is wiped on file replacement
Linux clears file capabilities whenever the binary is replaced. Every
`cargo build --release` therefore wipes `cap_sys_ptrace`. Document
this in any "how to run the app" instructions you write.

### Linux `/proc/<pid>/comm` truncates at 15 chars
`Warframe.x64.exe` (16 chars) arrives as `Warframe.x64.ex`. Match the
unambiguous prefix in `matches_warframe()`, not the full string. Same
applies to any process-name match on Linux.

### Build on the oldest glibc you intend to support
glibc has backward-compat but **no** forward-compat. CI uses
`ubuntu-22.04` (glibc 2.35) deliberately. A binary built on modern
Arch / CachyOS will not run on Ubuntu 20.04. Don't bump the runner
without thinking about who that excludes.

### Desktop releases: the version lives in FOUR places
`tauri.conf.json`, `tennoworth-desktop/Cargo.toml`, and **both**
`packaging/aur/*/PKGBUILD` pkgvers must equal the `desktop-v*` tag. CI's guard
only compares the first two; a stale PKGBUILD pkgver just 404s on a tag that
doesn't exist.

### Tauri deb/rpm facts, each of which cost a debugging cycle
- **reprepro REJECTS a deb with no `Section:`** ("No section given"). Set
  `bundle.linux.deb.section`. Without it CI stays green and the repo silently
  publishes nothing — the worst possible failure shape.
- Package name is **`tenno-worth`**: Tauri kebab-cases `productName` and the
  schema has no override. Both formats declare `Provides: tennoworth`, and
  `apt install tennoworth` / `dnf install tennoworth` were verified to resolve
  through it. Don't "fix" this by renaming productName.
- Tauri **auto-adds** the webkit/gtk/appindicator deps; declaring them again
  duplicates them in `Depends:`. Only declare `libcap2-bin` / `libcap`.
- `postInstallScript` takes a path **relative to tauri.conf.json** (the docs
  show absolute). The v2 bundlers are native Rust — no `dpkg-deb`/`rpmbuild`.

### `StartupWMClass` is `tennoworth-desktop`, not the product name
GTK derives WM_CLASS from `g_get_prgname()` (the binary basename) because
Tauri's `enable_gtk_app_id` defaults to false. Verified by running the app:
`WM_CLASS = "tennoworth-desktop", "Tennoworth-desktop"`. A wrong value breaks
taskbar icon binding *silently* — it shipped broken in the AUR package for
weeks. Confirm with `xprop WM_CLASS`, never by reasoning from the app name.

### `regex` crate feature flags affect binary size *and* pattern syntax
With `default-features = false`, `\d` and `\b` fail to compile (NFA
error). We accept default features — adds ~150 KB but lets us write
normal regexes. Don't disable them in a "minimize binary size" PR
without checking every regex still compiles.

---

## Pacing, caps and pending-plan recovery

These live in `wfm-core` (shared) and are driven by the desktop's IPC commands
(`submit_plan` / `get_pending_plan` / `resume_pending_plan` / …):

- Plan execution paced to ~3 req/sec to match WFM's norm
  (`SERVE_RATE_LIMIT_MS = 350` in `wfm-core/src/listing.rs`).
- `MAX_PLAN_ITEMS = 50`, `MIN_PLATINUM = 5`, `MAX_PLATINUM = 3000`
  (the WFM UI cap — maxed arcanes legitimately trade 1500-2500p; an
  earlier 999 cap silently blocked those listings). The edit-order
  command enforces the same cap.
  Slug-mismatch guard: refuse listings priced ≥ 3× below the
  reference `low_sell`.
- Pending-plan recovery: every plan is persisted to
  `~/.config/wfminv/pending_plan.json` (atomic tmp+rename) before the
  first POST, updated after each item, and deleted on clean
  completion. `get_pending_plan` / `resume_pending_plan` /
  `discard_pending_plan` expose this to the webview.

### The assistant relay is dormant

The DeepSeek advisor (`wfm_core::assistant`) still has a registered
`ask_assistant` Tauri command, but **no UI surfaces it** — the chat drawer was
removed with the CLI. Nothing calls the command from the SPA, so no data goes
to DeepSeek. Its caps (`question ≤ 2000 chars`, `context ≤ 100 KB`, `history ≤
12 turns`, throttle ≤ 20 calls / 60 s) and prompt-injection defenses remain in
wfm-core for when a desktop assistant UI ships. Before re-enabling, update
SECURITY.md's assistant section — third-party egress is a deliberate, audited
list.

## Cross-platform memory access

`scan_session(pid)` is implemented twice, gated by
`#[cfg(target_os = …)]`:

- **Linux**: parse `/proc/<pid>/maps` → seek+read `/proc/<pid>/mem` in
  chunks with a small overlap so cross-chunk pattern matches don't
  escape. Needs `CAP_SYS_PTRACE` — prefer the one-time
  `setcap cap_sys_ptrace=eip` (no sudo per run) over `sudo`. On a
  PermissionDenied open, `ptrace_open_error()` prints the setcap path
  and flags `ptrace_scope=3` (which disables ptrace even for a capable
  binary).
- **Windows**: `VirtualQueryEx` to walk regions, `ReadProcessMemory`
  to read, filtering on `MEM_COMMIT` and excluding `PAGE_NOACCESS` /
  `PAGE_GUARD`. No elevation needed if running as the user that
  launched WF.

Patterns scanned (`regex::bytes::Regex`):
- `accountId=([0-9a-fA-F]{24})&nonce=([0-9]{6,})` — session creds
- `"BuildLabel":"([0-9.]+)/[A-Za-z0-9]+` — game build → appVersion
- `&ct=([A-Z]{2,4})\b` — platform tag

---

## WFM API quirks (May 2026, v1 ↔ v2 migration in progress)

Auth: `POST /v1/auth/signin` with `{email, password, auth_type:
"cookie"}`. Grab JWT from `Set-Cookie`. v2 endpoints require this
cookie-style JWT — header-style is rejected. CSRF token:
`GET https://warframe.market/auth/signin`, parse
`<meta name="csrf-token">`, send as `X-CSRFToken` on signin POST.

Every **api.warframe.market** call needs `Crossplay: true` + `Platform: pc` +
`Language: en` — that's what `wfm_client::wfm_headers()` sends. Signin is the
exception and is NOT a bug: it POSTs to `warframe.market/v1/auth/signin` with
`Platform` + `Language` + `auth_type` + `X-CSRFToken` and no `Crossplay`, which
is what works against the live endpoint. Don't "fix" the omission by adding the
header — it's an auth-host request, not an API call, and it is verified working
as written.
Cloudflare blocks generic UAs (error 1015) — always use `BROWSER_UA`.

| Action | Method + path | Body / notes |
|---|---|---|
| Sign in | `POST /v1/auth/signin` | `{email, password, auth_type: "cookie"}` |
| Item catalog | `GET /v2/items` | flat `data: [{id, slug, i18n.en.name, …}]` |
| Current user | `GET /v2/me` | needs JWT cookie; `data.slug` = username |
| Create listing | `POST /v2/order` | see body schema below |
| Update listing | `PATCH /v2/order/<id>` | any subset of `{platinum, quantity, visible, rank}` |
| Delete listing | `DELETE /v2/order/<id>` | — |
| List my orders | `GET /v2/orders/user/<username>` | response carries `itemId` only — we enrich with `item.name` via the catalog |

If `/v2/orders/user/<username>` starts returning `item` metadata on
its own, `attach_item_name()` already no-clobbers — but check for
shape drift in the agent that watches WFM endpoints.

### `POST /v2/order` body schema (verified May 2026)

Every 400 response of the form `{"inputs":{"<field>":"<rule>"}}` we've
hit is captured here. The body assembly lives in `build_order_body()`
in `companion/wfm-core/src/plan.rs`; treat that function as the single
source of truth and these notes as the *why*.

| Field | Rule | Notes |
|---|---|---|
| `itemId` | required | NOT `item`. From `/v2/items[].id`. |
| `type` | required, `"sell"` / `"buy"` | NOT `order_type`. |
| `platinum` | required, > 0 | We cap 5 ≤ p ≤ 3000 client-side. |
| `quantity` | required, > 0 | The stack size you're listing. |
| `visible` | required, bool | We default to `false` and let the user toggle later. |
| `perTrade` | required | Must divide `quantity` EVENLY and be ≤ 6 (in-game trade slots). Use `per_trade_for(quantity)` — largest divisor of quantity that's ≤ 6. qty=27 → 3, qty=10 → 5, qty=7 → 1. Rejected with `app.field.tooBig` if > 6; `app.field.orders.perTradeMustDivideQuantity` if not a divisor. |
| `rank` | conditional | **Required for items with `maxRank` in the catalog** (mods, arcanes); **`app.field.notAllowed` for items without it** (relics, sets, parts). Default 0. |
| `subtype` | conditional | **Required for items with `subtypes[]` in the catalog** (relics: `intact/exceptional/flawless/radiant`; veiled rivens: `unrevealed/revealed`). `app.field.required` if missing. Default to the first entry — that's the lowest-value variant. |

When the WFM frontend evolves, add a column here and update
`build_order_body()` + its tests in one go. Don't paper over a new 400
in calling code.

---

## Rust hygiene

- Atomic writes via `tmp` + `fs::rename`. The Linux semantics give us
  a torn-file-free read on POSIX FS — match the same convention used
  in `wfm_demand.py` (`os.replace`).
- Use `write_restricted()` (0600 from the first syscall — no
  umask race window) on anything containing a secret or
  partial pending-plan state.
- Network calls go through `wfm_client()` so the `BROWSER_UA` +
  timeout policy applies uniformly, and header-building goes through
  `wfm_client::wfm_headers()` / `wfm_authed_headers()` so the
  Crossplay/Platform/Language (+ Cookie/Origin/Referer for authed calls)
  set stays uniform too — don't hand-roll `.header(...)` chains at a new
  call site.
- Order-mutation calls (create/update/delete) go through
  `listing.rs`'s `send_with_retry()` — a short retry on transport
  errors and 5xx only, never 4xx (those are semantic and retrying won't
  help). Mirror this for any new order-mutation call site; a one-shot
  `.send()` here means a single dropped packet mid-batch permanently
  fails that item.
- Shared dependency versions (reqwest, serde, serde_json, anyhow,
  base64) live once in the workspace root's `[workspace.dependencies]`.
  A new member crate should inherit via `dep = { workspace = true }`,
  not pin its own version — that's how two of them drifted their
  `reqwest` feature sets before anyone noticed (Cargo was silently
  unifying the build anyway; only the *declaration* had gone stale).
- Cross-compile Linux → Windows works with `mingw-w64-gcc` system
  package + `rustup target add x86_64-pc-windows-gnu`, but CI uses a
  native Windows runner so we don't need to.
