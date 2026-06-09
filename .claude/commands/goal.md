---
description: Ship the remaining four features from .mockups/next-features.html — one focused chunk per fire, stop when all four are live and verified.
---

# Goal

Ship the four still-unimplemented features designed in
`/home/prowly/Desktop/Warframe market check/.mockups/next-features.html`,
**in this priority order**:

1. **Tag-chip filter row** — toggleable chips ANDed with existing
   filters. Multi-select. Strikethrough zero-match chips. Data already
   in `m.tags` per slug.
2. **Δ-vs-90d badge + sparkline column** — per-row ▲/▼/· badge with
   `%` vs `median_90d`; 60×18 inline `<svg>` polyline from `medians_7d`.
   Up = good (rising 90d median = sell into a peak). Data already in
   `m.median_90d` / `m.medians_7d`.
3. **Set-completion card** — peer card above the table. Group prime
   parts by set, surface three reco kinds (near-complete / extras /
   duplicate sets). Cap at 4 visible rows then "more…" disclosure.
   Needs a new derived `set_to_parts` map in `market.json` — derive
   from the same warframestat parent walk that produces `path_to_info`.
4. **Relic planner card** — three-card grid above the table. EPP
   (expected plat per crack, drop-weighted) headline; volume signal
   ("4/6 rewards moving"); three to four entries. Needs relic→reward
   drop tables in `market.json.relic_rewards` from
   `https://drops.warframestat.us/data/relics.json`.

The goal is reached when **all four are shipped and verified
end-to-end** in a real browser via Playwright with the project's
`inventory.json`, AND every test suite is green.

## What each fire of this command does

1. Read `CLAUDE.md`, `prototype/CLAUDE.md`, `companion/CLAUDE.md`, the
   relevant component files, AND the loop journal at
   `/home/prowly/Desktop/Warframe market check/.loop-journal.md`.
   Identify what's *already* done by inspection — don't re-ship, don't
   re-walk paths the journal says are dead-ends.
2. Pick the next unfinished feature from the priority list.
3. Ship the smallest end-to-end slice (data layer, then UI, then test).
4. Run the full sweep:
   - `cd prototype && bun run test`
   - `cd prototype && npx svelte-check`
   - `cd prototype && bun run build`
   - `/home/prowly/.local/bin/pytest /home/prowly/Desktop/Warframe\ market\ check/tests/ -q`
   - `cd companion && cargo test --release --quiet` if Rust changed
   If anything is red, fix before claiming done.
5. Spawn a dev server in the background, drive Playwright, load
   `/home/prowly/Desktop/Warframe market check/inventory.json`, and
   visually verify the new feature. Stop the dev server and close the
   browser before yielding.
6. Update the relevant `CLAUDE.md` if the change introduced new
   architectural rules (storage key bumps, new market.json fields,
   new component conventions).
7. **Append a journal entry** to
   `/home/prowly/Desktop/Warframe market check/.loop-journal.md` —
   timestamp, which feature was attempted, what shipped, what didn't,
   what the next fire should start with. See "Journal format" below.
8. End the turn — let the next fire pick up where this one left off.

## When stuck — argue before pushing through

If you hit any of these conditions, you are STUCK and must NOT proceed
on confidence alone:

- A test failure you've tried to fix once and it still fails.
- An architectural question with multiple defensible answers
  (e.g. "do we store this in market.json or compute in the browser?").
- A WFM/warframestat API response that doesn't match your mental
  model.
- A UI design decision the mockup doesn't already settle.

When stuck:

1. **Summon an adversarial agent** via the Agent tool with
   `subagent_type: general-purpose`. Brief it with:
   - The concrete decision you're about to make.
   - The reason you think it's right.
   - "Argue the opposite. Find the case I'm missing. Be specific —
     code lines, edge cases, failure modes."
2. **Read its report.** If the adversary's argument has merit you
   hadn't considered, adjust your plan.
3. **Iterate up to twice.** If after two rounds you and the adversary
   are still divergent, write a journal entry stating the unresolved
   dispute, yield, and let the next fire (or the user) break the tie.
