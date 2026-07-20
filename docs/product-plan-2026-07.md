# TennoWorth product plan — July 2026

Built from a three-way adversarial review (Fable orchestrating, Codex with
repo access, DeepSeek v4-pro) of a draft that was itself picked apart. Every
estimate below uses Codex's repo-grounded corrections, not the draft's.

**North star:** the fastest *trustworthy* answer to "what should I sell right
now" on Windows + Linux, no Overwolf.
**Success metrics:** new Windows user, clean machine: download → first answer
in < 3 min with zero terminal. Returning user: answer < 5 s from launch.

---

## Decisions locked (and why)

1. **The desktop app (Tauri v2) is the destination.** Same-origin webview
   kills the whole LNA/loopback/random-port class. The companion-serves-SPA
   pivot is dead (random port = storage amnesia).
2. ~~**Sign everything with one publisher identity, starting now.**~~
   **REVERSED 2026-07-20 — user declined to buy a code-signing cert.** The
   original call was a Certum Open Source cert (Azure Trusted Signing is
   closed to EU individual devs). That is off the table. Consequence,
   stated plainly: the unsigned-binary SmartScreen wall (the review's #1
   Windows adoption killer) is now an **accepted risk**, not a solved one —
   the desktop Tauri exe warns just like the CLI. Mitigation pivots to
   **package-manager distribution** (winget + Scoop on Windows; AUR +
   Flatpak/AppImage on Linux, our wedge) plus the trust page + SHA256SUMS +
   honest install copy. The B2 signing pipeline stays merged but **dormant**
   (every step no-ops without secrets), so the decision is reversible by
   adding secrets alone. See docs/signing-runbook.md and the
   windows-trust-via-package-managers task.
