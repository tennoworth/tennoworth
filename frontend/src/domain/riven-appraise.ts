// Riven appraisal, and the reroll decision.
//
// WHAT THIS DELIBERATELY DOES NOT DO: guess what a riven is worth from its
// stats. That needs per-stat roll ranges and a model of which stats buyers
// want, and we have neither - DE publishes dispositions, not roll ranges. It
// is why no riven price checker is trusted.
//
// WHAT IT ALSO NO LONGER DOES: fit a normal distribution to DE's summary
// statistics. The first version of this module did, and it was wrong in the
// way that matters. Riven prices are bounded below at zero and heavily
// right-skewed - a weapon with 90 sales at 10p and 10 at 1,000p has a mean of
// 109 and a standard deviation of 297 - and a normal fit to those two numbers
// reported a 10p offer as the 37th percentile with a 63% chance that a reroll
// beats it, and priced the losing outcomes at MINUS 194p. Every one of those
// numbers is impossible or backwards. Five summary statistics do not identify
// a distribution this skewed, and no parametric fit rescues that.
//
// So this module reports only what those five numbers actually support: where
// an offer sits against the landmarks DE gives us (min, median, average, max),
// how skewed the market is, and what rerolling costs. The price still comes
// from the USER - an offer they received, or their own estimate.

import type { RivenAuction } from '../contracts/desktop';
import type { RivenStatTier } from '../contracts/data';

/**
 * DE's `pop` is a relative popularity score on a 0-100 scale, not a count of
 * trades: each week exactly one row is 100, and rows like AX-52 rolled
 * (pop 5, max 3,800p, average 182p) could not exist if it counted sales. It
 * says a band is thin, not how thin. The old gate read it as a count and hid
 * any band under 12, which on the 2026-10-08 PC feed was 98% of them. At or
 * under this score (the floor, held by over half the rows) the band is shown
 * with a warning instead, because no count exists to gate on. The threshold is
 * a product judgement.
 */
export const LOW_POPULARITY = 1;

/**
 * Kuva per reroll. Climbs with the reroll count and caps.
 *
 * In-game values, and the reason "just reroll it" is not free advice: a riven
 * at cap costs 3,500 kuva a spin.
 */
export const REROLL_KUVA = [900, 1000, 1200, 1400, 1700, 2000, 2350, 2750, 3150, 3500] as const;

/** Update 44.0: cycling with a manually locked trait costs double. A
 *  spliced trait is also fixed in place but does not double the cost. */
export const LOCKED_REROLL_FACTOR = 2;

export function rerollCost(rerolls: number, locked = false): number {
  const i = Math.max(0, Math.floor(rerolls));
  const base = REROLL_KUVA[Math.min(i, REROLL_KUVA.length - 1)];
  return locked ? base * LOCKED_REROLL_FACTOR : base;
}

export interface Distribution {
  median: number;
  avg: number;
  min: number;
  max: number;
  stddev: number;
  /** DE's 0-100 relative popularity - see `LOW_POPULARITY`. */
  pop: number;
}

/** A usable distribution, or null when DE published no price for the tier. */
export function distributionOf(tier: RivenStatTier | null | undefined): Distribution | null {
  if (!tier) return null;
  const pop = Number(tier.pop) || 0;
  const median = Number(tier.median) || 0;
  if (median <= 0) return null;
  return {
    median,
    avg: Number(tier.avg) || 0,
    min: Number(tier.min) || 0,
    max: Number(tier.max) || 0,
    stddev: Number(tier.stddev) || 0,
    pop,
  };
}

/**
 * Ratio of mean to median - how far a few large sales drag the average.
 *
 * Above `SKEWED` the average is not a typical price and the median is the
 * number to quote. Riven markets are routinely 3× or worse. The threshold is a
 * product judgement - see `LOWBALL_FRACTION`.
 */
export const SKEWED = 1.5;

