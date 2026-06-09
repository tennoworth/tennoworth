# Wiki value opportunities

## Executive summary

Three features clear the "measurably better at what to sell right now" bar
and should ship first: **(1) Prime Vault status** (vaulted / soon-to-vault
/ unvaulted is the single largest trading signal in the game and
warframestat.us already exposes it via `vaultData`); **(2) Baro Ki'Teer
countdown + ducat-target alignment** (slots into the existing ducat
preset — a row that says "Baro arrives in 3d, you have 14k ducats" rebuilds
urgency from already-derived data); and **(3) Riven disposition column +
weapon-riven sub-table** seeded from the wiki's `Module:Weapons/data`
Lua module via Scribunto. These three are all small/medium builds against
sources that already cache cleanly into `market.json` and don't add new
runtime fetches in the browser.

After that, **trade-tax credit budget** and **augment-mod syndicate
provenance** are cheap wins; **lich/sister auctions** is a separate
subsurface worth scoping. Mastery-aware "this loses MR" is rejected
because it's factually wrong (see anti-features).

## Ranked features

### 1. Prime Vault status — vaulted / soon-to-vault / recently unvaulted

- **User value**: Vaulted primes spike in price 30-200% over their
  unvaulted floor; the cliff is visible weeks before it happens (DE
  announces Prime Resurgence rotations on a roughly 2-week cadence).
  A `Vaulted` / `Resurgence` / `Unvaulted` chip on each owned prime
  part directly answers "is this the moment to dump or hold?"
- **Persona**: Sasha (heavy trader) primarily; Jamal (casual flipper)
  secondarily — Mira (Linux power user) doesn't care, she's farming.
- **Data source**: `https://api.warframestat.us/pc/vaultData` (CORS
  ok, no auth) + `https://api.warframestat.us/pc/voidTrader` for the
  next Baro window. The wiki's `Prime Vault` page (visited) lists
  recent vault/unvault dates but isn't structured — warframestat
  derives those programmatically already.
- **Build**: medium. Extend `scripts/csv_to_market_json.py` to fetch
  `vaultData`, map prime-name → status, bake `market.json.vault_status:
  {slug: "vaulted"|"resurgence"|"available"}`. New column / chip in
  the sell table; new "vaulted" preset pill.
- **Risk**: warframestat's `vaultData` shape isn't formally documented
  on the live docs site (returned 403 to WebFetch, but the endpoint
  exists per community references); will need a one-time probe to
  confirm shape. Drift: low — DE's Prime Resurgence cadence is stable.
