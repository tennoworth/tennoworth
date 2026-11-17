import { describe, expect, it } from 'vitest';
import { DEFAULT_PRICE_RULE, MAX_PRICE_OFFSET, MIN_PRICE_OFFSET, parsePriceRule, priceFromRule, reasonableLowestAsk, snapshotEvidence } from './price-rule';

describe('priceFromRule', () => {
  const evidence = { lowestAsk: 20, low5Avg: 22.4, low20Avg: 27, avg48h: 25.2, median90d: 30 };

  it('prices from the average of the twenty cheapest asks, and only when the snapshot has it', () => {
    expect(priceFromRule({ basis: 'low20-avg', offset: 1 }, evidence)).toBe(28);
    expect(priceFromRule({ basis: 'low20-avg', offset: -3 }, { ...evidence, low20Avg: 0 })).toBeNull();
    expect(parsePriceRule('{"basis":"low20-avg","offset":-2}')).toEqual({ basis: 'low20-avg', offset: -2 });
  });

  it('adds the offset to the chosen basis without rounding the unit price', () => {
    expect(priceFromRule({ basis: 'lowest-ask', offset: 0 }, evidence)).toBe(20);
    expect(priceFromRule({ basis: 'low5-avg', offset: 1 }, evidence)).toBeCloseTo(23.4);
    expect(priceFromRule({ basis: 'avg-48h', offset: -2 }, evidence)).toBeCloseTo(23.2);
    expect(priceFromRule({ basis: 'median-90d', offset: 1 }, evidence)).toBe(31);
  });

  it('never suggests less than 1p, or less than a sub-1p basis', () => {
    expect(priceFromRule({ basis: 'lowest-ask', offset: -50 }, { lowestAsk: 3 })).toBe(1);
    expect(priceFromRule({ basis: 'lowest-ask', offset: -2 }, { lowestAsk: 0.5 })).toBe(0.5);
  });

  it('suggests nothing when the basis is unknown, instead of inventing a price', () => {
    expect(priceFromRule({ basis: 'median-90d', offset: 1 }, { lowestAsk: 20 })).toBeNull();
    expect(priceFromRule({ basis: 'low5-avg', offset: 1 }, { low5Avg: 0 })).toBeNull();
    expect(priceFromRule({ basis: 'lowest-ask', offset: 0 }, { lowestAsk: null })).toBeNull();
    expect(priceFromRule({ basis: 'avg-48h', offset: 0 }, { avg48h: Number.NaN })).toBeNull();
  });
});

describe('parsePriceRule', () => {
  it('keeps a valid rule and clamps its offset', () => {
    expect(parsePriceRule('{"basis":"median-90d","offset":1}')).toEqual({ basis: 'median-90d', offset: 1 });
    expect(parsePriceRule('{"basis":"avg-48h","offset":999}')).toEqual({ basis: 'avg-48h', offset: MAX_PRICE_OFFSET });
    expect(parsePriceRule('{"basis":"avg-48h","offset":-999}')).toEqual({ basis: 'avg-48h', offset: MIN_PRICE_OFFSET });
  });

  it.each([null, '', 'not json', '{"basis":"top-20","offset":1}', '{"basis":"avg-48h","offset":1.5}', '{"basis":"avg-48h"}'])(
    'falls back to matching the lowest ask for %j',
    raw => expect(parsePriceRule(raw)).toEqual(DEFAULT_PRICE_RULE),
  );
});

describe('reasonableLowestAsk', () => {
  it('skips a troll undercut for the next believable ask', () => {
    expect(reasonableLowestAsk([2, 14, 15], 18)).toEqual({ ask: 14, ignored: 2 });
    expect(reasonableLowestAsk([1, 2, 14], 18)).toEqual({ ask: 14, ignored: 1 });
  });

  it('keeps the cheapest ask when it is believable, at the third exactly too', () => {
    expect(reasonableLowestAsk([12, 14], 18)).toEqual({ ask: 12, ignored: null });
    expect(reasonableLowestAsk([6, 14], 18)).toEqual({ ask: 6, ignored: null });
  });

  it('treats a book that is low all the way up as a move, not a troll', () => {
    expect(reasonableLowestAsk([3, 4, 5], 30)).toEqual({ ask: 3, ignored: null });
  });

  it('judges nothing without a median or without asks', () => {
    expect(reasonableLowestAsk([1, 14], null)).toEqual({ ask: 1, ignored: null });
    expect(reasonableLowestAsk([1, 14], 0)).toEqual({ ask: 1, ignored: null });
    expect(reasonableLowestAsk([], 18)).toEqual({ ask: null, ignored: null });
  });
});

describe('snapshotEvidence', () => {
  it('reads the rule bases and judges outliers against today\'s median, else the baseline', () => {
    const entry = { low5_avg: 16, low20_avg: 17, avg: 20, median_now: 18, median_90d: 21 } as Parameters<typeof snapshotEvidence>[0];
    expect(snapshotEvidence(entry)).toEqual({ low5Avg: 16, low20Avg: 17, avg48h: 20, median90d: 21, median: 18 });
    expect(snapshotEvidence({ ...entry!, median_now: 0 }).median).toBe(21);
    expect(snapshotEvidence(null)).toEqual({ median: null });
  });
});
