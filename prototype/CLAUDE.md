# prototype/ — Svelte 5 + Vite web app

Static-deployable informational site. No backend, no accounts, no file
input — it renders the market snapshot and the desktop-app showcase. The
desktop app scans the game and holds your inventory; the site never
receives it.

Dev: `bun run dev` (http://127.0.0.1:5173). Tests: `bun run test`.
Build: `bun run build`. Type-check: `bun run check`. Install: `bun install`.

`npm` works too — `package.json` scripts are runtime-agnostic — but
`bun.lock` is the source of truth lockfile and CI uses `bun install
--frozen-lockfile`.

---

## Architectural rules

### No third-party fetches from the browser. None.
WFM serves no `Access-Control-Allow-Origin` header. Direct browser
fetches will fail CORS. All WFM data must come from the static
`market.json` snapshot under `public/`, produced by the box's Rust
`wfm-scrape`; the GH Actions cron's Python run refreshes the repo copy.

`warframestat.us` used to be the one allowed direct call (it sent
CORS headers; the resolver hit `/items/` for `/Lotus/...` paths) —
**upstream dropped its CORS headers on 2026-06-09** and broke every
inventory upload. The resolver catalog is now baked too:
`csv_to_market_json.py` writes `public/wfstat-catalog.json` (slim
`[uniqueName, {name, category}]` pairs, forced `Accept-Language: en` —
localized names silently fail the WFM name join). All vendor data
(relic rewards, vault status, **Baro schedule** — `market.baro`,
resolver catalog) is baked at build time and served same-origin; the
CSP `connect-src` has no third-party origins left. A runtime
warframestat fetch broke this rule once before and vanished during
outages — don't reintroduce one.

The **desktop** build keeps its bundled `market.json` fresh from
`https://tennoworth.app/market.json`, but that egress lives in **Rust**
(the `refresh_market` / `cached_market` Tauri commands in
`tennoworth-desktop`), never the webview — same rule. The SPA touches it
only through the transport seam (`transport.refreshMarket()` /
`loadCachedMarket()`), a no-op on the hosted site. Boot order: load
the cached copy (fresher than the compile-time bundle) else the bundle,
then refresh in the background and swap in a strictly-newer snapshot.
Never add a webview `fetch('https://tennoworth.app/…')`.

Interactive features — scanning, order management, login — are **desktop
only**. The hosted site is informational: `createTransport()` returns
`HostedTransport` there, which no-ops the market-cache calls and throws on
every interactive op. The webview never sees the JWT; it stays in the Rust
process.

### One source of truth for owned-item resolution
`src/lib/resolver.ts` is the only place that maps a `/Lotus/...` path
to a `{name, slug, category}`, reading the baked
`/wfstat-catalog.json`. All UI code joins through
`market.items[slug]` (stats) and `market.catalog[name_lower]` (slug
lookup).

---

## Svelte 5 rules — the non-obvious ones we've already hit

### `$effect` cannot read and write the same state
`$effect` tracks every reactive read inside its body as a dependency.
If the effect then writes to that state, the write re-triggers the
effect → infinite loop. We hit this concretely (Maximum update depth
exceeded) when an init effect wrote `resolved` and then called
`recomputeResults()` which read `resolved.owned`.

**Rule:** for one-time initialization, use `onMount` (no reactivity
tracking). Use `$effect` only when you genuinely want re-runs on
state change — and even then, never write to anything the effect
reads.

### `$derived` must be pure
No side effects, no writes, no mutations. If it computes a value, it
goes in `$derived`. If it performs an action (including saving to
localStorage), it goes in an event handler or `$effect`.

### Don't destructure `$state` objects
Destructuring takes a snapshot — you get plain values, not reactive
bindings. Always access through the original (`user.name`), never
`const { name } = user`.

### Use event attributes, not directives
`onclick={fn}`, not `on:click={fn}`. The directive form is deprecated
in Svelte 5 and will warn during build.

### Pass callbacks as props, not via `createEventDispatcher`
Child components take callbacks (`onunlocked={fn}`, `onimport={fn}`) as
`$props()` props, not `dispatch(...)`.

---

## Browser storage — when to use which

| Need | Pick |
|---|---|
| Small key/value, sync, ≤5 MB | `localStorage` |
| Multi-MB structured cache (item catalogs) | IndexedDB |
| HTTP request/response cache | Cache API (not used yet) |

`localStorage` keys we use:
- `wfminv:last-owned-v5` — saved owned-items snapshot.
- `wfminv:reserve-copies-v1` — reserve/keep-copies count.
- `wfminv:filters-open-v1` — filter panel expanded/collapsed.
- `wfminv:view-v1` — selected view/preset.
- `wfminv:score-explainer-dismissed-v1` — score explainer dismissed flag.

**Don't read/write these via raw `localStorage` calls.** Go through
`src/lib/state-store.ts`'s `store.getSetting`/`setSetting` (`SettingKey`
union) — it's the one seam that also backs the desktop build (Tauri/SQLite)
with the same calls; a raw `localStorage.setItem` silently no-ops there.
New setting → extend `SettingKey`, `LOCAL_SETTING_KEYS`, and the desktop
`hydrate()` key list together.

