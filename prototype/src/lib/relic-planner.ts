// Relic planner — answers "which of MY relics should I crack tonight?".
//
// For each owned relic, computes expected-plat-per-crack (EPP) using
// drop-weighted reward prices from market.relic_rewards. v1 uses the
// Intact-state drop table only — that's the lowest-effort refinement
// and matches the "I have these now, what next?" workflow.
//
// EPP = Σ (chance / 100) × clearingPrice(reward)
//
// clearingPrice, not raw low_sell: a single aspirational ask on one reward
// inflates the whole relic's EV (ballistica prime string's lone 40p ask
// over a 15p median read a Meso G1 crack ~50% rich). Same clamp the Sell
// score uses — the EV should describe what the parts actually clear at.
//
// Volume signal: count of rewards with vol_48h ≥ MOVING_THRESHOLD —
// helps the user spot a high-EPP-but-dead-rewards trap (the relic's
// expected drop chart is gold, but nobody's actually buying its parts).
//
// Returns the user's top-N relics by EPP, owned-count tied in for
// total-plat-if-I-crack-them-all context. sell_now carries the relic's own
// clearing price so the card can show "crack vs sell the relic itself".

import type { Market, OwnedRecord } from './types';
import { clearingPrice } from './sell-priority';

const MOVING_THRESHOLD = 5;  // 48h trades; below this the part stagnates

export interface RelicPlanReward {
  slug: string;
  name: string;
  rarity: string;
  chance: number;
  low_sell: number;
  vol_48h: number;
}

export interface RelicPlanEntry {
  relic_slug: string;
  relic_name: string;
  owned: number;
  epp: number;
  epp_owned: number;
  /** What the relic itself clears at if sold intact instead of cracked. */
  sell_now: number;
  moving_count: number;
  total_rewards: number;
  rewards: RelicPlanReward[];
}

export function deriveRelicPlan(
  owned: Map<string, OwnedRecord> | null | undefined,
  market: Market | null | undefined,
  limit = 3,
): RelicPlanEntry[] {
  const rewards = market?.relic_rewards;
  if (!owned || !rewards) return [];

  const candidates: RelicPlanEntry[] = [];
  for (const rec of owned.values()) {
    // Intact only for v1. If you own only refined copies of this relic
    // we skip — refining is a separate decision the planner doesn't
    // make for you.
    if (rec.subtype !== 'intact') continue;
    const dropTable = rewards[rec.slug];
    if (!Array.isArray(dropTable) || dropTable.length === 0) continue;

    let epp = 0;
    let movingCount = 0;
    const rewardRows: RelicPlanReward[] = [];
    for (const r of dropTable) {
      const me = market.items?.[r.reward_slug];
      const price = me ? clearingPrice(me) : 0;
      const vol = me?.vol || 0;
      epp += (r.chance / 100) * price;
      if (vol >= MOVING_THRESHOLD) movingCount += 1;
      rewardRows.push({
        slug: r.reward_slug,
        name: r.reward_name,
        rarity: r.rarity,
        chance: r.chance,
        low_sell: price,
        vol_48h: vol,
      });
    }
    // `!(epp > 0)` also rejects NaN — if a drop entry's chance is
    // malformed (string, null, missing) and slipped past the scraper's
    // `float() or 0` guard, `epp` could become NaN and produce a card
    // full of NaN%/NaNp values. `epp <= 0` is false for NaN, so use the
    // affirmative form.
    if (!(epp > 0)) continue;

    const relicEntry = market.items?.[rec.slug];
    candidates.push({
      relic_slug: rec.slug,
      relic_name: rec.name,
      owned: rec.count,
      epp,                                    // plat per crack (single)
      epp_owned: epp * rec.count,             // if you cracked them all
      sell_now: relicEntry ? clearingPrice(relicEntry) : 0,
      moving_count: movingCount,
      total_rewards: dropTable.length,
      rewards: rewardRows.sort((a, b) => b.chance - a.chance),
    });
  }

  candidates.sort((a, b) => b.epp - a.epp);
  return candidates.slice(0, limit);
}
