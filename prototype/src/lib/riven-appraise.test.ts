import { describe, expect, it } from 'vitest';

import {
  appraise,
  distributionOf,
  judgeOffer,
  MIN_POPULATION,
  normalCdf,
  percentileOf,
  rerollCost,
  rerolledDiscount,
  rerollOutcome,
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

describe('normalCdf', () => {
  it('is a proper CDF at the landmarks', () => {
    expect(normalCdf(0)).toBeCloseTo(0.5, 4);
    expect(normalCdf(1.96)).toBeCloseTo(0.975, 3);
    expect(normalCdf(-1.96)).toBeCloseTo(0.025, 3);
  });
});

describe('distributionOf', () => {
  it('rejects a sample too thin to be a distribution', () => {
    // DE's weekly file happily reports pop: 1. A median of one sale is an
    // anecdote, and pricing against it would be worse than saying nothing.
    expect(distributionOf(tier({ pop: MIN_POPULATION - 1 }))).toBeNull();
  });

  it('rejects a zero standard deviation, which cannot place anything', () => {
    expect(distributionOf(tier({ stddev: 0 }))).toBeNull();
  });

  it('accepts a real sample', () => {
    expect(distributionOf(tier({}))?.pop).toBe(1204);
  });

  it('is null rather than throwing on a missing tier', () => {
    expect(distributionOf(null)).toBeNull();
    expect(distributionOf(undefined)).toBeNull();
  });
});

describe('percentileOf', () => {
  const dist = distributionOf(tier({}))!;

  it('puts the mean at the middle', () => {
    expect(percentileOf(210, dist)).toBe(50);
  });

  it('rises with price', () => {
    expect(percentileOf(330, dist)).toBeGreaterThan(percentileOf(210, dist));
    expect(percentileOf(90, dist)).toBeLessThan(percentileOf(210, dist));
  });

  it('clamps rather than reporting an impossible percentile', () => {
    expect(percentileOf(100000, dist)).toBe(100);
    expect(percentileOf(0, dist)).toBeGreaterThanOrEqual(0);
  });
});

describe('judgeOffer', () => {
  const dist = distributionOf(tier({}))!;

  it('calls a mean-ish offer fair', () => {
    expect(judgeOffer(210, dist)).toBe('fair');
  });

  it('calls a lowball low and a premium high', () => {
    expect(judgeOffer(60, dist)).toBe('below');
    expect(judgeOffer(400, dist)).toBe('above');
  });

  it('flags a price above anything DE observed as an outlier, not merely high', () => {
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

describe('rerollOutcome', () => {
  const dist = distributionOf(tier({}))!;

  it('is a coin flip at the mean', () => {
    const o = rerollOutcome(210, dist, 0);
    expect(o.pBetter).toBeCloseTo(0.5, 2);
    expect(o.expectedChange).toBeCloseTo(0, 6);
  });

  it('goes negative above the mean — the thing players get wrong', () => {
    const o = rerollOutcome(400, dist, 3);
    expect(o.pBetter).toBeLessThan(0.1);
    expect(o.expectedChange).toBeLessThan(0);
    expect(o.kuva).toBe(1400);
  });

  it('goes positive below the mean', () => {
    const o = rerollOutcome(40, dist, 0);
    expect(o.pBetter).toBeGreaterThan(0.85);
    expect(o.expectedChange).toBeGreaterThan(0);
  });

  it('brackets the current price with the two conditional means', () => {
    const o = rerollOutcome(210, dist, 0);
    expect(o.meanIfBetter).toBeGreaterThan(210);
    expect(o.meanIfWorse).toBeLessThan(210);
  });

  it('stays finite at an absurd price where the tail probability underflows', () => {
    const o = rerollOutcome(1_000_000, dist, 0);
    expect(Number.isFinite(o.meanIfBetter)).toBe(true);
    expect(Number.isFinite(o.meanIfWorse)).toBe(true);
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
    expect(a.percentile).toBeNull();
    expect(a.caveats[0]).toContain('4 trades');
  });

  it('distinguishes no data from a thin sample', () => {
    const a = appraise(200, tier({ pop: 0 }), 0);
    expect(a.unavailable).toBe('no-data');
  });

  it('returns the distribution with no verdict when no price is supplied', () => {
    const a = appraise(null, tier({}), 0);
    expect(a.dist).not.toBeNull();
    expect(a.verdict).toBeNull();
    expect(a.reroll).toBeNull();
  });

  it('always carries the caveat that stat quality is not modelled', () => {
    // The whole design rests on being honest about this; if the caveat ever
    // stops shipping, the feature has quietly become a guess-o-matic.
    const a = appraise(250, tier({}), 2);
    expect(a.caveats.some((c) => c.includes('Stat desirability is not modelled'))).toBe(true);
    expect(a.caveats.some((c) => c.includes('1204 trades'))).toBe(true);
  });

  it('appraises a supplied offer end to end', () => {
    const a = appraise(340, tier({}), 8);
    expect(a.percentile).toBeGreaterThan(50);
    expect(a.verdict).toBe('above');
    expect(a.reroll?.kuva).toBe(3150);
  });

  it('ignores a nonsense price rather than producing NaN', () => {
    const a = appraise(0, tier({}), 0);
    expect(a.percentile).toBeNull();
    expect(a.reroll).toBeNull();
  });
});
