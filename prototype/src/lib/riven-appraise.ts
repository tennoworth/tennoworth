// Riven appraisal, and the reroll decision.
//
// WHAT THIS DELIBERATELY DOES NOT DO: guess what a riven is worth from its
// stats. Doing that needs per-stat roll ranges and a model of which stats
// buyers want, and we have neither — DE's export publishes dispositions, not
// roll ranges. Every "riven price checker" that shows you a confident number
// is guessing, which is why nobody trusts them.
//
// What we DO have is better than a guess and nobody surfaces it: DE publishes
// the actual trade distribution per weapon per reroll-state — average, median,
// min, max, standard deviation and the population behind them. So this module
// answers the questions that distribution can genuinely answer:
//
//   "Someone offered me 200p. Where does that sit for this weapon?"
//   "If I reroll, what are the odds I beat what I have?"
//   "Do rerolled rivens on this weapon trade below unrolled ones?"
//
// The price comes from the user (an offer, or their own estimate). We supply
// the distribution and the arithmetic.

import type { RivenStatTier } from './types';

/** Below this many observed trades the distribution is not a distribution.
 *  DE's weekly file happily reports pop: 1, and a median of one sale is a
 *  single anecdote wearing a statistic's clothes. */
export const MIN_POPULATION = 12;

/**
 * Kuva per reroll. Climbs with the reroll count and caps.
 *
 * In-game values, and they are the reason "just reroll it" is not free advice:
 * a riven at cap costs 3,500 kuva a spin.
 */
export const REROLL_KUVA = [900, 1000, 1200, 1400, 1700, 2000, 2350, 2750, 3150, 3500] as const;

export function rerollCost(rerolls: number): number {
  const i = Math.max(0, Math.floor(rerolls));
  return REROLL_KUVA[Math.min(i, REROLL_KUVA.length - 1)];
}

/** Abramowitz & Stegun 7.1.26 — good to ~1e-7, which is far past what a
 *  price distribution built from a few hundred trades can justify. */
function erf(x: number): number {
  const sign = x < 0 ? -1 : 1;
  const a = Math.abs(x);
  const t = 1 / (1 + 0.3275911 * a);
  const y =
    1 -
    ((((1.061405429 * t - 1.453152027) * t + 1.421413741) * t - 0.284496736) * t + 0.254829592) *
      t *
      Math.exp(-a * a);
  return sign * y;
}

/** Standard normal CDF. */
export function normalCdf(z: number): number {
  return 0.5 * (1 + erf(z / Math.SQRT2));
}

/** Standard normal PDF. */
function normalPdf(z: number): number {
  return Math.exp(-0.5 * z * z) / Math.sqrt(2 * Math.PI);
}

export interface Distribution {
  median: number;
  avg: number;
  min: number;
  max: number;
  stddev: number;
  pop: number;
}

/** A usable distribution, or null when the sample is too thin to reason with. */
export function distributionOf(tier: RivenStatTier | null | undefined): Distribution | null {
  if (!tier) return null;
  const pop = Number(tier.pop) || 0;
  const stddev = Number(tier.stddev) || 0;
  const median = Number(tier.median) || 0;
  const avg = Number(tier.avg) || 0;
  if (pop < MIN_POPULATION || median <= 0 || stddev <= 0) return null;
  return {
    median,
    avg,
    min: Number(tier.min) || 0,
    max: Number(tier.max) || 0,
    stddev,
    pop,
  };
}

/**
 * Where a price sits in the weapon's distribution, 0–100.
 *
 * Normal approximation around the MEAN, not the median — the mean is what the
 * standard deviation is defined against, and riven prices are right-skewed so
 * the two differ. Reported to the nearest whole percent because a decimal
 * would imply precision this sample does not have.
 */
export function percentileOf(price: number, dist: Distribution): number {
  const z = (price - dist.avg) / dist.stddev;
  return Math.round(Math.min(100, Math.max(0, normalCdf(z) * 100)));
}

export type PriceVerdict = 'below' | 'fair' | 'above' | 'outlier';

