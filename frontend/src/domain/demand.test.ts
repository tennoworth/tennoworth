import { describe, expect, it } from 'vitest';

import { masteryBand } from './demand';

/** A usage curve peaking at `peak`, with a little mass either side. */
function curve(peak: number, height = 9): number[] {
  return Array.from({ length: 34 }, (_, mr) => {
    const d = Math.abs(mr - peak);
    return Math.max(0, height - d * 1.2);
  });
}

describe('masteryBand', () => {
  it('brackets the peak rather than reporting a single spike', () => {
    const b = masteryBand(curve(10))!;
    expect(b.from).toBeLessThan(10);
    expect(b.to).toBeGreaterThan(10);
  });

  it('separates a low-MR weapon from a high-MR one', () => {
    const low = masteryBand(curve(5))!;
    const high = masteryBand(curve(28))!;
    expect(low.to).toBeLessThan(high.from);
  });

  it('clamps at the ends of the rank range', () => {
    const b = masteryBand(curve(0))!;
    expect(b.from).toBe(0);
    const top = masteryBand(curve(33))!;
    expect(top.to).toBe(33);
  });

  it('is null for an empty curve rather than dividing by zero', () => {
    expect(masteryBand(new Array(34).fill(0))).toBeNull();
    expect(masteryBand([])).toBeNull();
  });
});

