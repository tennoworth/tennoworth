# Desktop spike findings — Phase C day-1 de-risking

**Date:** 2026-07-20 · **Branch:** `feat/desktop-spike` (off `main` @ 2f03eb9)
**Goal:** verify how the existing Svelte SPA behaves inside a real Tauri v2
WebKitGTK webview, before committing to the Phase C desktop build. This is a
**spike** — the code is a minimal scaffold + throwaway instrumentation, not a
feature surface.

## TL;DR

All five questions answered with runtime evidence. **No blocker found.** The
storage-amnesia risk that killed the companion-serves-SPA plan does **not**
recur: the Tauri origin is a stable `tauri://localhost`, and localStorage +
IndexedDB persist across restarts. Root-relative `/market.json` and
`/wfstat-catalog.json` resolve unchanged through the asset protocol. IPC into
`wfm-core` works. The one real gotcha is CSP: the current SPA meta CSP blocks
the C4 remote refresh (as designed) **and** makes every Tauri IPC call emit a
`connect-src` violation (invoke still works via a postMessage fallback, but
noisily) — the desktop target needs its own `connect-src`.

## Environment (observed)

| Thing | Value |
|---|---|
| OS / session | CachyOS (Arch), **Wayland** session (`XDG_SESSION_TYPE=wayland`), XWayland also available (`DISPLAY=:0`) |
| webkit2gtk-4.1 | 2.52.5 · gtk+-3.0 3.24.52 · libsoup-3.0 3.6.6 · ayatana-appindicator3 present |
| Rust / cargo | 1.91.0 |
| tauri-cli | 2.11.4 (installed user-level via `cargo install tauri-cli --locked`) |
| tauri crate / tauri-build | 2.11.5 / 2.6.3 · tao 0.35.3 · webkit2gtk-sys 2.0.2 (the 4.1 API, via soup3) |
| bun | 1.3.14 |

The webview ran on the **first attempt** with the ambient Wayland display — no
`WEBKIT_DISABLE_COMPOSITING_MODE` or `xvfb-run` fallback was needed (and
`xvfb-run` is not installed here anyway).

## Scaffold committed

