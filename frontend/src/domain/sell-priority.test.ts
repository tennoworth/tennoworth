// @ts-nocheck - vitest runs these as JS-style fixtures; full TS shapes here would be busy-work without catching real bugs.
import { describe, it, expect } from 'vitest';
//
// The rules are native; these run them through the view module, so they also
// check what the wrappers hand it (missing fields, a null kept_lvl, hold rows).
import { clearingPrice, sellableQty, spareQty, selectPicks, MIN_PICK_SCORE } from './sell-priority.js';
import pickFloor from '../../../tests/fixtures/sell-priority/pick-floor.json';

describe('clearingPrice', () => {
  it('uses the live ask when it agrees with the median', () => {
    expect(clearingPrice({ low_sell: 60, median_now: 65, avg: 62, vol: 20 })).toBe(60);
  });

  it('clamps a troll undercut up to the median (akbolto receiver case)', () => {
    // One 1p ask under a stable 38p median: one such listing absorbs one
    // sale; everything else clears near the median.
    expect(clearingPrice({ low_sell: 1, median_now: 38, avg: 30, vol: 54 })).toBe(38);
  });

  it('clamps a thin item’s aspirational ask down to 1.5× median', () => {
    expect(clearingPrice({ low_sell: 2999, median_now: 204, avg: 204, vol: 1 })).toBe(306);
    // vol 2–4 books get the same treatment - they used to dodge the vol<2 gate
    expect(clearingPrice({ low_sell: 100, median_now: 10, avg: 10, vol: 2 })).toBe(15);
    expect(clearingPrice({ low_sell: 100, median_now: 10, avg: 10, vol: 4 })).toBe(15);
  });

  it('keeps a thin item’s ask when it is within 1.5× of the median', () => {
    expect(clearingPrice({ low_sell: 14, median_now: 10, avg: 11, vol: 2 })).toBe(14);
  });

  it('keeps a liquid item’s high ask - there the book is real information', () => {
    expect(clearingPrice({ low_sell: 700, median_now: 200, avg: 250, vol: 30 })).toBe(700);
    // LIQUID_VOL boundary: vol 5 counts as liquid
    expect(clearingPrice({ low_sell: 100, median_now: 10, avg: 10, vol: 5 })).toBe(100);
  });

  it('falls back median → avg → 1 when there is no ask', () => {
    expect(clearingPrice({ low_sell: 0, median_now: 42, avg: 50, vol: 5 })).toBe(42);
    expect(clearingPrice({ low_sell: 0, median_now: 0, avg: 50, vol: 5 })).toBe(50);
    expect(clearingPrice({ low_sell: 0, median_now: 0, avg: 0, vol: 5 })).toBe(1);
  });

  it('reads a missing row or garbage fields as no figure', () => {
    expect(clearingPrice(null)).toBe(1);
    expect(clearingPrice({ low_sell: 'x', median_now: Number.NaN, avg: 7 })).toBe(7);
  });

  it('uses median_90d when median_now is absent (pre-split snapshots)', () => {
    expect(clearingPrice({ low_sell: 1, median_90d: 38, avg: 30, vol: 54 })).toBe(38);
  });
});

describe('sellableQty', () => {
  it('passes owned count through untouched when reserve is 0', () => {
    expect(sellableQty(5, 0)).toBe(5);
    expect(sellableQty(0, 0)).toBe(0);
  });

  it('subtracts the reserve from owned count', () => {
    expect(sellableQty(5, 1)).toBe(4);
    expect(sellableQty(5, 5)).toBe(0);
  });

  it('clamps at 0 rather than going negative when reserve exceeds owned', () => {
    expect(sellableQty(2, 5)).toBe(0);
    expect(sellableQty(0, 3)).toBe(0);
  });

  it('is backward-equivalent to the old 2-arg signature when leveled is 0 (default)', () => {
    expect(sellableQty(5, 1)).toBe(sellableQty(5, 1, 0));
    expect(sellableQty(2, 5)).toBe(sellableQty(2, 5, 0));
  });

  it('leveled copies satisfy the reserve when leveled > reserve', () => {
    // 5 owned, 2 leveled (untradeable), reserve only asks for 1 kept back -
    // the leveled copies already cover that, so sellable = 5 - 2 = 3.
    expect(sellableQty(5, 1, 2)).toBe(3);
  });

  it('the reserve holds back more than the leveled count when reserve > leveled', () => {
    // 5 owned, 1 leveled, but the user wants 3 kept back - reserve wins.
    expect(sellableQty(5, 3, 1)).toBe(2);
  });

  it('holds back everything when all owned copies are leveled', () => {
    expect(sellableQty(4, 0, 4)).toBe(0);
  });

  it('clamps at 0 rather than going negative when leveled exceeds owned', () => {
    expect(sellableQty(3, 0, 5)).toBe(0);
  });
});

