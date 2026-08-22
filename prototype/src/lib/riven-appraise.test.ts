import { describe, expect, it } from 'vitest';

import {
  appraise,
  distributionOf,
  judgeOffer,
  MIN_POPULATION,
  placementOf,
  rerollCost,
  rerolledDiscount,
  rerollRead,
  skewOf,
} from './riven-appraise';
import type { RivenStatTier } from './types';

function tier(over: Partial<RivenStatTier>): RivenStatTier {
  return {
    avg: 210,
    median: 195,
    min: 7,
    max: 980,
    stddev: 120,
    pop: 1204,
    ...over,
  } as RivenStatTier;
}

/**
 * The distribution that broke the previous implementation: 90 sales at 10p and
 * 10 at 1,000p. A normal fit to its mean and standard deviation called a 10p
 * offer the 37th percentile, gave a reroll a 63% chance of beating it, and
 * priced the losing outcomes at MINUS 194p — below a stated minimum of 10.
 */
const SKEWED_MARKET = tier({ avg: 109, median: 10, min: 10, max: 1000, stddev: 297, pop: 100 });

describe('distributionOf', () => {
  it('rejects a sample too thin to be a distribution', () => {
    expect(distributionOf(tier({ pop: MIN_POPULATION - 1 }))).toBeNull();
  });

  it('accepts a real sample', () => {
    expect(distributionOf(tier({}))?.pop).toBe(1204);
  });

  it('no longer requires a standard deviation, which nothing reads now', () => {
    expect(distributionOf(tier({ stddev: 0 }))).not.toBeNull();
  });

  it('is null rather than throwing on a missing tier', () => {
    expect(distributionOf(null)).toBeNull();
    expect(distributionOf(undefined)).toBeNull();
  });
});

describe('placementOf', () => {
  const dist = distributionOf(tier({}))!;

  it('places against the landmarks DE actually publishes', () => {
    expect(placementOf(3, dist)).toBe('below-observed');
    expect(placementOf(100, dist)).toBe('bottom');
    expect(placementOf(200, dist)).toBe('middle'); // median 195, avg 210
    expect(placementOf(400, dist)).toBe('upper');
    expect(placementOf(2000, dist)).toBe('above-observed');
  });

  it('never claims a band the skewed market contradicts', () => {
    // 10p is the cheapest sale observed, and 90% of trades were at it. The old
    // normal fit called this the 37th percentile; the landmark answer is
    // simply "at the bottom", which is true.
    const dist2 = distributionOf(SKEWED_MARKET)!;
    expect(placementOf(10, dist2)).toBe('middle');
    expect(placementOf(9, dist2)).toBe('below-observed');
    expect(placementOf(500, dist2)).toBe('upper');
  });

  it('calls an at-the-median offer typical, even on a flat market', () => {
    // With avg == median, falling through to the skew branch called the most
    // ordinary price on the weapon "above the average" — wrong, and the
    // opposite of useful.
    const flat = distributionOf(tier({ avg: 100, median: 100, min: 90, max: 110 }))!;
    expect(placementOf(100, flat)).toBe('middle');
    expect(judgeOffer(100, flat)).toBe('fair');
    expect(placementOf(95, flat)).toBe('bottom');
    expect(placementOf(105, flat)).toBe('upper');
  });

  it('calls an at-the-median offer typical on a left-skewed market too', () => {
    const left = distributionOf(tier({ avg: 80, median: 100, min: 40, max: 120 }))!;
    expect(placementOf(100, left)).toBe('middle');
  });
});

describe('skewOf', () => {
  it('measures how far large sales drag the average', () => {
    expect(skewOf(distributionOf(SKEWED_MARKET)!)).toBeCloseTo(10.9, 1);
    expect(skewOf(distributionOf(tier({}))!)).toBeCloseTo(1.08, 2);
  });
});

describe('judgeOffer', () => {
  const dist = distributionOf(tier({}))!;

  it('calls a median-ish offer fair', () => {
    expect(judgeOffer(195, dist)).toBe('fair');
    expect(judgeOffer(200, dist)).toBe('fair');
  });

  it('calls a real lowball low', () => {
    expect(judgeOffer(60, dist)).toBe('below');
  });

  it('does not call a slightly-under-median offer a lowball', () => {
    expect(judgeOffer(180, dist)).toBe('fair');
  });

  it('calls an above-average offer high, and one past the range an outlier', () => {
    expect(judgeOffer(400, dist)).toBe('above');
    expect(judgeOffer(1500, dist)).toBe('outlier');
  });
});

