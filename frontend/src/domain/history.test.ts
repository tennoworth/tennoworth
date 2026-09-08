// @ts-nocheck
import { describe, it, expect } from 'vitest';
import { isHistory, dateAt, points, yearStats, weekly } from './history.js';

const series = (median, volume) => ({ median, volume: volume ?? median.map((m) => (m == null ? 0 : 1)) });

describe('history helpers', () => {
  it('isHistory validates the shape', () => {
    expect(isHistory({ start: '2025-08-17', days: 365, items: {} })).toBe(true);
    expect(isHistory({ start: '2025-08-17', items: {} })).toBe(false);
    expect(isHistory(null)).toBe(false);
    expect(isHistory('nope')).toBe(false);
  });

  it('dateAt walks the window from start', () => {
    expect(dateAt({ start: '2025-08-17' }, 0)).toBe('2025-08-17');
    expect(dateAt({ start: '2025-08-17' }, 364)).toBe('2026-08-16');
  });

  it('points skips nulls and keeps indexes', () => {
    expect(points(series([null, 10, null, 12]))).toEqual([[1, 10], [3, 12]]);
  });

  it('yearStats needs enough traded days, then reports latest/baseline/Δ/high/low', () => {
    expect(yearStats(series([10, null, 12]))).toBeNull();
    // 40 days: first 30 at ~20, then ramps to 40
    const m = [...Array(30).fill(20), ...Array(10).fill(0).map((_, i) => 22 + i * 2)];
    const s = yearStats(series(m));
    expect(s.tradedDays).toBe(40);
    expect(s.latest).toBe(40);
    expect(s.latestIdx).toBe(39);
    expect(s.baseline).toBe(20);
    expect(s.deltaPct).toBe(100);
    expect(s.high).toBe(40);
    expect(s.low).toBe(20);
  });

  it('weekly downsamples by bucket median and skips empty buckets', () => {
    const m = [10, 12, 14, 16, 18, 20, 22, null, null, null, null, null, null, null, 30, 30, 30, 30, 30, 30, 30];
    expect(weekly(series(m), 3)).toEqual([16, 30]);
    expect(weekly(series([]), 3)).toEqual([]);
  });
});
