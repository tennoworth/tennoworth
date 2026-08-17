# Plan — long history, hold/sell advisor, rivens, and the 0.3.7 release (2026-08-17)

Continues from the 2026-08-16 idea board (35 ideas, artifact) and what shipped
that day on `develop`: WFM ToS compliance, Spares preset, live prices in the
listing review, Listing Health, riven disposition watcher, price watches, and
EE.log trade detection → ledger + auto-close. This document is the execution
plan for the next tranche, in order. Each phase is independently shippable.

Ground rules that carried over and still bind:

- WFM rules: descriptive UA (done), 3 req/s, 10/min on auctions, app name in
  any generated whisper, **contact WFM staff before going public** (§P4).
- No new dependency in the player binary without a reason written down
  (`chrono` is deliberately host-only).
- Every pure decision has a unit test; every panel has a component test;
  every WFM write goes through the same call as a manual edit.
- The box is the only place with persistent pipeline state; the GitHub cron is
  stateless and must never bootstrap a multi-hundred-MB fetch.

---

## P1 — Long price history from relics.run (pipeline, box only)

**Why.** WFM's `/statistics` stops at 90 days. relics.run publishes one file
per day since 2021-09-12 (`https://relics.run/history/price_history_YYYY-MM-DD.json`,
~3.9 MB, keyed by WFM display name, rows = WFM's own per-(rank, subtype)
`closed`/`sell`/`buy` daily statistics — probed 2026-08-17). With it we can
answer "what does this item do around Baro / Prime Access / vaulting" — the
foundation of the hold/sell advisor (P2) and the honest Δ1y everyone else
lacks.

**Shape.** A new box-only artifact `prototype/public/history.json` (served by
the existing `@livedata` Caddy block, gzip), NOT folded into market.json
(size). Format, keyed by WFM slug:

```
{
  "generated_at": "2026-08-17T06:00:00Z",
  "start": "2025-08-17",            // day 0 of every series
  "days": 365,
  "items": {
    "primed_flow": { "median": [20, 20, null, …365], "volume": [86, 90, 0, …] }
  }
}
```

- One tier per item, chosen exactly as the scraper does today: `rank0_rows`
  → `canonical_subtype` → `drop_poisoned_rows` (reuse `market-math`; the
  history builder lives in `wfm-scrape` so nothing is re-implemented).
- `null` median = no closed trades that day. Arrays, not objects: 2,700 items
  × 365 × 2 ≈ 2 M numbers ≈ 6 MB raw, ~1 MB gzipped.
- Display name → slug via the WFM catalog the build already holds
  (`catalog: {name_lower: slug}`); unmatched names are dropped and counted.