export function skewOf(dist: Distribution): number | null {
  return dist.median > 0 && dist.avg > 0 ? dist.avg / dist.median : null;
}

/** Where an offer sits against the landmarks DE actually publishes. No model,
 *  no interpolation - each of these is directly checkable against the feed. */
export type Placement =
  | 'below-observed' //  under the cheapest sale DE saw
  | 'bottom' //          between the minimum and the median
  | 'middle' //          between the median and the average
  | 'upper' //           above the average, still within the observed range
  | 'above-observed'; //  over the dearest sale DE saw

export function placementOf(price: number, dist: Distribution): Placement {
  if (dist.min > 0 && price < dist.min) return 'below-observed';
  if (dist.max > 0 && price > dist.max) return 'above-observed';
  if (price < dist.median) return 'bottom';
  // An offer AT the median is the typical price by definition. It has to be
  // tested before the skew branch: on a flat market where the average equals
  // the median, falling through would call the most ordinary price on the
  // weapon "above the average", which is both wrong and the opposite of
  // useful.
  if (price === dist.median) return 'middle';
  // On a skewed market the average sits above the median, so "between them" is
  // a real band. On a flat or left-skewed one there is no such band and
  // anything past the median is the upper part of the range.
  if (dist.avg > dist.median && price < dist.avg) return 'middle';
  return 'upper';
}

export type PriceVerdict = 'below' | 'fair' | 'above' | 'outlier';

/**
 * How far under the median an offer has to sit before it is a lowball rather
 * than ordinary haggling.
 *
 * A PRODUCT judgement, not something DE's statistics imply - stated plainly
 * because the rest of this module is careful about that distinction. Same goes
 * for `SKEWED`. Both are tuneable opinions about presentation; neither is
 * dressed up as a derived quantity.
 */
export const LOWBALL_FRACTION = 0.75;

/** How an offer reads against the weapon's own market. */
export function judgeOffer(price: number, dist: Distribution): PriceVerdict {
  switch (placementOf(price, dist)) {
    case 'below-observed':
      return 'below';
    case 'bottom':
      return price < dist.median * LOWBALL_FRACTION ? 'below' : 'fair';
    case 'middle':
      return 'fair';
    case 'upper':
      return 'above';
    case 'above-observed':
      return 'outlier';
  }
}

/**
 * What can be said about rerolling, which is less than you would like.
 *
 * There is no probability here and no direction either. An earlier version
 * returned a "lean" - likely gains / likely loses / coin flip - and that was
 * still a probability claim wearing different clothes: a reroll draws from the
 * set of POSSIBLE rolls while DE publishes the SOLD ones, so no comparison
 * against the sold median establishes what a new roll will do. The band width
 * that separated the three cases was arbitrary on top of that.
 *
 * So this reports two facts and stops: what the spin costs, and where the
 * offer sits relative to the weapon's median. The reader draws the inference.
 */
export interface RerollRead {
  /** Kuva this spin costs. */
  kuva: number;
  /** The weapon's median trade. */
  median: number;
  /** Whether the offer is above that median. A fact about two published
   *  numbers, not a forecast. */
  aboveMedian: boolean;
}

export function rerollRead(price: number, dist: Distribution, rerolls: number, locked = false): RerollRead {
  return {
    kuva: rerollCost(rerolls, locked),
    median: dist.median,
    aboveMedian: price > dist.median,
  };
}

/**
 * How much less a rerolled riven fetches than an unrolled one on this weapon.
 *
 * Straight out of DE's feed, which splits `rerolled` - and nothing in the
 * ecosystem surfaces it. Buyers pay for reroll headroom, so an unrolled riven
 * of the same apparent quality is usually worth more. Medians, not means,
 * because these markets are skewed. Null when either side has no price.
 */
export function rerolledDiscount(
  unrolled: RivenStatTier | null | undefined,
  rolled: RivenStatTier | null | undefined,
): number | null {
  const u = distributionOf(unrolled);
  const r = distributionOf(rolled);
  if (!u || !r || u.median <= 0) return null;
  return (u.median - r.median) / u.median;
}

