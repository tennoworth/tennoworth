// Relic expected value, across all four refinements.
//
// The planner used to answer "which of my relics is worth cracking" using the
// Intact table only, because that was all the old drop-table source gave us.
// DE's export gives the reward list first-party — but NOT the per-refinement
// odds: the four Bronze/Silver/Gold/Platinum variants carry identical reward
// lists, so refinement changes the odds and not the contents. Those odds are
// therefore ours (see REFINEMENT_CHANCE in the pipeline), and this module is
// where they turn into a decision.
//
// Everything here is per-crack expectation in a SOLO run. A four-person squad
// opens four relics and everyone takes the best drop, which changes the maths
// materially — so the caller must label which one it is showing rather than
// letting a reader assume.

import { clearingPrice, hasRealPrice } from './sell-priority';
import type { Market, MarketItemEntry } from './types';

export const REFINEMENTS = ['intact', 'exceptional', 'flawless', 'radiant'] as const;
export type Refinement = (typeof REFINEMENTS)[number];

/** Void traces each refinement costs. Fixed in-game values; they are the whole
 *  reason "radiant is higher EV" is not automatically "radiant is worth it". */
export const TRACE_COST: Record<Refinement, number> = {
  intact: 0,
  exceptional: 25,
  flawless: 50,
  radiant: 100,
};

/** 48h trades below which a reward's price is one optimistic ask, not a market.
 *  Kept equal to the planner's existing threshold so both agree. */
export const MOVING_THRESHOLD = 5;

/** A relic drop-table row as it appears in the snapshot. */
export interface RelicRewardRow {
  reward_slug: string;
  reward_name: string;
  rarity: string;
  chance: number;
  chances?: Partial<Record<Refinement, number>>;
  item_count?: number;
}

/**
 * Drop chance for one reward at one refinement.
 *
 * Older snapshots carry only the bare `chance` (which is the intact figure).
 * For those, intact is answerable and the other three are not — returning
 * `null` rather than reusing the intact number keeps a pre-2026-08 snapshot
 * from silently rendering four identical columns as if they were real.
 */
export function chanceAt(reward: RelicRewardRow, refinement: Refinement): number | null {
  const c = reward.chances?.[refinement];
  if (typeof c === 'number' && Number.isFinite(c)) return c;
  return refinement === 'intact' && Number.isFinite(reward.chance) ? reward.chance : null;
}

export interface RelicEv {
  refinement: Refinement;
  /** Expected plat from one solo crack. */
  ev: number;
  /** Void traces spent to get here. */
  traces: number;
  /** Plat gained over cracking it intact. */
  gainOverIntact: number;
  /** Plat gained per trace spent — the number that decides whether refining
   *  is worth it, since traces are the scarce input, not relics. */
  platPerTrace: number | null;
}

/** Expected plat for one solo crack at one refinement, or null when the
 *  snapshot cannot answer for that refinement. */
export function evAt(
  rewards: RelicRewardRow[],
  market: Market | null | undefined,
  refinement: Refinement,
): number | null {
  let ev = 0;
  let answered = false;
  for (const r of rewards) {
    const chance = chanceAt(r, refinement);
    if (chance == null) continue;
    answered = true;
    const entry: MarketItemEntry | undefined = market?.items?.[r.reward_slug];
    // An unlisted reward contributes nothing. Without this it would contribute
    // clearingPrice's 1p floor, quietly inflating every relic whose table is
    // full of parts nobody sells.
    const price = hasRealPrice(entry) ? clearingPrice(entry!) : 0;
    ev += (chance / 100) * price * (r.item_count ?? 1);
  }
  // `!(ev >= 0)` also rejects NaN, which a malformed chance could produce.
  if (!answered || !(ev >= 0)) return null;
  return ev;
}

/** EV at every refinement the snapshot can answer for, cheapest first. */
export function evLadder(
  rewards: RelicRewardRow[],
  market: Market | null | undefined,
): RelicEv[] {
  const intact = evAt(rewards, market, 'intact');
  const out: RelicEv[] = [];
  for (const refinement of REFINEMENTS) {
    const ev = evAt(rewards, market, refinement);
    if (ev == null) continue;
    const traces = TRACE_COST[refinement];
    const gain = intact == null ? 0 : ev - intact;
    out.push({
      refinement,
      ev,
      traces,
      gainOverIntact: gain,
      platPerTrace: traces > 0 ? gain / traces : null,
    });
  }
  return out;
}

export type RelicVerdict =
  | 'crack' //        cracking beats selling it intact
  | 'refine' //       cracking beats selling it AND refining pays for its traces
  | 'sell-intact' //  the relic itself is worth more than what falls out of it
  | 'thin' //         its rewards barely trade; the EV is a number, not a market
  | 'unknown'; //     the snapshot cannot price it

/** Plat-per-trace below which refining is not worth the traces. Traces cap at
 *  a few hundred and are farmed one relic at a time, so a refinement has to
 *  clear a real bar, not merely be positive. */
export const REFINE_WORTH_IT = 0.15;

export interface RelicDecision {
  ladder: RelicEv[];
  /** What to recommend: the highest-EV refinement whose extra plat pays for
   *  its traces, or intact when none of them do. */
  best: RelicEv | null;
  /** What the relic itself clears at, sold rather than cracked. */
  sellNow: number;
  /** Rewards trading at or above MOVING_THRESHOLD in the last 48h. */
  movingCount: number;
  totalRewards: number;
  verdict: RelicVerdict;
}

/**
 * Crack it, refine it, or sell it.
 *
 * The order of tests is the argument: a relic whose rewards do not trade gets
 * `thin` regardless of how good its EV looks, because the EV is computed from
 * asks nobody is hitting. Only then does the crack-versus-sell comparison run.
 */
export function decideRelic(
  rewards: RelicRewardRow[],
  relicEntry: MarketItemEntry | undefined,
  market: Market | null | undefined,
): RelicDecision {
  const ladder = evLadder(rewards, market);
  const sellNow = hasRealPrice(relicEntry) ? clearingPrice(relicEntry!) : 0;

  let movingCount = 0;
  for (const r of rewards) {
    if ((market?.items?.[r.reward_slug]?.vol ?? 0) >= MOVING_THRESHOLD) movingCount += 1;
  }

  const intact = ladder.find((l) => l.refinement === 'intact') ?? null;
  // Two-step, and the order matters.
  //
  // First: plat-per-trace decides WHETHER to refine at all. Traces are the
  // scarce input, not relics, so a refinement has to earn them.
  //
  // Then: EV decides WHICH rung, not plat-per-trace — because per trace the
  // rungs are nearly tied by construction. The rare slot goes 2 → 4 → 6 → 10%
  // for 0 → 25 → 50 → 100 traces, which is exactly 0.08% per trace at every
  // step; the only thing separating the rungs is how much value the shrinking
  // common slots were carrying. Ranking on a near-tie would make the
  // recommendation flip on rounding, so once refining is justified we take the
  // most plat.
  const worthRefining = ladder.filter(
    (l) => l.platPerTrace != null && l.platPerTrace >= REFINE_WORTH_IT,
  );
  const best = worthRefining.length
    ? worthRefining.reduce((a, b) => (b.ev > a.ev ? b : a))
    : intact;

  let verdict: RelicVerdict;
  if (!best) verdict = 'unknown';
  else if (movingCount === 0) verdict = 'thin';
  else if (sellNow > 0 && sellNow >= best.ev) verdict = 'sell-intact';
  else if (best.refinement !== 'intact') verdict = 'refine';
  else verdict = 'crack';

  return { ladder, best, sellNow, movingCount, totalRewards: rewards.length, verdict };
}