IndexedDB DB:
- `wfminv` / store `catalogs` / key `wfstat-items-v3` — slim
  `[uniqueName, {name, category}]` pairs from the baked
  `/wfstat-catalog.json` (v2 caches could hold localized names).

**Always bump the version suffix in the key when the stored shape
changes** so old data is silently invalidated. **And add the outgoing key to
`RETIRED_KEYS` in `catalog-cache.ts` in the same edit** — bumping invalidates
the old row but cannot delete it, because nothing reads a key it no longer
knows. The v2→v3 bump shipped without this and left a dead multi-hundred-KB
row in every existing user's IndexedDB, reachable by no code path until
`purgeRetiredCaches()` was added. Invalidation is not reclamation.

---

## Desktop transport (Tauri IPC)

`src/lib/transport.ts` is the app's seam to wfm-core. `createTransport()`
returns `TauriTransport` (invoke into wfm-core over Tauri IPC) on the
desktop and `HostedTransport` on the informational site. The hosted site has
NO interactive ops — `HostedTransport` no-ops the two market-cache calls and
throws on everything else, because the site is informational only. App-level
ops go through the transport:

- `health` — `{ok, platform}`.
- `scan_inventory` — memory-scans the running game and returns inventory
  JSON directly. Desktop only. 503-equivalent rejection when the game isn't
  scannable.
- `submit_plan` / `get_pending_plan` / `resume_pending_plan` /
  `discard_pending_plan` — listing batch + interrupted-batch recovery.
- `fetch_orders` / `update_order` / `delete_order` / `bulk_visibility` —
  the My orders panel.
- `wfm_auth_status` / `wfm_login` / `unlock_jwt` / `try_silent_unlock` —
  desktop login + unlock (WfmAuthDialogs.svelte).

Listing/order commands reject with a typed `{code, message}` CmdError that
surfaces as `DesktopCmdError` — `needs_login` / `needs_unlock` drive the
SPA's login and passphrase dialogs.

---

## Crypto

Encrypted export (`src/lib/crypto.ts`):
- **PBKDF2-HMAC-SHA256**, 600,000 iterations (OWASP 2023).
- **AES-256-GCM**, fresh 12-byte IV + 16-byte salt per export.
- Native WebCrypto. No third-party crypto libraries.

Same parameters mirror the desktop app's `wfminv-jwt-v1` on-disk format
so a single human can reason about both.

---

## CSP & headers

Production serves through **Caddy on the self-host box**, which applies
the full header set (HSTS, `frame-ancestors` / X-Frame-Options, the
CSP) from `deploy/Caddyfile` — kept in sync with the other CSP copies
by `scripts/sync-csp.mjs`. The `<meta http-equiv="Content-Security-Policy">`
in `index.html` still ships script/connect/style protection as a
belt-and-suspenders fallback. The `public/_headers` file only matters
for preview deployments on Cloudflare Pages / Netlify / Vercel (GitHub
Pages silently drops it), where the header host isn't ours.

Allowed `connect-src` (hosted): `self`. The hosted site makes no
loopback or third-party calls — the loopback entries were for the removed
companion CLI. The CSP ships in three places
(`index.html` meta, `public/_headers`, `deploy/Caddyfile`) but is
**edited in ONE**: `scripts/sync-csp.mjs`. Change the directives there,
run `bun run csp` to rewrite all three; `bun run build` fails via its
prebuild `--check` if any copy drifted. (The meta copy deliberately
omits `frame-ancestors` — browsers ignore it in meta tags.)

**Desktop (Tauri) is a build-variant, not a fourth committed copy.**
`bun run build:desktop` builds to `dist-desktop/` (gitignored) and runs
`sync-csp.mjs --desktop dist-desktop/index.html`, which rewrites only that
built file's meta CSP to
`connect-src 'self' ipc://localhost http://ipc.localhost https://tennoworth.app`
(the Tauri IPC scheme added so `invoke` uses the fast path with no CSP
violations; plus the one C4 refresh origin). It NEVER touches the three hosted
copies, so the hosted CSP stays byte-identical.
`companion/tennoworth-desktop`'s `frontendDist` points at `dist-desktop`.

---

## Hygiene

- **No comments that restate the code.** Comments explain *why*.
- **No backwards-compat shims** for code that hasn't shipped yet.
  Renaming a state field? Bump the storage key version and move on.
- **Edit existing files** in preference to creating new ones.
- **Match the scope of the request.** Bug fix ≠ refactor pass.
- **Verify in the browser, and A/B the fix.** For UI changes drive
  Playwright or the dev server. "Tests pass" ≠ "feature works" — and a
  passing test after a fix doesn't prove the bug was real. Revert the fix,
  re-run the same script, watch it fail, restore. That is what turned R1
  (the review modal resetting in-flight edits) from a plausible reading of
  the code into a demonstrated defect.
  Gotchas that cost time here: the Playwright MCP wants Chrome specifically
  but `~/.cache/ms-playwright/chromium-*/chrome-linux64/chrome` works when
  passed as `executablePath`; the app stays on the onboarding view until an
  inventory is loaded, so anything gated on a desktop session (the bulk
  List CTA, the orders panel) is invisible before that; the review modal is a
  `div[role="dialog"]`, not a `<dialog>`; and `.playwright-mcp/inventory-test.json`
  is a real 2 MB inventory to drive it with.
