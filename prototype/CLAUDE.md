# prototype/ — Svelte 5 + Vite web app

Static-deployable browser app. No backend. Drop `inventory.json` →
join against `public/market.json` → ranked table of what to sell.

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
`market.json` snapshot under `public/`, produced by `wfm_demand.py`
(locally) or the GH Actions cron (in production).

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

For order management (create / edit / delete listings), the browser
talks to the **companion's loopback HTTP server** on `127.0.0.1`,
which has the JWT in memory and relays. The browser never sees the
JWT.

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
Child components take `oninventory={fn}` as a `$props()` callback,
not `dispatch('inventory', detail)`.

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
- `wfminv:companion-v1` — companion URL + session token.
- `wfminv:filters-open-v1` — filter panel expanded/collapsed.
- `wfminv:view-v1` — selected view/preset.
- `wfminv:score-explainer-dismissed-v1` — score explainer dismissed flag.

IndexedDB DB:
- `wfminv` / store `catalogs` / key `wfstat-items-v3` — slim
  `[uniqueName, {name, category}]` pairs from the baked
  `/wfstat-catalog.json` (v2 caches could hold localized names).

**Always bump the version suffix in the key when the stored shape
changes** so old data is silently invalidated.

---

## Companion HTTP contract

`src/lib/transport.ts` is the dual-mode seam: it picks `HttpCompanionTransport`
(wraps `companion.ts` + `assistant.ts` 1:1 — the routes below) or
`TauriTransport` (`invoke` into wfm-core) once at boot by sniffing
`window.__TAURI_INTERNALS__`. App-level ops go through the transport;
`companion.ts` / `assistant.ts` keep their exports for the components that
import them directly. The HTTP contract itself is unchanged:

Routes the browser depends on (see `src/lib/companion.ts`):
- `GET /health` — no auth; `{ok, platform}`
- `GET /inventory` — memory-scans the running game and returns inventory.json
  directly (no file). Token-authed; **JWT-free** (in-memory session creds only,
  so it works without `login`). 503 + `{error}` when the game isn't scannable.
  The app pulls this on the "Pull/Refresh inventory" button and auto-pulls when
  it arrives via the companion's deep link (`#companion=<url>?token=…`).
- `POST /plan` — submit listing batch
- `GET /plan/pending` — last pending plan or 404 (no JWT — safe to poll on connect)
- `POST /plan/resume` — re-runs skipping completed items
- `DELETE /plan/pending` — discard
- `GET /orders` — user's listings (enriched server-side with item names)
- `POST /orders/visibility` — bulk toggle
- `PATCH /order/<id>` — price/qty/visible/rank
- `DELETE /order/<id>`

All authed routes require `X-Session-Token` from the URL the
companion prints at startup.

**Listing unlock (lazy JWT).** `serve` starts without decrypting the JWT, so
`/health`, `/inventory`, and `/plan/pending` work with no login. The
listing/order routes (`/plan`, `/plan/resume`, `/orders`, `/orders/visibility`,
`/order/<id>`) unlock the JWT on first use and can return:
- **401 `{error, needs_login: true}`** — no login on the companion; steer the
  user to run `wfm-fetch-inventory login`.
- **503 `{error, needs_login: false}`** — login exists but couldn't unlock (the
  companion has no terminal to prompt for the passphrase, or a transient
  failure). The first real listing request may also block while the companion
  prompts for the passphrase in *its* terminal, so surface a "check the terminal
  running serve" hint on the List action.

---

## Crypto

Encrypted export (`src/lib/crypto.ts`):
- **PBKDF2-HMAC-SHA256**, 600,000 iterations (OWASP 2023).
- **AES-256-GCM**, fresh 12-byte IV + 16-byte salt per export.
- Native WebCrypto. No third-party crypto libraries.

Same parameters mirror the companion's `wfminv-jwt-v1` on-disk format
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

Allowed `connect-src` (hosted): `self`, `http://127.0.0.1:*`,
`http://localhost:*`. The two loopback entries are for the companion;
there are no third-party origins. The CSP ships in three places
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
(loopback dropped — there is no companion HTTP server in desktop; the Tauri
IPC scheme added so `invoke` uses the fast path with no CSP violations; plus
the one C4 refresh origin). It NEVER touches the three hosted copies, so the
hosted CSP stays byte-identical. `companion/tennoworth-desktop`'s
`frontendDist` points at `dist-desktop`.

---

## Hygiene

- **No comments that restate the code.** Comments explain *why*.
- **No backwards-compat shims** for code that hasn't shipped yet.
  Renaming a state field? Bump the storage key version and move on.
- **Edit existing files** in preference to creating new ones.
- **Match the scope of the request.** Bug fix ≠ refactor pass.
- **Verify in the browser.** For UI changes, drive Playwright or open
  the dev server and use the feature. "Tests pass" ≠ "feature works."