**Incremental state.** `wfm-scrape build` gains a `history` step with its own
state file `<state>/history.json` (same directory as `prior-market.json`):
on each run fetch only the days after the last stored day (normally one file;
relics.run publishes yesterday's file early UTC), append, trim to 365, write.
Bootstrap = up to 365 fetches ≈ 1.4 GB, once, on the box; paced 1 req/s
(relics.run states no limit; be polite — it's a hobbyist mirror). Fetch
failure of any day = keep going, note the gap (`null` day), warn; never abort
the market build over history.

**CLI / ops.**
- `wfm-scrape build --history <state-path>` enables the step; the GitHub cron
  does not pass it (stateless runner). `deploy/run-scrape.sh` passes it on the
  box. Document in deploy/README.md ("Live-site data stale?" gets a history
  entry).
- `TimeoutStartSec` stays; the bootstrap is a one-off manual run
  (`wfm-scrape history --bootstrap 365`), not the timer's job.

**Consumers (this phase).**
- SPA `Market` type gains nothing; a new lazy loader `loadHistory()` fetches
  `history.json` on demand (desktop: via the existing market-refresh path so
  egress stays in Rust — a `history.json` cache next to `market.json`, same
  ETag logic; hosted site: plain fetch).
- Search rows and the item detail get a **1-year sparkline** and **Δ1y**;
  Trending gains a `1y` toggle. That is the visible win of P1 on its own.

**Tests.** history builder: append/trim/gap/dedupe by date; tier selection
parity with the scraper on a fixture day; name→slug drop counting. SPA:
loader + Δ1y helper.

**Effort.** ~1 day pipeline + ½ day SPA. **Moat 2.**

---

## P2 — Hold-or-sell advisor (desktop, needs P1)

**Why.** The "relic gremlins of '25" play — hoard vaulted sets, sell months
after the vault — is manual folklore. We hold the three inputs nobody joins:
your scanned inventory, the calendars, and (after P1) a year of prices.

**Calendars (pipeline surface `calendar` in market.json).**
- Prime Access history + upcoming, and Prime Resurgence 28-day rotations:
  wiki `Prime_Access` / `Prime_Resurgence` tables via the MediaWiki API
  (CC BY-SA — attribution line in the FAQ). Parse `Module:Void/data` for
  per-relic vault status (already have `vault_status` per item from
  warframestat; keep that, add the DATES).
- Baro: our own `baro` surface + captured inventories (from 2026-08-21).
- Recorded as `{prime_access:[{name, released, vaulted_items:[slug]}],
  resurgence:[{from,to,items:[slug]}], baro_visits:[{from,to,items:[slug]}]}`.

**Signals per owned set/part (pure, in `market-math` so the tray and SPA
agree):**
- `days_since_vault`, `days_to_next_pa` (estimate from cadence when unknown),
  `resurgence_active`, `baro_recovery` (median now vs median 30 d before the
  last visit that carried the item).
- From history: `median_now / median_90d_before_vault`,
  `median_now / median_1y_high`, slope over 30 d.
- Verdict enum: `sell_now` (at/near 1y high, or resurgence active and price
  falling), `hold` (recently vaulted, below the typical post-vault ramp),
  `neutral`. Every verdict carries the two or three numbers that produced it —
  shown, not hidden.

**UI.** Sell view: a `Hold / Sell` preset (`typesAny` = sets + parts) with a
column `advice` (chip) and the reason on hover; the Sets view gets the same
chip per set. **No automation** — advice only.

**Tests.** Verdicts on synthetic series (post-vault ramp, PA-day crash,
Resurgence flood); calendar parser on a captured wiki fixture.

**Effort.** ~2 days. **Moat 3.**

---

## P3 — Riven appraiser with comps (desktop)

**Why.** Top plat method in every 2026 guide; AlecaFrame's paid hook; single-
number tools got roasted for being wrong. We show the comparables.

**Data.**
- DE weekly: `https://www-static.warframe.com/repos/weeklyRivensPC.json`
  (Mondays 00:00Z; a JS object literal, not strict JSON — a small relaxed
  parser: unquoted keys, single quotes). Per weapon × rerolled: avg / median /
  min / max / stddev / pop. Pipeline surface `riven_stats` in market.json
  (~2 k rows, small).
- WFM v1 `/auctions/search?type=riven&weapon_url_name=…&sort_by=price_asc`
  on demand from the desktop, 10/min cap → a per-weapon "comps" panel with
  the ≤20 cheapest matching auctions (attributes, rerolls, MR, buyout).
- WFM v2 `/riven/attributes` manifest for attribute names/units (already
  fetched by the disposition watcher's neighbour; extend `fetch_rivens`).

**Owned rivens.** Parse `Upgrades[]` entries under
`/Lotus/Upgrades/Mods/Randomized/*` — fingerprint JSON carries `compat`
(weapon path), `buffs`/`curses` (tag + value), `rerolls`, `lvl`, `pol`. Map
`compat` → weapon slug via the WFM riven weapons manifest `gameRef` (already
in the payload). Show a **Rivens** view: your rivens, DE weekly band for the
weapon (rolled/unrolled), disposition + recent change (from `rivens.changes`),
and a "Show comps" button per riven.

**Explicitly not doing:** a "this riven is worth N" number. The band + comps
+ dispo is the product; a grade would be the morrowshore mistake.

**Effort.** ~3 days. **Moat 2.**

---

## P4 — Going public properly

- **WFM staff contact** (user action; draft below).
- Whisper text: when P-later buy-order matching generates a message it MUST
  carry "via TennoWorth" — recorded here so it isn't forgotten.
- README + FAQ: ban-risk section adds EE.log (read-only, the game's own log)
  and the price-watch cadence (10 min, ≤100 items, ~3 req/s bursts).

Draft (send from the project's account to WFM's contact / Discord #api):

> Hi — I maintain TennoWorth (https://tennoworth.app, MIT,
> github.com/tennoworth/tennoworth), a Windows/Linux desktop companion that
> reads a player's inventory locally and helps them list and manage orders on
> warframe.market. Per your API rules I'm writing before wider release.
> UA: `TennoWorth/<ver> (tennoworth-desktop; +https://tennoworth.app; <issues>)`.
> Traffic: one 2-hourly catalogue-wide statistics scrape (≈6.5k requests at
> ≤3 req/s from one host), and per-user on-demand `orders/item/{slug}/top`
> lookups (≤100 items, 3 req/s) plus a 10-minute price-watch pass (≤100
> items). Order writes are user-initiated only; the app never auto-bids or
> undercuts. Login uses v1 `/auth/signin` since v2 is first-party-only.
> Happy to adjust anything.

---

## P5 — 0.3.7 release checklist (user drives the tag)

1. Promote `develop → main` (fast-forward). CI on `develop` is green through
   2a0d474 (audit + Windows ui-smoke, which runs the real app with the v3/v4
   migrations and both background threads).
2. One real desktop session before tagging: scan → Spares preset → listing
   review "Check live prices" → My orders "Check live" → add a price watch →
   complete a trade in-game and watch the Ledger. Note anything off.
3. `git tag desktop-v0.3.7 && git push --tags` → release-desktop.yml.
4. Release notes: the seven features + the Windows >64 MB scan fix.

---

## Not planned (decided)

- `/tools/ducats` card — the Ducats preset already computes plat/100 ducats.
- In-game overlays — crowded (WFInfo-Linux, WFHelper, wfcli), trust cost.
- Auto-bidding / undercut automation — the grievance we position against.
