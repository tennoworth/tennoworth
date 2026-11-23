import { describe, expect, it } from 'vitest';

import { appraise, describeComps, rerolledDiscount } from './riven-appraise';
import type { RivenAuction } from '../contracts/desktop';
import type { RivenStatTier } from '../contracts/data';
import fixture from '../../../tests/fixtures/rivens/appraise.json';

function tier(over: Partial<RivenStatTier>): RivenStatTier {
  return {
    avg: 210,
    median: 195,
    min: 7,
    max: 980,
    stddev: 120,
    pop: 37,
    ...over,
  } as RivenStatTier;
}

/** The native popularity floor (`appraise::LOW_POPULARITY`). */
const LOW_POPULARITY = 1;

/**
 * The distribution that broke the previous implementation: 90 sales at 10p and
 * 10 at 1,000p. A normal fit to its mean and standard deviation called a 10p
 * offer the 37th percentile, gave a reroll a 63% chance of beating it, and
 * priced the losing outcomes at MINUS 194p - below a stated minimum of 10.
 */
const SKEWED_MARKET = tier({ avg: 109, median: 10, min: 10, max: 1000, stddev: 297, pop: 100 });

const placement = (price: number, t: RivenStatTier) => appraise(price, t, 0).placement;
const verdict = (price: number, t: RivenStatTier) => appraise(price, t, 0).verdict;
const kuva = (rerolls: number, locked = false) => appraise(400, tier({}), rerolls, locked).reroll?.kuva;

describe('the band', () => {
  // DE's pop is a 0-100 popularity score, and over half of each week's rows
  // sit at 1. Reading it as a trade count hid nearly every band.
  it('keeps a band whatever its popularity, because pop is not a count', () => {
    expect(appraise(null, tier({ pop: 1 }), 0).dist?.median).toBe(195);
    expect(appraise(null, tier({ pop: 0 }), 0).dist?.median).toBe(195);
  });

  it('rejects a tier with no published price', () => {
    expect(appraise(null, tier({ median: 0 }), 0).dist).toBeNull();
  });

  it('accepts a real sample', () => {
    expect(appraise(null, tier({}), 0).dist?.pop).toBe(37);
  });

  it('no longer requires a standard deviation, which nothing reads now', () => {
    expect(appraise(null, tier({ stddev: 0 }), 0).dist).not.toBeNull();
  });

  it('is no-data rather than throwing on a missing tier', () => {
    expect(appraise(null, null, 0).unavailable).toBe('no-data');
    expect(appraise(null, undefined, 0).unavailable).toBe('no-data');
  });
});

describe('placement', () => {
  it('places against the landmarks DE actually publishes', () => {
    expect(placement(3, tier({}))).toBe('below-observed');
    expect(placement(100, tier({}))).toBe('bottom');
    expect(placement(200, tier({}))).toBe('middle'); // median 195, avg 210
    expect(placement(400, tier({}))).toBe('upper');
    expect(placement(2000, tier({}))).toBe('above-observed');
  });

  it('never claims a band the skewed market contradicts', () => {
    // 10p is the cheapest sale observed, and 90% of trades were at it. The old
    // normal fit called this the 37th percentile; the landmark answer is
    // simply "at the bottom", which is true.
    expect(placement(10, SKEWED_MARKET)).toBe('middle');
    expect(placement(9, SKEWED_MARKET)).toBe('below-observed');
    expect(placement(500, SKEWED_MARKET)).toBe('upper');
  });

  it('calls an at-the-median offer typical, even on a flat market', () => {
    // With avg == median, falling through to the skew branch called the most
    // ordinary price on the weapon "above the average" - wrong, and the
    // opposite of useful.
    const flat = tier({ avg: 100, median: 100, min: 90, max: 110 });
    expect(placement(100, flat)).toBe('middle');
    expect(verdict(100, flat)).toBe('fair');
    expect(placement(95, flat)).toBe('bottom');
    expect(placement(105, flat)).toBe('upper');
  });

  it('calls an at-the-median offer typical on a left-skewed market too', () => {
    expect(placement(100, tier({ avg: 80, median: 100, min: 40, max: 120 }))).toBe('middle');
  });
});

