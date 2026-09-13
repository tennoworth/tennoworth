# frontend/ - Svelte 5 + Vite web app

## Required design reference

Before any UI markup, style, layout, or interaction-state change, read
[`../docs/design-system.md`](../docs/design-system.md) completely. Reuse its
tokens and patterns, preserve behavior and edits during resizing, and verify
both themes and narrow/short/wide layouts in the actual styled app. Generic
design suggestions do not override this product's visual contract.

This gate is repeated in the root and rust instruction files on purpose: missing
it is expensive, and each copy is read where UI work starts.

Shared frontend for the desktop app, overlay, and static informational site.
The hosted surface has no accounts or inventory input; the desktop app owns
interactive operations. Read ../docs/architecture.md for capability boundaries.

Use ../CONTRIBUTING.md for development URLs, setup, and required checks.
Use the committed bun.lock with `bun install --frozen-lockfile`.

---

## Architectural rules

### No third-party fetches from the browser. None.
WFM serves no `Access-Control-Allow-Origin` header. Direct browser
fetches will fail CORS. All WFM data must come from the static
`market.json` snapshot under `public/`, produced by the box's Rust
`wfm-scrape`; desktop release preparation refreshes the repo copy from the
box's locked pair.

`warframestat.us` used to be the one allowed direct call (it sent
CORS headers; the resolver hit `/items/` for `/Lotus/...` paths) --
**upstream dropped its CORS headers on 2026-06-09** and broke every
inventory upload. The resolver catalog is now baked too:
`wfm-scrape build` writes `public/wfstat-catalog.json` (slim
`[uniqueName, {name, category}]` pairs, forced `Accept-Language: en` --
localized names silently fail the WFM name join). All vendor data
(relic rewards, vault status, **Baro schedule** - `market.baro`,
resolver catalog) is baked at build time and served same-origin; the
CSP `connect-src` has no third-party origins left. A runtime
warframestat fetch broke this rule once before and vanished during
outages - don't reintroduce one.

Desktop network access stays in Rust. Frontend features consume typed
capabilities from `src/contracts/`; shells choose implementations from
`src/adapters/`. Hosted capabilities provide public market/history access only,
not fake interactive methods. Preserve cache-first startup and strictly newer
snapshot replacement. The webview must never receive the WFM JWT.

### One source of truth for owned-item resolution
`src/domain/resolver.ts` is the only place that maps a `/Lotus/...` path
to a `{name, slug, category}`, reading the baked
`/wfstat-catalog.json`. All UI code joins through
`market.items[slug]` (stats) and `market.catalog[name_lower]` (slug
lookup).

### Market intelligence must preserve unknowns

Do not turn a failed/missing join into a plausible number. Meta Drift and
calendar rows render absent price/volume/reward reach as `-` or `unknown`, not
`0p`, zero volume, or "none you hold". Annual usage comparisons use the latest
two actually available years, compare only the same slug in the same category,
and label one-year-only rows `Only in <year> data` - never "new", "removed", or
a zero-filled change. A non-consecutive pair names the real gap and is not
called YoY. Annual immutable data also does not use the generic seven-day stale
warning; its freshness is the source year.

Percentage-point deltas must never render a real nonzero change as `+0.00` or
`−0.00`. Shares are rounded to four percentage-point decimals, so display four
decimals below `abs(delta) < 0.01` and two at/above that boundary. This is a
display-resolution rule, not a significance cutoff; do not filter tiny valid
movements.

---

## Svelte 5 rules - the non-obvious ones we've already hit

### `$effect` cannot read and write the same state
`$effect` tracks every reactive read inside its body as a dependency.
If the effect then writes to that state, the write re-triggers the
effect → infinite loop. We hit this concretely (Maximum update depth
exceeded) when an init effect wrote `resolved` and then called
`recomputeResults()` which read `resolved.owned`.

**Rule:** for one-time initialization, use `onMount` (no reactivity
tracking). Use `$effect` only when you genuinely want re-runs on
state change - and even then, never write to anything the effect
reads.

### `$derived` must be pure
No side effects, no writes, no mutations. If it computes a value, it
goes in `$derived`. If it performs an action (including saving to
localStorage), it goes in an event handler or `$effect`.

