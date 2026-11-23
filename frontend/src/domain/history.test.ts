// @ts-nocheck
import { describe, it, expect } from 'vitest';
import { isHistory, yearViews } from './history.js';

const series = (median, volume) => ({ median, volume: volume ?? median.map((m) => (m == null ? 0 : 1)) });

describe('history helpers', () => {
  it('isHistory validates the shape', () => {
    expect(isHistory({ start: '2025-08-17', days: 365, items: {} })).toBe(true);
    expect(isHistory({ start: '2025-08-17', items: {} })).toBe(false);
    expect(isHistory(null)).toBe(false);
    expect(isHistory('nope')).toBe(false);
  });

  it('stats need enough traded days, then report latest/baseline/Δ/high/low', () => {
    expect(yearViews([series([10, null, 12])])[0].stats).toBeNull();
    // 40 days: first 30 at ~20, then ramps to 40
    const m = [...Array(30).fill(20), ...Array(10).fill(0).map((_, i) => 22 + i * 2)];
    const [{ stats: s }] = yearViews([series(m)]);
    expect(s.tradedDays).toBe(40);
    expect(s.latest).toBe(40);
    expect(s.latestIdx).toBe(39);
    expect(s.baseline).toBe(20);
    expect(s.deltaPct).toBe(100);
    expect(s.high).toBe(40);
    expect(s.low).toBe(20);
  });

  it('weekly downsamples by bucket median and skips empty and non-numeric days', () => {
    const m = [10, 12, 14, 16, 18, 20, 22, null, null, null, null, null, null, null, 30, 30, 30, 30, 30, '9', 30];
    expect(yearViews([series(m), series([])], { buckets: 3 }).map((v) => v.weekly)).toEqual([[16, 30], []]);
    expect(yearViews([])).toEqual([]);
  });

  it('defaults to 52 buckets and 20 traded days', () => {
    const [full, thin] = yearViews([series(Array.from({ length: 104 }, (_, i) => i + 1)), series(Array.from({ length: 19 }, (_, i) => i + 1))]);
    expect(full.weekly).toHaveLength(52);
    expect(full.stats.tradedDays).toBe(104);
    expect(thin.stats).toBeNull();
  });
});