4. Only proceed when **both your confidence and the adversary's
   acknowledgement** are high. "High" means: the adversary explicitly
   says it can't find further objections, OR the remaining objections
   are tradeoffs you've consciously accepted.

The journal must record each adversarial round — verdict, what
changed, what didn't — so the user can review the reasoning when they
return.

## Journal format

`.loop-journal.md` is append-only. Each entry:

```
## YYYY-MM-DD HH:MM:SSZ — Fire N — <feature name or "blocker"> — <SHIPPED|YIELDED|BLOCKED>

**Did:** one-paragraph summary of what landed (files touched, tests
added, mockup section satisfied).

**Verified:** how (test counts, Playwright probe result, manual
observation). Cite numbers.

**Decisions:** any defaults applied or questions resolved. Note
adversarial-agent rounds inline as "Argued: ..." sub-bullets.

**Next fire starts with:** specific next action — file, function,
feature.

**Open:** anything yielded without resolving (and why).
```

Read previous entries before doing anything. They are the source of
truth on what's done and what was tried.

## When all four are shipped

Output a single line:
`GOAL REACHED — <N> tests pass, 4/4 features shipped.`
Do not schedule another fire.

## Defaults — don't ask the user, just pick

- **Δ-vs-90d direction**: up = good (price rising vs 90d median means
  it's a good time to sell).
- **Set-completion placement**: peer card above the table.
- **Set-completion cap**: 4 visible rows then a `<details>` "more…"
  disclosure.
- **Tag-chip coverage**: only show chips for tags that genuinely
  appear in the live `m.tags` arrays. Don't synthesise from
  `category`.
- **Relic planner data source**: precomputed in
  `scripts/csv_to_market_json.py` (mirror the path_to_info pattern).
  Bake `market.json.relic_rewards: {relic_slug: [{reward_slug, rarity,
  drop_chance_pct}]}`. If the source endpoint is unreachable on a
  given fire, leave the field empty and render an empty-state
  placeholder card — don't block on it.
- **Sparkline empty data**: when `medians_7d` is empty (CSV-only
  rebuilds inherit zeros until the next full scrape), render `—` in
  the column, not a flat line.

## Hard rules (already documented; restated so the loop can't drift)

- Edit existing files in preference to new ones.
- No comments that restate the code.
- Match the existing colour palette in `app.css`; introducing a new
  token requires the same justification `--ducat` got (a domain
  signal, not decoration).
- Verify in the actual runtime, not just type-check. Playwright is
  the source of truth for UI claims.
- Don't reinvent shipped behaviour — read `App.svelte` first.
- If you bump `market.json` shape, rebuild it once via
  `python3 scripts/csv_to_market_json.py` before the browser test.

## Files of record

- Mockup: `/home/prowly/Desktop/Warframe market check/.mockups/next-features.html`
- Root: `/home/prowly/Desktop/Warframe market check/CLAUDE.md`
- Svelte: `/home/prowly/Desktop/Warframe market check/prototype/CLAUDE.md`
- Companion: `/home/prowly/Desktop/Warframe market check/companion/CLAUDE.md`
- Main app:
  - `/home/prowly/Desktop/Warframe market check/prototype/src/App.svelte`
  - `/home/prowly/Desktop/Warframe market check/prototype/src/components/ResultsTable.svelte`
- Backend:
  - `/home/prowly/Desktop/Warframe market check/scripts/csv_to_market_json.py`
  - `/home/prowly/Desktop/Warframe market check/wfm_demand.py`
- Test inventory: `/home/prowly/Desktop/Warframe market check/inventory.json`

## Stop conditions (any one ends the loop)

1. All four features are shipped and verified (the normal exit).
2. A test failure can't be resolved in this fire — yield with a clear
   note to the user describing what failed and what's needed.
3. The drop-table source for the relic planner is unreachable AND
   features 1–3 are already done — yield with a clear note; the user
   can decide whether to source the data manually.