describe('selectPicks', () => {
  // Minimal row shape - real callers pass the full `results` row (name,
  // slug, clearing_price, …); selectPicks only reads these and timing.
  function row(overrides = {}) {
    return { sellable: 5, patience: false, sell_score: 50, ...overrides };
  }

  it('returns nothing for empty input', () => {
    expect(selectPicks([])).toEqual([]);
  });

  it('excludes rows below the score floor', () => {
    const rows = [row({ sell_score: 5 }), row({ sell_score: 19.9 })];
    expect(selectPicks(rows)).toEqual([]);
  });

  it('excludes patience rows even when the score is high', () => {
    const rows = [row({ sell_score: 500, patience: true })];
    expect(selectPicks(rows)).toEqual([]);
  });

  it('excludes hold rows, which would sell near their 90-day low', () => {
    const rows = [row({ sell_score: 500, timing: 'hold' }), row({ sell_score: 400, timing: 'peak', key: 'peak' })];
    expect(selectPicks(rows).map((r) => r.key)).toEqual(['peak']);
  });

  it('reads a missing score or quantity as zero', () => {
    expect(selectPicks([row({ sell_score: Number.NaN }), row({ sellable: undefined })])).toEqual([]);
  });

  it('excludes rows with nothing sellable', () => {
    const rows = [row({ sell_score: 500, sellable: 0 })];
    expect(selectPicks(rows)).toEqual([]);
  });

  it('respects an explicit cap', () => {
    const rows = Array.from({ length: 10 }, (_, i) => row({ sell_score: 100 - i, key: i }));
    const picks = selectPicks(rows, { cap: 3 });
    expect(picks).toHaveLength(3);
    expect(picks.map((r) => r.key)).toEqual([0, 1, 2]);
  });

  it('defaults the cap to the native pick count', () => {
    const rows = Array.from({ length: pickFloor.max_picks + 3 }, () => row());
    expect(selectPicks(rows)).toHaveLength(pickFloor.max_picks);
  });

  it('holds the score floor at MIN_PICK_SCORE', () => {
    const rows = [row({ sell_score: MIN_PICK_SCORE - 0.01 }), row({ sell_score: MIN_PICK_SCORE })];
    expect(selectPicks(rows)).toEqual([row({ sell_score: MIN_PICK_SCORE })]);
  });

  it('slices in the given order instead of re-sorting - trusts the caller pre-sorted', () => {
    // Deliberately out of score order. If selectPicks ever re-sorted, this
    // would come back ['high', 'low'] and mask a caller regression instead
    // of surfacing it.
    const rows = [row({ sell_score: 30, key: 'low' }), row({ sell_score: 90, key: 'high' })];
    const picks = selectPicks(rows);
    expect(picks.map((r) => r.key)).toEqual(['low', 'high']);
  });
});

describe('spareQty', () => {
  it('all unranked copies are spare when a ranked copy is owned', () => {
    expect(spareQty(4, 5)).toBe(4);
    expect(spareQty(4, 1)).toBe(4);
  });
  it('keeps one copy when no ranked copy is owned (kept_lvl null or 0)', () => {
    expect(spareQty(4, null)).toBe(3);
    expect(spareQty(4, 0)).toBe(3);
    expect(spareQty(1, null)).toBe(0);
    expect(spareQty(0, null)).toBe(0);
  });
  it('leveled copies are never spare', () => {
    expect(spareQty(3, 5, 3)).toBe(0);
    expect(spareQty(3, 5, 1)).toBe(2);
    expect(spareQty(3, null, 1)).toBe(1);
  });
});