describe('rerollCost', () => {
  it('climbs with the reroll count and then caps', () => {
    expect(rerollCost(0)).toBe(900);
    expect(rerollCost(4)).toBe(1700);
    expect(rerollCost(9)).toBe(3500);
    expect(rerollCost(50)).toBe(3500);
  });

  it('treats a nonsense count as a fresh riven rather than throwing', () => {
    expect(rerollCost(-3)).toBe(900);
  });
});

describe('rerollRead', () => {
  const dist = distributionOf(tier({}))!;

  it('reports the cost and the median, and nothing else', () => {
    const r = rerollRead(400, dist, 3);
    expect(r.kuva).toBe(1400);
    expect(r.median).toBe(195);
    expect(r.aboveMedian).toBe(true);
  });

  it('states the comparison as a fact about two published numbers', () => {
    expect(rerollRead(40, dist, 0).aboveMedian).toBe(false);
    expect(rerollRead(195, dist, 0).aboveMedian).toBe(false);
  });

  it('exposes no probability and no direction', () => {
    // A "lean" is a probability claim wearing different clothes: a reroll
    // draws from the POSSIBLE rolls while DE publishes the SOLD ones. If a
    // fourth field ever appears on this type, a model came back.
    const r = rerollRead(300, dist, 1);
    expect(Object.keys(r).sort()).toEqual(['aboveMedian', 'kuva', 'median']);
  });
});

describe('rerolledDiscount', () => {
  it('measures the premium buyers pay for reroll headroom', () => {
    const d = rerolledDiscount(tier({ median: 200 }), tier({ median: 176 }));
    expect(d).toBeCloseTo(0.12, 4);
  });

  it('refuses to compare when either side is too thin', () => {
    expect(rerolledDiscount(tier({ median: 200 }), tier({ pop: 3 }))).toBeNull();
    expect(rerolledDiscount(null, tier({}))).toBeNull();
  });
});

describe('appraise', () => {
  it('explains a thin sample instead of pricing against it', () => {
    const a = appraise(200, tier({ pop: 4 }), 0);
    expect(a.unavailable).toBe('thin-sample');
    expect(a.placement).toBeNull();
    expect(a.caveats[0]).toContain('4 trades');
  });

  it('distinguishes no data from a thin sample', () => {
    expect(appraise(200, tier({ pop: 0 }), 0).unavailable).toBe('no-data');
  });

  it('returns the distribution with no verdict when no price is supplied', () => {
    const a = appraise(null, tier({}), 0);
    expect(a.dist).not.toBeNull();
    expect(a.verdict).toBeNull();
    expect(a.reroll).toBeNull();
  });

  it('always carries the caveat that stat quality is not modelled', () => {
    const a = appraise(250, tier({}), 2);
    expect(a.caveats.some((c) => c.includes('Stat desirability is not modelled'))).toBe(true);
    expect(a.caveats.some((c) => c.includes('1204 trades'))).toBe(true);
  });

  it('warns when the average is not a typical price', () => {
    const a = appraise(10, SKEWED_MARKET, 0);
    expect(a.skewed).toBe(true);
    expect(a.caveats.some((c) => c.includes('read the median'))).toBe(true);
  });

  it('never reports a negative or impossible price on the skewed market', () => {
    // The regression this rewrite exists for.
    const a = appraise(10, SKEWED_MARKET, 0);
    const numbers = [a.dist!.median, a.dist!.avg, a.dist!.min, a.reroll!.median, a.reroll!.kuva];
    for (const n of numbers) expect(n).toBeGreaterThanOrEqual(0);
  });

  it('appraises a supplied offer end to end', () => {
    const a = appraise(340, tier({}), 8);
    expect(a.placement).toBe('upper');
    expect(a.verdict).toBe('above');
    expect(a.reroll?.kuva).toBe(3150);
    expect(a.reroll?.aboveMedian).toBe(true);
  });

  it('ignores a nonsense price rather than producing NaN', () => {
    const a = appraise(0, tier({}), 0);
    expect(a.placement).toBeNull();
    expect(a.reroll).toBeNull();
  });
});
