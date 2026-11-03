import { describe, expect, it } from 'vitest';
import { fitNote, fitStep, hidesAt, MAX_DROP_RANK } from './table-fit';

describe('table fit steps', () => {
  it('takes the first step that fits, and the last when none does', () => {
    expect(fitStep(() => true, MAX_DROP_RANK + 1)).toBe(0);
    expect(fitStep((step) => step >= 2, MAX_DROP_RANK + 1)).toBe(2);
    expect(fitStep(() => false, MAX_DROP_RANK + 1)).toBe(MAX_DROP_RANK + 1);
  });

  it('hides nothing before step 2, then one more rank per step', () => {
    expect([0, 1, 2, 3, 4].map((step) => hidesAt(1, step))).toEqual([false, false, true, true, true]);
    expect([0, 1, 2, 3, 4].map((step) => hidesAt(3, step))).toEqual([false, false, false, false, true]);
  });

  it('never hides an unranked column', () => {
    expect(hidesAt(0, MAX_DROP_RANK + 1)).toBe(false);
    expect(hidesAt(Number.NaN, MAX_DROP_RANK + 1)).toBe(false);
  });

  it('names what is hidden, and says nothing when nothing is', () => {
    expect(fitNote([])).toBeUndefined();
    expect(fitNote(['Trend'])).toBe('Hidden to fit this window: Trend. Widen the window to show it.');
    expect(fitNote(['Trend', 'Δ 90d'])).toBe('Hidden to fit this window: Trend, Δ 90d. Widen the window to show them.');
  });
});