describe('skew', () => {
  it('flags a market whose average large sales drag far above the median', () => {
    expect(appraise(null, SKEWED_MARKET, 0).skewed).toBe(true);
    expect(appraise(null, tier({}), 0).skewed).toBe(false);
    expect(appraise(null, tier({ avg: 150, median: 100 }), 0).skewed).toBe(true);
    expect(appraise(null, tier({ avg: 149, median: 100 }), 0).skewed).toBe(false);
  });
});

describe('verdict', () => {
  it('calls a median-ish offer fair', () => {
    expect(verdict(195, tier({}))).toBe('fair');
    expect(verdict(200, tier({}))).toBe('fair');
  });

  it('calls a real lowball low', () => {
    expect(verdict(60, tier({}))).toBe('below');
  });

  it('does not call a slightly-under-median offer a lowball', () => {
    expect(verdict(180, tier({}))).toBe('fair');
    expect(verdict(146.25, tier({}))).toBe('fair');
    expect(verdict(146, tier({}))).toBe('below');
  });

  it('calls an above-average offer high, and one past the range an outlier', () => {
    expect(verdict(400, tier({}))).toBe('above');
    expect(verdict(1500, tier({}))).toBe('outlier');
  });
});

describe('reroll cost', () => {
  it('climbs with the reroll count and then caps', () => {
    expect(kuva(0)).toBe(900);
    expect(kuva(4)).toBe(1700);
    expect(kuva(9)).toBe(3500);
    expect(kuva(50)).toBe(3500);
  });

  it('treats a nonsense count as a fresh riven rather than throwing', () => {
    expect(kuva(-3)).toBe(900);
  });

  it('doubles while a trait is locked, up to 7,000 at the cap', () => {
    expect(kuva(0, true)).toBe(1800);
    expect(kuva(50, true)).toBe(7000);
    expect(kuva(4, false)).toBe(1700);
  });
});

describe('reroll read', () => {
  it('reports the cost and the median, and nothing else', () => {
    const r = appraise(400, tier({}), 3).reroll!;
    expect(r.kuva).toBe(1400);
    expect(r.median).toBe(195);
    expect(r.aboveMedian).toBe(true);
  });

  it('states the comparison as a fact about two published numbers', () => {
    expect(appraise(40, tier({}), 0).reroll?.aboveMedian).toBe(false);
    expect(appraise(195, tier({}), 0).reroll?.aboveMedian).toBe(false);
  });

  it('exposes no probability and no direction', () => {
    // A "lean" is a probability claim wearing different clothes: a reroll
    // draws from the POSSIBLE rolls while DE publishes the SOLD ones. If a
    // fourth field ever appears on this type, a model came back.
    const r = appraise(300, tier({}), 1).reroll!;
    expect(Object.keys(r).sort()).toEqual(['aboveMedian', 'kuva', 'median']);
  });
});

describe('rerolledDiscount', () => {
  it('measures the premium buyers pay for reroll headroom', () => {
    const d = rerolledDiscount(tier({ median: 200 }), tier({ median: 176 }));
    expect(d).toBeCloseTo(0.12, 4);
  });

  it('refuses to compare when either side has no price', () => {
    expect(rerolledDiscount(tier({ median: 200 }), tier({ median: 0 }))).toBeNull();
    expect(rerolledDiscount(null, tier({}))).toBeNull();
  });
});

