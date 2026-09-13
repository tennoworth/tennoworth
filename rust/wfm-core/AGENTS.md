# wfm-core/ - acquisition, auth, orders and recovery

Parent rules: [`../AGENTS.md`](../AGENTS.md). Transport - headers, user agent,
budgets, retry - lives in [`../wfm-client/AGENTS.md`](../wfm-client/AGENTS.md);
this file covers what the core does with it.

## Pacing, caps and pending-plan recovery

Driven by the desktop's IPC commands (`submit_plan` / `get_pending_plan` /
`resume_pending_plan` / …):

- Every WFM HTTP attempt, including retries, goes through
  `wfm-client::transport`. Use the current request-budget and reconciliation
  policy in ../../docs/wfm-access.md; do not add independent sleeps or retry
  loops.
- `MAX_PLAN_ITEMS = 50`, `MIN_PLATINUM = 5`, `MAX_PLATINUM = 3000`
  (the WFM UI cap - maxed arcanes legitimately trade 1500-2500p; an
  earlier 999 cap silently blocked those listings).
  Slug-mismatch guard: refuse listings priced ≥ 3× below the
  reference `low_sell`.

  The edit-order command in `tennoworth-desktop/src/commands/listing.rs`
  enforces the same cap, so these constants have two enforcement points and one
  home. Change them together and cover the pair with a test - see the cross-crate
  invariant in [`../AGENTS.md`](../AGENTS.md).
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

The scanned values are session secrets: see "The app never prints secrets" in
[`../AGENTS.md`](../AGENTS.md).

## WFM API quirks (May 2026, v1 ↔ v2 migration in progress)

Auth: `POST /v1/auth/signin` with `{email, password, auth_type:
"cookie"}`. Grab JWT from `Set-Cookie`. v2 endpoints require this
cookie-style JWT - header-style is rejected. CSRF token:
`GET https://warframe.market/auth/signin`, parse
`<meta name="csrf-token">`, send as `X-CSRFToken` on signin POST.

`api.warframe.market` calls carry `Crossplay` + `Platform` + `Language` through
`wfm_client::wfm_headers()`. Signin is the documented exception and is NOT a
bug - see [`../wfm-client/AGENTS.md`](../wfm-client/AGENTS.md) before "fixing"
the missing header.

User-Agent: always the descriptive project UA, built by
`wfm_client::user_agent(component, version)`. WFM's rules (ToS §11) REQUIRE it
and treat browser spoofing as block-worthy; the old Firefox `BROWSER_UA` is gone
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
in `wfm-core/src/trading/plan.rs`; treat that function as the single
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