3. **Window + notifications first; overlay is a v2 spike, not v1 scope.**
   DeepSeek pushed overlay-first (click-through HUD). Rejected for v1: it is
   the highest-maintenance surface (DPI, fullscreen modes, per-driver bugs —
   DeepSeek's own sustainability section says so), and our wedge is *existing
   on Linux*, not overlay parity. Stepping stone in v1.5: a compact
   always-on-top "top 5" mini-card (cheap, no click-through needed).
4. **Linux desktop stays v1** (the wedge and the maintainer's own platform),
   but Linux *tray* is de-scoped to best-effort; window + notifications are
   the Linux baseline. (DeepSeek's "drop Linux" rejected.)
5. **No EE.log inventory fallback — it does not exist.** Fact-checked:
   EE.log is a debug log; WFInfo only uses it to detect the reward screen
   then OCRs. AlecaFrame itself memory-scans via an Overwolf plugin
   (gep_warframeext.dll, ReadProcessMemory). Consequence for trust copy: say
   plainly "AlecaFrame reads game memory too — through Overwolf; we do the
   same thing without the middleman."
6. **SQLite ships with the desktop app** — justified because inventory
   *history* is committed v1 scope (it powers profit tracking and
   trend-aware advice, the differentiator vs a WFM mirror). Settings alone
   would not justify it (Codex's point, accepted).
7. **The hosted site is permanent**: landing, no-install market browser,
   docs, trust page, file-drop. It is the demo and discovery surface, not
   the app. The `serve` loopback mode stays for the transition then becomes
   a power-user flag.

---

## Phase A — this week (6–10 eng days total, all independent)

**A1. LNA compatibility patch (0.5–1 d).** Add
`targetAddressSpace: "loopback"` to ALL companion fetches — companion.ts has
two call sites and assistant.ts has a third direct fetch (easy to miss).
Permission probing already exists in App.svelte (don't rebuild). Rewrite the
Firefox banner: Settings → Privacy & Security → Permissions grant path;
delete the shield-toggle advice (wrong mechanism). Keeps the hosted path
alive during the entire desktop build. (DeepSeek's "kill it" rejected:
0.5–1 d bridge for a 6–10 week gap.)

**A2. Landing = instant no-install value (4–7 d, not the draft's 2–3).**
First screen becomes a market browser powered by market.json alone (verified
sufficient: 2,719 items with avg/low_sell/top_buy/vol/tags, `medians_7d`
sparkline series, 90-d medians and Donchian bands, vault_status,
relic_rewards, Baro schedule). Scope: item search, top movers
(median_now vs median_90d), vaulted/returning report, Baro countdown.
Explicitly OUT: Baro stock/flip view — market.json carries Baro
activation/expiry/location only; a stock source is a separate task. Needs a
market-only route + no-inventory variants of the recommendation logic (the
landing is currently inventory-gated at the `phase !== 'done'` branch).

**A3. Trust copy honesty pass (1–2 d).** Align FAQ + companion README with
SECURITY.md ("we cannot promise ban-safe"); delete unsupported EAC-behavior
claims. New player-readable trust page: what is read, what never leaves the
machine, where the JWT lives, the AlecaFrame-also-memory-scans fact, open
source + CI-built releases. Qualify "reproducible builds" honestly once the
desktop app exists (webview glue is not reproducible; the core crates are).
Reposition file-drop as an offline/connectivity fallback — it is circular
for trust purposes (the file comes from the companion).

**A4. Buy the Certum cert now (elapsed-time starter, ~0 eng days).**
Identity validation takes days-to-weeks; start it before any code needs it.

## Phase B — weeks 2–3: the two real gates

**B1. `wfm-core` extraction (the Track-2 gate; ~1–2 wk).** Codex's key
finding: wfm-fetch-inventory is a 3,140-line `main.rs` with NO lib target —
scan, fetch, login, JWT storage, server, listings, assistant relay all
private. Extract a `wfm-core` crate: public DTOs/errors, process
detection/scan (single-flight + cancellation — the current server does
one-thread-per-request with no scan lock), inventory fetch, WFM auth + JWT
storage (preserve `~/.config/wfminv/wfm-jwt.enc` format), listing service,
pending-plan store. CLI becomes adapter #1; Tauri commands later become
adapter #2. The stdin/rpassword login flow must be abstracted (desktop
passphrase prompt is a UI concern). Reuse reality: market-math is clean,
wfm-client selectively (reconcile the duplicate WFM client in main.rs),
wfm-scrape stays host-only and OUT of the desktop runtime.

**B2. Signing pipeline (5–10 eng d + 1–3 wk external elapsed).**
release-companion.yml today: build → rename → SHA256SUMS → release. Add:
Authenticode signing (Certum) + timestamping, signature verification step in
CI and in install.ps1 (currently hash-only), key-recovery runbook. Installer
decision before winget: keep portable exe for CLI; the DESKTOP app gets a
real installer (NSIS/MSI via Tauri bundler) and that is what goes to winget.
AV playbook: VirusTotal scan per release in CI, Microsoft Security
Intelligence false-positive submission documented. Gate: clean Win11 VM
first-run test, zero terminal.

## Phase C — weeks 3–9: TennoWorth Desktop (30–50 eng days, Codex-corrected)

- **C1. Tauri v2 shell** in the cargo workspace; webview loads the existing
  SPA; `wfm-core` invoked via Tauri commands. No HTTP, no ports, no LNA.
- **C2. Transport abstraction in the SPA.** Not a fetch→invoke swap: keep
  the high-level operation names (fetchInventory, submitPlan, …) but strip
  the URL/token config, `#companion=` fragment handshake, health/token UI,
  and loopback error taxonomy out of the desktop target. Root-relative
  `/market.json` + `/wfstat-catalog.json` and the CSP must be validated
  inside the Tauri asset protocol early (day-1 spike, not a late surprise).
- **C3. State = SQLite** (rusqlite, app-data dir), canonical from day one:

  ```sql
  CREATE TABLE snapshot (
    id INTEGER PRIMARY KEY,
    taken_at TEXT NOT NULL,            -- ISO8601 UTC
    source TEXT NOT NULL CHECK(source IN ('memory','import')),
    game_version TEXT
  );
  CREATE TABLE snapshot_item (
    snapshot_id INTEGER NOT NULL REFERENCES snapshot(id),
    slug TEXT NOT NULL,                -- resolved item slug
    count INTEGER NOT NULL,
    leveled INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (snapshot_id, slug)
  );
  CREATE TABLE setting (key TEXT PRIMARY KEY, value TEXT NOT NULL);
  CREATE TABLE reserve (slug TEXT PRIMARY KEY, keep INTEGER NOT NULL);
  CREATE TABLE listing_log (            -- what we listed, when, at what price
    id INTEGER PRIMARY KEY,
    slug TEXT NOT NULL, listed_at TEXT NOT NULL,
    price INTEGER NOT NULL, qty INTEGER NOT NULL,
    outcome TEXT                        -- NULL until sold/cancelled observed
  );
  ```

  History across `snapshot` rows + `listing_log` is the profit-tracking and
  trend-advice substrate. Client-side market-price history is NOT stored —
  the server's market.json (with medians_7d/90d built in) remains the price
  source of truth. Migration from the browser app: manual encrypted-export
  import (that's what the export actually contains: `{invName, ts, owned}`)
  — never advertised as automatic.
- **C4. Market data:** bundled bootstrap market.json via include_bytes!,
  ETag-cached refresh from https://tennoworth.app/market.json, loud
  staleness banner (>24 h), explicit Tauri capability + CSP entry for the
  one remote origin.
- **C5. Auto-update:** Tauri updater against GitHub releases; updater
  keypair generated offline, documented rotation/recovery. No silent
  updates — notify + apply on restart.
- **C6. Surfaces v1:** main window (existing dashboard), post-scan system
  notification ("3 new sellables, ~120p"), Windows tray with top-5 menu;
  Linux = window + notifications (tray best-effort behind libayatana).
  **v1.5:** compact always-on-top mini-card. **v2 spike:** click-through
  overlay (WS_EX_TRANSPARENT/NOACTIVATE) — decide after v1 telemetry
  (does anyone actually alt-tab-pain-report?).
- **C7. Scan resilience:** remote-updatable scan config
  (https://tennoworth.app/definitions.json, ETag) carrying the
  accountId/nonce search patterns + known game-version quirks, and an
  in-app "scan broke" report flow (opens a prefilled GitHub issue — no
  telemetry backend). Note: our scanner pattern-searches for tokens, not
  struct offsets, so per-hotfix breakage is unlikely by design — this is
  cheap insurance, not the DeepSeek-feared weekly chase.
- **C8. Linux packaging:** AppImage first (self-contained, Proton users are
  used to it), AUR second, Flatpak deferred (ptrace sandbox friction).
  Document CAP_SYS_PTRACE per-update reality for the raw binary path; test
  matrix must include Steam/Proton (path-matching in find_wf_pid).
- **C9. Desktop assistant:** keep the DeepSeek chat opt-in, now calling
  wfm-core directly (no proxy route needed in-app); same disclosure copy.

## Phase D — cutover (after desktop beta)

Desktop app becomes the primary download everywhere; hosted site = landing +
market browser + trust/docs + file-drop; `serve` demoted to a documented
power-user flag; revisit whether hosted→loopback guidance is worth keeping
at all. Web-app semver continues; desktop app gets its own line
(tennoworth-desktop vX).

## WFM etiquette (cross-cutting)

Per-client budgets already exist (350 ms listing throttle, caller-owned rate
limiter in wfm-client) but are per-process; a popular desktop fleet is many
independent clients. v1: honor 429s with backoff + jittered schedules, keep
the listing batch cap, identify with a distinct User-Agent + contact URL so
WFM admins can reach us before they block us.

## Risk register (top 5)

1. **wfm-core extraction reveals hidden coupling** (login flow, scan
   concurrency) → timebox 2 wk; if it slips, ship Phase A/B value anyway —
   they're independent.
2. **Signed-but-unknown publisher still triggers SmartScreen** early →
   winget listing + install.ps1 education + accept the reputation ramp;
   measure with a clean-VM check each release.
3. **AV heuristics on a memory-reading exe** → CI VirusTotal watch, MS
   false-positive submissions, trust page transparency; never auto-elevate.
4. **Solo-maintainer overload** (Codex: 30–50 days is real) → phases are
   independently shippable; desktop can pause mid-way without stranding
   Phase A/B wins.
5. **DE stance shift on memory reading** → the tool degrades to file-drop +
   market browser (both keep working); trust page states this contingency.

## Rejected ideas (kept for the record)

- **EE.log inventory fallback** — factually impossible; EE.log has no
  inventory data.
- **Overlay-first v1** — highest-maintenance surface, wrong first bet for a
  solo maintainer; revisit v2.
- **Drop Linux desktop** — Linux is the wedge and the maintainer's platform.
- **Azure Trusted Signing** — EU individual developers ineligible (2026
  preview restriction).
- **Killing the LNA patch** — 0.5–1 d to keep the only working path alive
  during a 6–10 week build.
- **Companion-serves-SPA** — random-port origin strands all browser state;
  superseded by the desktop app.
- **Client-side market price history DB** — server market.json already
  ships 7d/90d aggregates; client stores only its own inventory/listing
  history.

## Effort summary (Codex-corrected)

| Phase | Engineering | Elapsed extra |
|---|---|---|
| A (LNA + landing + trust + cert start) | 6–10 d | cert validation 1–2 wk |
| B (wfm-core + signing pipeline) | 10–20 d | winget/AV review 1–3 wk |
| C (desktop v1) | 30–50 d | beta feedback |
| D (cutover) | 2–3 d | — |