- `companion/tennoworth-desktop/` — new workspace member (added to
  `companion/Cargo.toml` `members`). Minimal Tauri v2 crate:
  - `tauri.conf.json` — `frontendDist: "../../prototype/dist"`,
    `app.security.csp: null`, `withGlobalTauri: true`, `windows: []` (the window
    is built in Rust so the spike probe's init script can be attached).
  - `src/main.rs` — one window loading the SPA + three commands: `hello`
    (returns the linked `wfm-core` version — the IPC round-trip), and the
    spike-only `spike_report` / `spike_exit`.
  - `capabilities/default.json`, `build.rs`, `icons/` (desktop set only).
- `companion/wfm-core/src/lib.rs` — added a trivial, side-effect-free
  `pub fn version() -> &'static str` (returns `CARGO_PKG_VERSION`) for the
  `hello` command to call. CLI unaffected; `cargo test -p wfm-core` = 44 passed.

**Build order dependency:** `frontendDist` points at `prototype/dist`, which is
git-ignored. `cd prototype && bun install && bun run build` must run before any
`cargo tauri dev|build`. Assets are embedded into the binary at compile time
(tauri-codegen), so re-running the SPA build requires a desktop rebuild to take
effect — patching `dist/` under a built binary does nothing.

## Evidence method

Instrumentation is opt-in behind `SPIKE_PROBE=1` (the default-run shell is a
plain window). With it set, an `initialization_script` (`PROBE_JS` in
`main.rs`) runs at document-start in the SPA's own context — so it is governed
by the SPA's real meta CSP — and records origin, storage, fetch, IPC, CSP
violations, and SPA-mount state. Results are exfiltrated three redundant ways:
the `spike_report` command (→ file + stdout), `localStorage`/`IndexedDB` (read
back off disk), and the Rust-side `WebviewWindow::url()`. Four launches were
run (run1–run4); run2–run4 read the previous run's marker to prove persistence.

---

## Q1 — Root-relative fetches & origin

**Answer: they resolve to the bundled dist files, unchanged. Origin is
`tauri://localhost`.**

Rust-side (`WebviewWindow::url()`) and JS-side (`location.origin`) agree:

```
SPIKE_WEBVIEW_URL tauri://localhost
"origin":"tauri://localhost","href":"tauri://localhost","protocol":"tauri:"
```

Both root-relative fetches succeed through Tauri's asset protocol, served
same-origin (`response.type: "basic"`):

```
"fetchMarket":  {"ok":true,"status":200,"type":"basic","len":1556217,"head":"{\"updated_at\":\"2026-07-19T08:02:36Z\",\"platform\":"}
"fetchCatalog": {"ok":true,"status":200,"type":"basic","len":1927898,"head":"[[\"/Lotus/Types/StoreItems/AvatarImages/FanChann"}
```

`prototype/src/lib/market.ts` (`fetch('/market.json')`) and `resolver.ts`
(`fetch('/wfstat-catalog.json')`) need **no change** for the desktop target —
`include_bytes!` is unnecessary for these two; `frontendDist` already embeds
them and the asset protocol serves them at the root. (C4's *bundled bootstrap +
remote refresh* is a separate concern — see Q2.)

## Q2 — CSP

**Answer: the SPA boots cleanly under its existing meta CSP. The remote fetch
fails specifically because of `connect-src` (proven, not a network artifact).
Tauri's own `csp` should stay `null`; the binding policy is the SPA meta tag.**

SPA boots under the CSP inside the webview (run3):

```
"appMounted":true,"appChildCount":5,"bodyTextLen":5531,
"spaTitle":"TennoWorth — what to sell in Warframe, right now",
"consoleErrors":[]
```

`withGlobalTauri` injected `window.__TAURI__` without tripping `script-src
'self'` (Tauri injects via a privileged init script, not an inline `<script>`).

Captured `securitypolicyviolation` events (run3/run4) — **definitive**
attribution, independent of network (the violation fires at the CSP check,
before any socket):

```json
"cspViolations":[
  {"blockedURI":"https://tennoworth.app/market.json","violatedDirective":"connect-src","effectiveDirective":"connect-src","disposition":"enforce"},
  {"blockedURI":"ipc://localhost/hello","violatedDirective":"connect-src","effectiveDirective":"connect-src","disposition":"enforce"}
]
```

Note: network egress *does* work in this environment (the AppImage tooling
downloaded from GitHub during `tauri build`), so the `tennoworth.app` failure is
CSP, not connectivity — corroborating the violation event.

**Two consequences:**

1. **C4 remote refresh** (`https://tennoworth.app/market.json`) is blocked, as
   the plan anticipated. Unblocking needs `https://tennoworth.app` in
   `connect-src` **and** a Tauri capability/`security` allowance for that one
   origin.
2. **IPC is CSP-relevant too.** Tauri v2's primary IPC transport is a
   `fetch('ipc://localhost/<cmd>')`, which the current `connect-src 'self' …`
   blocks — hence the second violation above. Invoke *still worked* (Q4) because
   Tauri v2 **falls back to a postMessage transport**, but every call emits a
   CSP violation and pays a failed-fetch + fallback cost.

**Tauri `csp` config:** leave `app.security.csp = null`. When null, Tauri
injects/merges nothing and the SPA's `<meta>` CSP is the sole policy. Setting it
non-null would add a *second* CSP header; browsers enforce the **intersection**
of all policies, so the restrictive meta tag would still bind `connect-src` —
setting Tauri's csp does not loosen the meta tag. The fix belongs in the meta
CSP the desktop build ships (see recommendation).

## Q3 — Storage identity & persistence

**Answer: localStorage + IndexedDB are keyed to the stable `tauri://localhost`
origin and persist across process restarts. This is the property the
random-port plan lacked.**

Marker chain across four separate launches (each run reads the prior run's
value from **both** stores):

| run | priorLocalStorage | priorIndexedDB |
|---|---|---|
| run1 | `null` | `null` |
| run2 | `run1@2026-07-19T23:08:20.771Z` | `run1@…` |
| run3 | `run2@2026-07-19T23:08:57.766Z` | `run2@…` |
| run4 | `run3@2026-07-19T23:11:16.567Z` | `run3@…` |

On-disk confirmation (data dir keyed to the bundle identifier
`app.tennoworth.desktop`):

```
~/.local/share/app.tennoworth.desktop/localstorage/tauri_localhost_0.localstorage        (SQLite)
~/.local/share/app.tennoworth.desktop/databases/indexeddb/v1/tauri_localhost_0/…/IndexedDB.sqlite3
```

The `tauri_localhost_0` filename **is** the origin key (scheme `tauri`, host
`localhost`, port `0`) — fixed, identifier-derived, no random port. SQLite dump
of the localStorage table (`ItemTable`, values UTF-16LE):

```
__spike_marker__  -> run2@2026-07-19T23:08:57.766Z
__spike_report__  -> {"runtag":"run2","origin":"tauri://localhost",…}
```

The old `wfminv:*` localStorage keys and the `wfminv`/`catalogs` IndexedDB DB
(see `prototype/CLAUDE.md`) will therefore carry over between desktop launches
exactly as they do in a normal browser tab. **De-risked.**

## Q4 — Tauri command IPC

**Answer: round-trip works.** `invoke('hello')` returns the live `wfm-core`
version from the SPA's document context, in every run:

```
"invokeHello":"wfm-core 0.1.0"
```

`hello` calls `wfm_core::version()` — proving the crate is linked and callable
across the IPC boundary, which is exactly the C2 premise (a high-level SPA
operation can be serviced by a Tauri command that drives `wfm-core`). Caveat:
the postMessage-fallback / CSP-violation nuance from Q2 applies to every invoke
until the desktop `connect-src` allows the `ipc:` scheme.

## Q5 — Window basics & bundling

**Session / window:** native **Wayland** (346 lines of Wayland protocol traffic
under `WAYLAND_DEBUG=1` — `wl_compositor`, `xdg_wm_base`, `wl_surface` binds;
XWayland not used despite being available). The 1200×800 window opened, the SPA
rendered, and the process produced no GTK/WebKit errors or warnings on
stdout/stderr. No DPI/scale anomaly surfaced in logs (a pixel-level DPI check
wasn't possible from the agent; Wayland fractional scaling remains the usual
thing to eyeball on real hardware).

**`cargo tauri build` bundles** (`bundle.targets: "all"`, no extra config):

| Target | Result |
|---|---|
| `.deb` | **OK** — `bundle/deb/TennoWorth_0.1.0_amd64.deb` (1.81 MB) |
| `.rpm` | **OK** — `bundle/rpm/TennoWorth-0.1.0-1.x86_64.rpm` (1.81 MB) |
| `.AppImage` | **Failed** — `Error failed to bundle project: 'failed to run linuxdeploy'` |

The AppImage tooling downloaded fine (linuxdeploy + gtk/gstreamer plugins from
GitHub); `linuxdeploy` itself then failed to run. FUSE2 *is* present
(`libfuse.so.2`, `fusermount3`), so this is a sandbox mount restriction, not a
missing dependency. The standard CI workaround is `APPIMAGE_EXTRACT_AND_RUN=1`
(avoids the FUSE mount). **Not chased** — noted only, per the spike brief. deb +
rpm are sufficient to prove Linux bundling works here; C8's AppImage-first path
needs an env that permits the linuxdeploy mount (or the extract-and-run env
var). Release binary itself: 3.75 MB (`opt-level=z` + strip + fat LTO).

---

## Recommendation for C2's transport-abstraction shape

What the spike settles:

- **Data loading needs no abstraction.** `loadMarket()` / `loadCatalogs()` keep
  their root-relative `fetch('/…')` in the desktop target — the asset protocol
  serves them same-origin already.
- **Companion IPC does.** Keep the high-level operation names
  (`fetchInventory`, `submitPlan`, `loadOrders`, `runAssistant`, …) behind a
  small `Transport` interface with two implementations, selected once at boot by
  sniffing `window.__TAURI_INTERNALS__`:
  - `HttpCompanionTransport` — today's `companion.ts`: URL + `X-Session-Token`,
    `#companion=` fragment handshake, `/health` probe, loopback error taxonomy.
    Stays for the hosted/`serve` path.
  - `TauriTransport` — each op is `invoke('<cmd>')` into `wfm-core`. **Strip**
    the URL/token config, fragment handshake, health/token UI, and loopback
    error taxonomy entirely (exactly as C2 says — not a fetch→invoke swap of the
    call sites, a whole config surface deleted from the desktop build).
- **CSP is a build-variant, not a code, concern.** The desktop `index.html`
  needs a different `connect-src` than the hosted one. `connect-src` should be
  roughly:

  ```
  connect-src 'self' ipc://localhost http://ipc.localhost https://tennoworth.app
  ```

  (`ipc:`/`http://ipc.localhost` clear the per-invoke violation and enable the
  fast IPC path; `https://tennoworth.app` is the single C4 refresh origin; the
  `http://127.0.0.1:*` / `http://localhost:*` loopback entries are **not**
  needed in the desktop target — there is no companion HTTP server). Because
  `scripts/sync-csp.mjs` is already the single CSP source for the three hosted
  copies, add a desktop CSP variant there and have the desktop build emit an
  `index.html` with this policy, rather than loosening the hosted policy or
  fighting CSP intersection with a Tauri-managed `csp`.
- **Nothing here is a blocker.** Basic invoke already works under the current
  CSP; the `connect-src` change is required only for (a) console cleanliness /
  IPC-path performance and (b) C4's remote refresh.

**Status: implemented in C2** (`feat/desktop-transport`). The transport lives at
`prototype/src/lib/transport.ts` (HTTP + Tauri impls, `__TAURI_INTERNALS__`
boot sniff); the desktop commands (`health`, `scan_inventory`) are in
`companion/tennoworth-desktop/src/main.rs`; the desktop CSP variant is a
`scripts/sync-csp.mjs --desktop` rewrite emitted by `bun run build:desktop`.

## Reproduce

```bash
# Desktop dist carries the desktop CSP (connect-src with ipc://…); frontendDist
# points at prototype/dist-desktop, so build THAT before the crate:
cd prototype && bun install && bun run build:desktop  # → prototype/dist-desktop
cd companion && cargo build -p tennoworth-desktop     # or: cargo tauri build (runs build:desktop for you)
# Plain shell:
companion/target/debug/tennoworth-desktop
# Evidence probe (writes JSON, drives the scan button, auto-exits):
TENNOWORTH_PROBE=1 TENNOWORTH_RUNTAG=run1 TENNOWORTH_PROBE_OUT=/tmp/probe.json \
  companion/target/debug/tennoworth-desktop
```

`cargo tauri build` here bundles deb + rpm OK; AppImage still fails at
`linuxdeploy` (the same sandbox mount restriction as the spike, Q5) — not a code
issue. **Path gotcha:** in `tauri.conf.json`, `frontendDist` is relative to the
config file (`tennoworth-desktop/`) → `../../prototype/dist-desktop`, but Tauri
runs `before{Dev,Build}Command` from the config's PARENT (`companion/`), so that
command uses `cd ../prototype` (one `..` fewer). Verified by printing `pwd`.