describe('appraise', () => {
  it('places an offer on a low-popularity band and warns instead of refusing', () => {
    const a = appraise(200, tier({ pop: LOW_POPULARITY }), 0);
    expect(a.unavailable).toBeUndefined();
    expect(a.placement).not.toBeNull();
    expect(a.caveats).toContain('rare');
    expect(appraise(200, tier({ pop: LOW_POPULARITY + 1 }), 0).caveats).not.toContain('rare');
  });

  it('reports no data when DE published no price', () => {
    const a = appraise(200, tier({ median: 0 }), 0);
    expect(a.unavailable).toBe('no-data');
    expect(a.placement).toBeNull();
  });

  it('prices a reroll with the lock when one is set', () => {
    expect(appraise(400, tier({}), 3, true).reroll?.kuva).toBe(2800);
    expect(appraise(400, tier({}), 3).reroll?.kuva).toBe(1400);
  });

  it('returns the distribution with no verdict when no price is supplied', () => {
    const a = appraise(null, tier({}), 0);
    expect(a.dist).not.toBeNull();
    expect(a.verdict).toBeNull();
    expect(a.reroll).toBeNull();
  });

  it('always carries the caveat that stat quality is not modelled', () => {
    const a = appraise(250, tier({}), 2);
    expect(a.caveats).toEqual(expect.arrayContaining(['band', 'desirability', 'spliced']));
  });

  it('warns when the average is not a typical price', () => {
    const a = appraise(10, SKEWED_MARKET, 0);
    expect(a.skewed).toBe(true);
    expect(a.caveats).toContain('skewed');
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
    expect(appraise(Number.NaN, tier({}), 0).placement).toBeNull();
  });
});

function comp(over: Partial<RivenAuction>): RivenAuction {
  return {
    id: 'a', price: 100, buyout_price: 100, starting_price: 100, top_bid: null,
    is_direct_sell: true, owner: 'Someone', owner_status: 'online', mod_rank: 0,
    mastery_level: 10, re_rolls: 0, polarity: null, created: null, updated: null,
    name: null, platform: 'pc', attributes: [],
    ...over,
  };
}

describe('describeComps', () => {
  const now = Date.parse('2026-09-19T00:00:00Z');

  it('describes an empty sample without inventing ages or a verdict', () => {
    expect(describeComps([], now)).toEqual({
      size: 0, dated: 0, oldestAskDays: null, newestAskDays: null,
      status: { online: 0, ingame: 0, offline: 0, unknown: 0 },
    });
  });

  it('reports the span of dated asks and how many carry a usable instant', () => {
    const read = describeComps([
      comp({ id: 'old', created: '2026-09-09T00:00:00Z' }),
      comp({ id: 'new', created: '2026-09-16T00:00:00Z' }),
      comp({ id: 'undated', created: null }),
    ], now);
    expect(read.size).toBe(3);
    expect(read.dated).toBe(2);
    expect(read.oldestAskDays).toBe(10);
    expect(read.newestAskDays).toBe(3);
  });

  it('keeps future and malformed instants out of the age span', () => {
    const read = describeComps([
      comp({ created: '2026-09-25T00:00:00Z' }),
      comp({ created: 'not a date' }),
      comp({ created: '2026-09-18T00:00:00Z' }),
    ], now);
    expect(read.dated).toBe(1);
    expect(read.oldestAskDays).toBe(1);
  });

  it('counts an unrecognized owner status as unknown, never offline', () => {
    const read = describeComps([
      comp({ owner_status: 'online' }),
      comp({ owner_status: 'ingame' }),
      comp({ owner_status: 'offline' }),
      comp({ owner_status: 'busy' }),
      comp({ owner_status: null }),
    ], now);
    expect(read.status).toEqual({ online: 1, ingame: 1, offline: 1, unknown: 2 });
  });

  it('reads no ages without a usable now', () => {
    expect(describeComps([comp({ created: '2026-09-09T00:00:00Z' })], Number.NaN).dated).toBe(0);
  });
});

// The outputs the TypeScript rules gave before they moved native.
describe('shared fixture', () => {
  it('appraises as before', () => {
    for (const c of fixture.appraise) {
      expect(appraise(c.price, c.tier as RivenStatTier | null | undefined, c.rerolls, c.locked), c.name).toEqual(c.expected);
    }
  });

  it('measures the rerolled discount as before', () => {
    for (const c of fixture.rerolled_discount) {
      expect(rerolledDiscount(c.unrolled as RivenStatTier | null, c.rolled as RivenStatTier | null), c.name).toBe(c.expected);
    }
  });

  it('describes comps as before', () => {
    for (const c of fixture.describe_comps) {
      const comps = c.comps.map((a) => ({ ...comp({}), owner_status: a.owner_status, created: a.created }) as RivenAuction);
      expect(describeComps(comps, c.now_ms as number), c.name).toEqual(c.expected);
    }
  });
});