### Don't destructure `$state` objects
Destructuring takes a snapshot - you get plain values, not reactive
bindings. Always access through the original (`user.name`), never
`const { name } = user`.

### Use event attributes, not directives
`onclick={fn}`, not `on:click={fn}`. The directive form is deprecated
in Svelte 5 and will warn during build.

### Pass callbacks as props, not via `createEventDispatcher`
Child components take callbacks (`onunlocked={fn}`, `onimport={fn}`) as
`$props()` props, not `dispatch(...)`.

### Conditional surfaces do not isolate global component CSS
A component's `<style>` is bundled when the component is statically imported,
not when it is mounted. A conditional `mount()` therefore does not stop
`:global(html)`, `:global(body)`, or `:global(#app)` rules from affecting the
normal app. The relic overlay once shipped `overflow: hidden` to the hosted
landing this way and disabled scrolling at every viewport width.

Standalone surfaces with document-level CSS must be dynamically imported and
must gate those global selectors behind a surface-specific root class. Browser-
check both the normal URL and the surface URL; a component test cannot observe
whether Vite leaked its extracted CSS into another entry path.

---

## Browser storage - when to use which

| Need | Pick |
|---|---|
| Small key/value, sync, ≤5 MB | `localStorage` |
| Multi-MB structured cache (item catalogs) | IndexedDB |
| HTTP request/response cache | Cache API (not used yet) |

The key mapping lives in `src/adapters/state-store.ts`; its `SettingKey`
contract lives in `src/contracts/state-store.ts`. Snapshot keys live in
`src/adapters/browser-snapshot.ts`. Keep key inventories in code and its tests
rather than maintaining another list here.

**Don't read/write these via raw `localStorage` calls.** Go through
`src/adapters/state-store.ts`'s `store.getSetting`/`setSetting` (`SettingKey`
contract) - it's the one seam that also backs the desktop build (Tauri/SQLite)
with the same calls; a raw `localStorage.setItem` silently no-ops there.
New setting → extend `SettingKey` and `LOCAL_SETTING_KEYS` together.
`hydrate()` derives its key list from `LOCAL_SETTING_KEYS` (no separate
literal), and `state-store.test.ts` pins the full set with a set-equality
gate - add a key without its fixture and the suite fails, so the "extend
the seam" rule can't drift.

IndexedDB DB:
- `wfminv` / store `catalogs` / key `wfstat-items-v3` - slim
  `[uniqueName, {name, category}]` pairs from the baked
  `/wfstat-catalog.json` (v2 caches could hold localized names).

**Always bump the version suffix in the key when the stored shape
changes** so old data is silently invalidated. **And add the outgoing key to
`RETIRED_KEYS` in `catalog-cache.ts` in the same edit** - bumping invalidates
the old row but cannot delete it, because nothing reads a key it no longer
knows. The v2→v3 bump shipped without this and left a dead multi-hundred-KB
row in every existing user's IndexedDB, reachable by no code path until
`purgeRetiredCaches()` was added. Invalidation is not reclamation.

---

## Desktop transport (Tauri IPC)

`src/contracts/desktop.ts` and the other capability contracts describe runtime
operations; `src/adapters/desktop.ts` binds native IPC. Features receive services
through composition rather than importing concrete adapters. Generated contracts
live in `src/contracts/generated/`; follow ../docs/architecture.md when changing
native request/response shapes. Browser previews do not verify native registration.

Listing/order commands reject with a typed `{code, message}` CmdError that
surfaces as `DesktopCmdError` - `needs_login` / `needs_unlock` drive the
SPA's login and passphrase dialogs.

An auth-gated surface that fetches on mount (MyOrdersPanel) must route a
`needs_login` / `needs_unlock` rejection to the auth dialogs via an
`onauthrequired` prop and refetch when the session unlocks (`sessionEpoch`)
- never render `humanError(e)` for those codes. That routing is what lets
the OS-keyring silent unlock fire for the surface; without it a keyring
unlock that should "just work" becomes an error banner.

Rust→SPA push events (the tray hint, the update check) go through the
shared no-op-safe `listenForTauriEvent` in `desktop-update.ts` - not a
per-event file - and the event name is pinned by a test (`tray.test.ts`)
so the Rust↔TS literal can't drift silently.

---

## Encrypted snapshots

