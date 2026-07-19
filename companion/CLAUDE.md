# companion/ — Rust workspace: CLI + loopback HTTP server

Cargo WORKSPACE with four members (target/ shared, so the binary path in
every doc stays `companion/target/release/wfm-fetch-inventory`):
- `wfm-fetch-inventory/` — the player-facing binary, cross-platform
  (Linux + Windows), ~3 MB. The ONLY release-gated crate.
- `market-math/` — pure market-data heuristics ported from wfm_demand.py.
  No I/O, no deps, no clocks — keep it that way; its tests are 1:1 ports of
  tests/test_wfm_demand.py. When you change a heuristic, change BOTH
  implementations (Python is still the production scraper) and both test
  suites, until the cutover.
- `wfm-scrape/` — host-only pipeline binary. `build` mirrors
  scripts/csv_to_market_json.py and is gated by the CI parity test
  (tests/test_convert_parity.py — semantic JSON vs the Python converter on
  frozen fixtures); `scrape` (the wfm_demand.py port) is not implemented yet.
  NOT production; Python remains the pipeline writer until cutover.
- `wfm-client/` — shared WFM transport primitives (UA, headers, envelope
  unwrap, rate limiter, retries). Share primitives only — do not grow it
  into an abstraction that swallows authed order mutation.

The binary has three subcommands in one tree:
- `fetch` — extracts `inventory.json` from the running game process.
- `login` — interactive WFM signin; encrypts JWT at rest.
- `serve` — loopback HTTP server the web app talks to for bulk
  listings + order management.

Build: `cargo build --release`. Tests: `cargo test`.

---

## Hard invariants — break these and we ship a regression

### Companion never prints secrets
`accountId` and `nonce` are session secrets while a play session is
live. The JWT is a multi-month bearer credential. Keep them out of
stdout/stderr at all costs. `wfm-fetch-inventory` deliberately omits
them; `serve` only ever logs `→ Decrypted JWT (N chars, platform=…)`.
If you add a new subcommand or log line, audit it.

### Companion chowns output back when run as root
If invoked via `sudo`, the resulting `inventory.json` and any other
file written under `~` would be root-owned and unreadable to the
user's file manager. We resolve `$SUDO_USER`'s home and chown the file
back. `chown_to_real_user()` is the helper — call it after every
file write.

### `setcap` is wiped on file replacement
Linux clears file capabilities whenever the binary is replaced. Every
`cargo build --release` therefore wipes `cap_sys_ptrace`. Document
this in any "how to run the companion" instructions you write.

### Linux `/proc/<pid>/comm` truncates at 15 chars
`Warframe.x64.exe` (16 chars) arrives as `Warframe.x64.ex`. Match the
unambiguous prefix in `matches_warframe()`, not the full string. Same
applies to any process-name match on Linux.

### Build on the oldest glibc you intend to support
glibc has backward-compat but **no** forward-compat. CI uses
`ubuntu-22.04` (glibc 2.35) deliberately. A binary built on modern
Arch / CachyOS will not run on Ubuntu 20.04. Don't bump the runner
without thinking about who that excludes.

### `regex` crate feature flags affect binary size *and* pattern syntax
With `default-features = false`, `\d` and `\b` fail to compile (NFA
error). We accept default features — adds ~150 KB but lets us write
normal regexes. Don't disable them in a "minimize binary size" PR
without checking every regex still compiles.

---

## Loopback server (`serve` subcommand)

- `tiny_http` blocking server bound to `127.0.0.1:0` (random port).
- Per-process random session token in `X-Session-Token`. CORS is `*`
  because origin isn't the protection — the token is.
- One thread per request. Plan execution paced to 3 req/sec to match
  WFM's norm (see `SERVE_RATE_LIMIT_MS`).
- `MAX_PLAN_ITEMS = 50`, `MIN_PLATINUM = 5`, `MAX_PLATINUM = 3000`
  (the WFM UI cap — maxed arcanes legitimately trade 1500-2500p; an
  earlier 999 cap silently blocked those listings). The PATCH
  (edit-order) route enforces the same cap.
  Slug-mismatch guard: refuse listings priced ≥ 3× below the
  reference `low_sell`.
- `CORS preflight` advertises `GET, POST, PATCH, DELETE, OPTIONS`. If
  you add a new HTTP method to any route, add it here too — browsers
  block the preflight otherwise.
- Pending-plan recovery: every plan is persisted to
  `~/.config/wfminv/pending_plan.json` (atomic tmp+rename) before the
  first POST, updated after each item, and deleted on clean
  completion. `/plan/pending`, `/plan/resume`, `DELETE /plan/pending`
  expose this to the browser.

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

Every API call needs `Crossplay: true` + `Platform: pc` + `Language: en`.
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
in `companion/src/main.rs`; treat that function as the single source
of truth and these notes as the *why*.

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
  timeout policy applies uniformly.
- Cross-compile Linux → Windows works with `mingw-w64-gcc` system
  package + `rustup target add x86_64-pc-windows-gnu`, but CI uses a
  native Windows runner so we don't need to.
