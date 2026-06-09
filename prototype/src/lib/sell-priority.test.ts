// @ts-nocheck — vitest runs these as JS-style fixtures; full TS shapes here would be busy-work without catching real bugs.
import { describe, it, expect } from 'vitest';
import { scoreRow } from './sell-priority.js';

describe('scoreRow', () => {
  it('returns zero score with no market data', () => {
    expect(scoreRow({ owned: 5, m: null })).toEqual({ sell_score: 0, patience: false });
  });

  it('uses low_sell as the clearing price when available', () => {
    const r = scoreRow({ owned: 10, m: { low_sell: 30, avg: 45, vol: 20 } });
    // dailySales = 20/2 = 10, capped by owned=10 → unitsToday = 10
    // score = 10 × 30 = 300
    expect(r.sell_score).toBe(300);
    expect(r.patience).toBe(false);
  });

  it('falls back to avg when low_sell missing', () => {
    const r = scoreRow({ owned: 1, m: { low_sell: 0, avg: 50, vol: 10 } });
    // dailySales = 5, capped by owned=1 → 1 × 50 = 50
    expect(r.sell_score).toBe(50);
  });

  it('caps units at the market absorption rate, not at what you own', () => {
    // owned 100, vol 4 → dailySales = 2, score = 2 × 100p = 200
    const r = scoreRow({ owned: 100, m: { low_sell: 100, avg: 110, vol: 4 } });
    expect(r.sell_score).toBe(200);
  });

  it('caps at what you own when owned is the bottleneck', () => {
    // owned 1, vol 100 → dailySales = 50, capped by owned=1 → 1 × 20 = 20
    const r = scoreRow({ owned: 1, m: { low_sell: 20, avg: 25, vol: 100 } });
    expect(r.sell_score).toBe(20);
  });

  it('flags low-volume items as patience', () => {
    expect(scoreRow({ owned: 5, m: { low_sell: 200, avg: 220, vol: 1 } }).patience).toBe(true);
    expect(scoreRow({ owned: 5, m: { low_sell: 200, avg: 220, vol: 0 } }).patience).toBe(true);
  });

  it('does not zero out completely dead items — they should still rank, just very low', () => {
    const r = scoreRow({ owned: 1, m: { low_sell: 100, avg: 100, vol: 0 } });
    // dailySales floor of 0.05 → 0.05 × 100 = 5
    expect(r.sell_score).toBeCloseTo(5);
    expect(r.patience).toBe(true);
  });

  it('handles missing fields defensively', () => {
    expect(scoreRow({ owned: 0, m: {} })).toEqual({ sell_score: 0, patience: true });
  });
});