/** How an offer reads against the weapon's own market. */
export function judgeOffer(price: number, dist: Distribution): PriceVerdict {
  if (dist.max > 0 && price > dist.max) return 'outlier';
  const p = percentileOf(price, dist);
  if (p < 35) return 'below';
  if (p > 65) return 'above';
  return 'fair';
}

export interface RerollOutcome {
  /** Chance the next roll is worth more than what you hold. */
  pBetter: number;
  /** Average price of the outcomes that beat what you hold. */
  meanIfBetter: number;
  /** Average price of the outcomes that do not. */
  meanIfWorse: number;
  /** Expected change in plat from one spin. Usually negative above the mean —
   *  that is the point. */
  expectedChange: number;
  /** Kuva this spin costs. */
  kuva: number;
}

/**
 * One reroll, modelled as redrawing from the same weapon's distribution.
 *
 * The approximation is stated because it matters: a reroll does not sample the
 * *sold* distribution, it samples the *possible* one, and the sold distribution
 * is biased toward rolls good enough that somebody listed them. So this
 * flatters rerolling slightly. It is still the right shape — the expected
 * change goes negative once you are above the mean, which is the thing players
 * get wrong.
 */
export function rerollOutcome(
  currentPrice: number,
  dist: Distribution,
  rerolls: number,
): RerollOutcome {
  const z = (currentPrice - dist.avg) / dist.stddev;
  const pWorse = normalCdf(z);
  const pBetter = 1 - pWorse;

  // Truncated normal means. E[X | X > c] = μ + σ·φ(z)/(1−Φ(z)).
  const phi = normalPdf(z);
  const meanIfBetter = pBetter > 1e-9 ? dist.avg + (dist.stddev * phi) / pBetter : currentPrice;
  const meanIfWorse = pWorse > 1e-9 ? dist.avg - (dist.stddev * phi) / pWorse : currentPrice;

  return {
    pBetter,
    meanIfBetter,
    meanIfWorse,
    expectedChange: dist.avg - currentPrice,
    kuva: rerollCost(rerolls),
  };
}

/**
 * How much less a rerolled riven fetches than an unrolled one on this weapon.
 *
 * Straight out of DE's feed, which splits `rerolled` — and nothing in the
 * ecosystem surfaces it. Buyers pay for reroll headroom, so an unrolled riven
 * of the same apparent quality is usually worth more. Returns null when either
 * side is too thin to compare, rather than a percentage built on two sales.
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
  /** Why there is no distribution, when there isn't one. */
  unavailable?: 'no-data' | 'thin-sample';
  percentile: number | null;
  verdict: PriceVerdict | null;
  reroll: RerollOutcome | null;
  /** Notes the UI must show alongside any number — the limits of the model. */
  caveats: string[];
}

/**
 * Appraise an offer against a weapon's distribution.
 *
 * `price` is supplied by the user — an offer they received, or their own
 * estimate. We do not invent it, and the caveats say so.
 */
export function appraise(
  price: number | null,
  tier: RivenStatTier | null | undefined,
  rerolls: number,
): Appraisal {
  const dist = distributionOf(tier);
  const caveats: string[] = [];

  if (!dist) {
    const pop = Number(tier?.pop) || 0;
    return {
      dist: null,
      unavailable: pop > 0 ? 'thin-sample' : 'no-data',
      percentile: null,
      verdict: null,
      reroll: null,
      caveats:
        pop > 0
          ? [`Only ${pop} trade${pop === 1 ? '' : 's'} observed — too few to place a price against.`]
          : ['DE published no trades for this weapon and reroll state this week.'],
    };
  }

  caveats.push(
    `Based on ${dist.pop} trades DE observed this week — the weapon's market, not this riven's stats.`,
  );
  caveats.push('Stat desirability is not modelled; a god roll and a junk roll sit in the same band.');

  if (price == null || !(price > 0)) {
    return { dist, percentile: null, verdict: null, reroll: null, caveats };
  }

  return {
    dist,
    percentile: percentileOf(price, dist),
    verdict: judgeOffer(price, dist),
    reroll: rerollOutcome(price, dist, rerolls),
    caveats,
  };
}