Encrypted snapshot import/export uses WebCrypto through
`src/adapters/encrypted-snapshot.ts` and `src/contracts/encrypted-snapshot.ts`.
Preserve the PBKDF2-HMAC-SHA256 (600,000 iterations) and AES-256-GCM envelope,
with fresh salt and IV for each export. WFM JWT encryption and credentials
remain Rust-side; snapshot encryption does not permit exposing those secrets.
Follow ../SECURITY.md for trust boundaries.

---

## CSP & headers

Production serves through **Caddy on the self-host box**, which applies
the full header set (HSTS, `frame-ancestors` / X-Frame-Options, the
CSP) from `deploy/Caddyfile` - kept in sync with the other CSP copies
by `scripts/sync-csp.ts`. The `<meta http-equiv="Content-Security-Policy">`
in `index.html` still ships script/connect/style protection as a
belt-and-suspenders fallback. The `public/_headers` file only matters
for preview deployments on Cloudflare Pages / Netlify / Vercel (GitHub
Pages silently drops it), where the header host isn't ours.

Allowed `connect-src` (hosted): `self`. The hosted site makes no
loopback or third-party calls - the loopback entries were for the removed
companion CLI. The CSP ships in three places
(`index.html` meta, `public/_headers`, `deploy/Caddyfile`) but is
**edited in ONE**: `scripts/sync-csp.ts`. Change the directives there,
run `bun run csp` to rewrite all three; `bun run build` fails via its
prebuild `--check` if any copy drifted. (The meta copy deliberately
omits `frame-ancestors` - browsers ignore it in meta tags.)

**Desktop (Tauri) is a build-variant, not a fourth committed copy.**
`bun run build:desktop` builds to `dist-desktop/` (gitignored) and runs
`sync-csp.ts --desktop dist-desktop/index.html`, which rewrites only that
built file's meta CSP to
`connect-src 'self' ipc://localhost http://ipc.localhost https://tennoworth.app`
(the Tauri IPC scheme added so `invoke` uses the fast path with no CSP
violations; plus the one C4 refresh origin). It NEVER touches the three hosted
copies, so the hosted CSP stays byte-identical.
`rust/tennoworth-desktop`'s `frontendDist` points at `dist-desktop`.

---

## Hygiene

The shared hygiene rules - no comments that restate the code, no
backwards-compat shims, edit existing files, match the scope of the request -
are in the root instruction file. These are frontend-specific:

- **No native `alert()` / `confirm()` in the SPA** - use the
  `Toast.svelte` corner stack and inline row-confirms. Browser-native
  dialogs read as "the app broke".
- **Listing-visibility copy says "hidden", never "invisible"** (2026-08-03
  rename). The WFM field/API name stays `visible`; only user-facing copy
  uses "hidden".
- **The unlock dialog always offers "Forgot it? Log in again"** - a
  forgotten passphrase is unrecoverable by design; re-login is the only
  reset, so the escape hatch must never be removed.
- **Verify in the browser, and A/B the fix.** For UI changes drive
  Playwright or the dev server. "Tests pass" ≠ "feature works" - and a
  passing test after a fix doesn't prove the bug was real. Revert the fix,
  re-run the same script, watch it fail, restore. That is what turned R1
  (the review modal resetting in-flight edits) from a plausible reading of
  the code into a demonstrated defect.
  Gotchas that cost time here: the Playwright MCP wants Chrome specifically
  but `~/.cache/ms-playwright/chromium-*/chrome-linux64/chrome` works when
  passed as `executablePath`; the app stays on the onboarding view until an
  inventory is loaded, so anything gated on a desktop session (the bulk
  List CTA, the orders panel) is invisible before that; the review modal is a
  `div[role="dialog"]`, not a `<dialog>`; use the fictional preview samples documented in CONTRIBUTING.md.
- **Runtime evidence must be the real styled surface.** A component harness
  without the app stylesheet can prove text exists but cannot prove responsive
  behavior. Capture the actual hosted/desktop route at wide and narrow widths;
  for horizontally scrolling tables, scroll far enough that the changed
  columns are visible. Rails that wrap on mobile must override any global
  fixed height with an auto/min height and padding - the Meta Drift year-gap
  label once existed in the DOM but was clipped beneath a one-line rail.
