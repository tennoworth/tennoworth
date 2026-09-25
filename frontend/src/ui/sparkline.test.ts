import { describe, it, expect } from 'vitest';
import { hasSparkline, sparklineGeometry } from './sparkline.js';

const ys = (points: string) => points.split(' ').map((p) => Number(p.split(',')[1]));
const xs = (points: string) => points.split(' ').map((p) => Number(p.split(',')[0]));

describe('sparklineGeometry', () => {
  it('returns null for fewer than 2 points', () => {
    for (const arr of [null, undefined, [], [5]]) {
      expect(hasSparkline(arr)).toBe(false);
      expect(sparklineGeometry(arr, 60, 18)).toBeNull();
    }
  });

  it('spans the width up to the right inset across n-1 segments', () => {
    const g = sparklineGeometry([1, 2, 3, 4], 60, 18, null, 2)!;
    expect(xs(g.points)).toEqual([0, 58 / 3, (58 / 3) * 2, 58].map((x) => Number(x.toFixed(1))));
    expect(g.end.x).toBe(58);
  });

  it('does not divide by zero for a flat series', () => {
    const g = sparklineGeometry([5, 5, 5], 60, 18)!;
    expect(g.points).not.toMatch(/NaN|Infinity/);
  });

  it('maps the max value to the top of the band and the min to the bottom', () => {
    const [low, high] = ys(sparklineGeometry([0, 100], 60, 18)!.points);
    expect(low).toBeCloseTo(17, 5);
    expect(high).toBeCloseTo(1, 5);
  });

  it('places the endpoint on the last point', () => {
    const g = sparklineGeometry([10, 30, 20], 60, 18)!;
    expect(g.end.y).toBeCloseTo(ys(g.points)[2], 1);
  });

  it('keeps a baseline outside the series on the same scale instead of clamping it', () => {
    // Series 10..20 sits well above a 0 baseline: the baseline takes the bottom
    // of the band and the line stays in the upper half.
    const g = sparklineGeometry([10, 20], 60, 18, 0)!;
    expect(g.baseline).toBeCloseTo(17, 5);
    expect(Math.max(...ys(g.points))).toBeLessThan(9.1);
  });

  it('reports no baseline when none is given', () => {
    expect(sparklineGeometry([1, 2], 60, 18)!.baseline).toBeNull();
  });
});
