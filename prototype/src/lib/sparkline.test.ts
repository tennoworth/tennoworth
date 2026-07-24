// @ts-nocheck — vitest runs these as JS-style fixtures; full TS shapes here would be busy-work.
import { describe, it, expect } from 'vitest';
import { sparklinePoints } from './sparkline.js';

describe('sparklinePoints', () => {
  it('returns null for fewer than 2 points', () => {
    expect(sparklinePoints(null, 60, 18)).toBeNull();
    expect(sparklinePoints(undefined, 60, 18)).toBeNull();
    expect(sparklinePoints([], 60, 18)).toBeNull();
    expect(sparklinePoints([5], 60, 18)).toBeNull();
  });

  it('spans the full width across n-1 segments', () => {
    const pts = sparklinePoints([1, 2, 3, 4], 60, 18).split(' ');
    expect(pts).toHaveLength(4);
    expect(pts[0].split(',')[0]).toBe('0.0');
    expect(pts[3].split(',')[0]).toBe('60.0');
  });

  it('does not divide by zero for a flat series', () => {
    const pts = sparklinePoints([5, 5, 5], 60, 18);
    expect(pts).not.toContain('NaN');
    expect(pts).not.toContain('Infinity');
  });

  it('maps the max value to the top of the band and the min to the bottom', () => {
    const [low, high] = sparklinePoints([0, 100], 60, 18).split(' ').map((p) => Number(p.split(',')[1]));
    expect(low).toBeCloseTo(17, 0); // min value -> near h-1
    expect(high).toBeCloseTo(1, 0); // max value -> near 1
  });
});