- **Hook into existing surfaces**: new column in `ResultsTable.svelte`
  (badge: `Vaulted` red, `Resurgence` amber, blank for available),
  new preset pill "Vaulted only", and a "Vault risk" reco kind in
  `set-recos.js` ("3 of 4 parts vaulted — your set is on borrowed
  time").

### 2. Baro Ki'Teer countdown + ducat-target card

- **User value**: The existing `p/100d` ratio and "deal" badge already
  surface ducat-good trades, but the urgency is implicit. A small
  card reading **"Baro arrives in 2d 14h at Larunda Relay — you have
  14,250 ducats from 47 sellable items"** turns the ducat preset
  from "interesting" into "act now". When Baro is *here*, swap the
  copy to "selling now".
- **Persona**: Jamal first (he doesn't track Baro himself), Sasha
  second (she does, but appreciates the join).
- **Data source**: `https://api.warframestat.us/pc/voidTrader` — has
  `activation`, `expiry`, `location`, and `inventory[]` (item + ducat
  + credit cost) per visit. CORS-friendly, free, real-time.
- **Build**: small. One fetch in `App.svelte` `onMount`, a derived
  ducat-sum from `resolved.owned` × `market.items[slug].ducats`, and
  a single card above the table. Show Baro's *current inventory*
  when he's surfaced so the user knows what to spend ducats on.
- **Risk**: warframestat.us availability — we'd want to soft-fail
  (hide the card) on fetch error rather than blocking the dashboard.
  Drift: low.
- **Hook into existing surfaces**: peer card sibling to the relic
  planner, only visible when the user has ≥ 500 ducats of sellable
  items. Reinforces the Ducats preset, doesn't replace it.

### 3. Riven disposition column + weapon-riven sub-board

- **User value**: A 1.55 disposition makes a riven 3× more valuable
  than a 0.5 disposition for the same stats. Users selling rivens
  (companion supports riven listings via WFM) need to see disposition
  inline. Users *buying* rivens need to compare against the asking
  price's disposition tier.
- **Persona**: Sasha primarily — riven trading is endgame. Useful to
  Mira if she has companion-fetched rivens.
- **Data source**: wiki `Module:Weapons/data` (Lua) → exposed as JSON
  via `https://wiki.warframe.com/api.php?action=scribunto-console`
  or scraped weekly from `/w/Riven_Mods/Weapon_Dispos/All`. Updated
  ~quarterly. Alternative: `omegaAttenuation` field in DE's
  `ExportWeapons_en.json` from `content.warframe.com/PublicExport/`
  is the canonical authoritative source — same numbers, no community
  layer.
- **Build**: medium. New `scripts/fetch_dispositions.py` baking
  `market.json.dispositions: {weapon_slug: float}`. Browser shows
  the multiplier next to riven items and tints the disposition chip
  (≥1.3 green, ≤0.7 red).
- **Risk**: DE's PublicExport is LZMA-compressed and index-keyed —
  a one-time decompression step in Python. Drift: dispositions
  change every ~3 months; the scrape needs to be re-run after each
  rebalance. Cron-acceptable.
- **Hook into existing surfaces**: new `Disposition` column visible
  only when the row's category is `Riven Mod`; new "Rivens" preset
  pill that filters to riven-bearing weapons sorted by disposition.

### 4. Trade-tax credit budget per session

- **User value**: A heavy trader running 30 trades in a day can spend
  240k credits in tax (8k per rare). The dashboard already knows
  per-row rarity (`m.tags[]` includes `rare`/`uncommon`). Summing
  credit cost across the current sell plan gives a "this session
  will cost you 184,000 credits in trade tax" line. Sasha can plan
  around it.
- **Persona**: Sasha. Casual flippers don't hit the credit wall.
- **Data source**: Trade tax table is on the wiki
  (`https://wiki.warframe.com/w/Trading`, visited — 2k common /
  4k uncommon / 8k rare / 1m legendary, +500 per plat). Static
  constants, no fetch needed.
- **Build**: small. ~30 LOC: derive `credit_cost_total` from selected
  rows and stamp it on the sell-plan summary.
- **Risk**: none — values are stable; arcanes have tiered costs
  100k–2.1M that we'd hard-code or skip.
- **Hook into existing surfaces**: footer of the main results table
  alongside the existing per-trade summary; an in-line warning chip
  if total tax > 1M credits.

### 5. Augment mod syndicate provenance + standing cost

- **User value**: Augments are tradeable but require 25k syndicate
  standing to acquire. "You're listing Repelling Bastille for 35p
  — to re-buy you'd need 25k Cephalon Suda standing (3-5 day farm)"
  changes the sell decision. The warframestat.us `Mods.json` field
  `isAugment` lets us flag the rows; `drops[0].location` carries the
  syndicate name.
- **Persona**: Jamal — he's most likely to regret-sell an augment.
- **Data source**: `https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/Mods.json`
  (already in our resolver lineage). Standing costs are uniform
  (25k standard, 35k Conclave) — wiki page `Augment_Mods` confirms
  the value but doesn't expose a clean structured table; constants
  acceptable.
- **Build**: small-medium. Extend `csv_to_market_json.py` to walk
  Mods.json once and emit `market.json.augments: {slug:
  {syndicate, standing_cost}}`. Browser adds a `(syndicate)` tag
  and a "to re-buy: 25k standing" tooltip.
- **Risk**: WFCD's `warframe-items` repo is reliably updated but
  not contractually stable. Drift: low.
- **Hook into existing surfaces**: small badge next to the item
  name in `ResultsTable.svelte`; new "Augments" preset pill.

### 6. Drop-source one-liner per sellable item

- **User value**: Powers the "instead of listing, farm one yourself"
  inversion the brief calls out, but framed differently: if the
  *buyer* could trivially farm it (Hieracon B, 23% rotation C), the
  *seller* should expect downward price pressure. Show the easiest
  drop source as a subtitle: `Hieracon B · 22.6% · Rotation C`.
- **Persona**: Jamal (knows farming routes less); Sasha (uses it as
  price-floor intuition).
- **Data source**: `https://drops.warframestat.us/data/all.json`
  already partially consumed by the relic planner. The full table
  has drop locations for every part. ~20 MB, cache once.
- **Build**: medium. Extend the relic-rewards bake to also produce
  `market.json.easiest_drop: {slug: {location, chance, rotation}}`
  taking the max-chance entry. Browser renders a subtitle row in
  expanded view.
- **Risk**: drop table size — bake-time only, no browser cost. Drift:
  low; DE patches drop tables only on prime releases.
- **Hook into existing surfaces**: subtitle under item name in the
  results table when the row is expanded (existing detail-view).

### 7. Lich / Sister auction sub-board

- **User value**: Entirely separate WFM market (`/v1/auctions/search?type=lich`)
  with its own pricing dynamics (ephemera, damage roll, element).
  Users with a freshly-killed lich don't know where to price it.
  A read-only "your lich would list at ~180p (median across 14
  comparable listings)" card answers the question.
- **Persona**: Sasha. Jamal doesn't grind liches.
- **Data source**: `https://api.warframe.market/v1/auctions/search?type=lich`
  (CORS-blocked from browser per the project's first rule, so this
  must be a `wfm_demand.py`-style background scrape that bakes a
  digest into `market.json` — the auctions endpoint *does* respond
  to server-side calls). Fields: weapon, element, damage, ephemera,
  buyout_price.
- **Build**: large. Separate scrape pipeline + separate UI surface
  (the existing table is part-shaped, not auction-shaped). Worth
  scoping as Phase 2 only if user has any lich/sister holdings to
  surface.
- **Risk**: WFM has historically deprecated v1 routes; the auctions
  endpoint may move to v2 with breaking changes. Plan for it.
- **Hook into existing surfaces**: separate route/card, not a column.
  Companion would need a "lich auction" listing flow eventually.

### 8. Quest-locked / event-exclusive irreplaceable warning

- **User value**: Some tradeable mods (Operative Standoff, Train
  Surfer, event-exclusive Nightwave items) are *technically*
  tradeable but the user can't re-acquire them without waiting for
  a rerun. "You won't easily replace this" is a sell-decision input.
- **Persona**: Jamal (more likely to regret-sell); Mira (collects
  irreplaceables).
- **Data source**: WFCD `warframe-items` Mods.json — items missing a
  `drops` array entirely, or with drop locations matching
  `/event|nightwave|operation/i`, are the candidates. Wiki has a
  `Discontinued` category but no clean structured export.
- **Build**: small. Bake a `market.json.irreplaceable: {slug: reason}`
  flag during the existing parent-walk in `csv_to_market_json.py`.
  UI shows a single icon + tooltip.
- **Risk**: false positives — the "no drops" heuristic catches some
  items that drop from sources warframe-items doesn't index. Want
  a manual override list. Drift: low.
- **Hook into existing surfaces**: lock icon next to item name,
  tooltip on hover. No new card.

## Anti-features (proposed and rejected)

### Mastery-loss warning on sell

The wiki is explicit (visited `Mastery_Rank`): mastery points are
awarded at rank-up and remain credited *forever* after sale. Selling
a Rank 30 weapon then re-buying it does **not** re-award MR.
"You'll lose MR if you sell this" is factually wrong and would
mislead new players. Reject.

### Generic wiki-info integration ("show wiki blurb in row")

Hover-card with the wiki's flavor text is the prototypical
"AI-generated feature that adds nothing actionable". Users selling
items already know what they are. Rejected as design noise that
doesn't move the "what to sell right now" needle.

### Eidolon / Profit-Taker / Orphix arcane farming routes

Arcanes already appear in our market view; surfacing optimal Eidolon
hunts is an *acquisition* feature, not a sell-side feature. Out of
scope for an inventory dashboard. The data exists
(`api.warframestat.us/pc/cetusCycle` + drop tables) but it belongs
in a farming companion, not here.

### Real-time Cetus / Solaris / Necralisk standing tracker

`https://api.warframestat.us/pc/cetusCycle` and friends are easy
data, but the dashboard doesn't track player standing — it tracks
sellable items. The tradeable items from those syndicates (rare
arcanes, mods) are *already covered* by the augment / arcane work.
The day/night cycle itself is noise.

### Conclave augment sub-pricing

Conclave is a niche game mode; the augment-mod feature already
indexes augments. Splitting Conclave augments into their own surface
optimises for ≤ 1% of inventories.

## Open questions for the user

1. **Riven sub-board scope** — do you want disposition as a single
   column on the main table (small) or a dedicated riven workbench
   that mirrors the WFM auction filters (large, Phase 2)?
2. **PublicExport vs. WFCD warframe-items as canonical metadata
   source** — DE's PublicExport is authoritative but LZMA-indexed
   and undocumented; WFCD is friendlier but a community layer.
   Preference? (Current code mixes both implicitly.)
3. **Anti-feature gut-check on lich auctions** — is the lich sub-board
   worth the separate scrape pipeline given how few inventories have
   live liches, or should it wait until the companion-fetch returns
   a non-empty lich/sister list for the user?