export interface Appraisal {
  dist: Distribution | null;
  /** Set when DE published no usable price for this tier. */
  unavailable?: 'no-data';
  placement: Placement | null;
  verdict: PriceVerdict | null;
  reroll: RerollRead | null;
  /** True when the average is dragged well above the median - the average is
   *  then not a typical price and the UI must lead with the median. */
  skewed: boolean;
  /** Notes the UI must show alongside any number - the limits of the model. */
  caveats: string[];
}

/**
 * Appraise an offer against a weapon's distribution.
 *
 * `price` is supplied by the user. We do not invent it, and the caveats say so.
 */
export function appraise(
  price: number | null,
  tier: RivenStatTier | null | undefined,
  rerolls: number,
  locked = false,
): Appraisal {
  const dist = distributionOf(tier);

  if (!dist) {
    return {
      dist: null,
      unavailable: 'no-data',
      placement: null,
      verdict: null,
      reroll: null,
      skewed: false,
      caveats: ['DE published no price for this weapon and reroll state this week.'],
    };
  }

  const skew = skewOf(dist);
  const skewed = skew != null && skew >= SKEWED;

  const caveats = [
    `DE's weekly band for the weapon's market, not this riven's stats. Popularity ${dist.pop}/100 is a relative score, not a count of trades.`,
    'Stat desirability is not modelled; a god roll and a junk roll sit in the same band.',
    "Since Update 44.1 DE's band mixes spliced and classic rivens.",
  ];
  if (dist.pop <= LOW_POPULARITY) {
    caveats.push('Rarely traded this week: the band may rest on a handful of sales.');
  }
  if (skewed) {
    caveats.push(
      `A few large sales pull the average (${dist.avg.toFixed(0)}p) well above the median (${dist.median.toFixed(0)}p) - read the median.`,
    );
  }

  if (price == null || !(price > 0)) {
    return { dist, placement: null, verdict: null, reroll: null, skewed, caveats };
  }

  return {
    dist,
    placement: placementOf(price, dist),
    verdict: judgeOffer(price, dist),
    reroll: rerollRead(price, dist, rerolls, locked),
    skewed,
    caveats,
  };
}

/** Owner-status buckets. Anything unrecognized stays `unknown` rather than
 *  being folded into `offline`, which would be a claim WFM did not make. */
export interface CompsStatusCounts {
  online: number;
  ingame: number;
  offline: number;
  unknown: number;
}

/**
 * What the comps sample on screen actually is.
 *
 * The sample is the cheapest eligible auctions for one weapon, not
 * attribute-matched comparables and not a record of completed sales. Listing
 * age is not time-to-sale, and an update is not evidence that a seller is
 * reachable - so this describes the sample and stops there: no verdict, no
 * liquidity score, no "how fast this sells".
 */
export interface CompsSampleRead {
  size: number;
  dated: number;
  oldestAskDays: number | null;
  newestAskDays: number | null;
  status: CompsStatusCounts;
}

export function describeComps(
  comps: readonly RivenAuction[],
  nowMs: number,
): CompsSampleRead {
  const status: CompsStatusCounts = { online: 0, ingame: 0, offline: 0, unknown: 0 };
  const ages: number[] = [];
  for (const comp of comps) {
    const state = comp.owner_status;
    if (state === 'online' || state === 'ingame' || state === 'offline') status[state] += 1;
    else status.unknown += 1;

    const created = Date.parse(comp.created ?? '');
    if (!Number.isFinite(created) || !Number.isFinite(nowMs)) continue;
    const days = Math.floor((nowMs - created) / 86_400_000);
    if (days < 0) continue;
    ages.push(days);
  }
  return {
    size: comps.length,
    dated: ages.length,
    oldestAskDays: ages.length ? Math.max(...ages) : null,
    newestAskDays: ages.length ? Math.min(...ages) : null,
    status,
  };
}
