// @ts-nocheck - fixture-driven parity check; full TS shapes here would be busy-work.
//
// The score is native; these are the pieces of its rules the views still
// apply synchronously, pinned to the values the native side uses.
import { describe, it, expect } from 'vitest';
import { clearingPrice, LIQUID_VOL, MAX_PICKS, MIN_PICK_SCORE, selectPicks, sellableQty } from './sell-priority.js';
import fixture from '../../../tests/fixtures/sell-priority/cases.json';
import boundary from '../../../tests/fixtures/sell-priority/liquidity-boundary.json';
import pickFloor from '../../../tests/fixtures/sell-priority/pick-floor.json';

describe('sell-priority rules the views share with the native side', () => {
  it('picks the rows the native digest does', () => {
    expect(MIN_PICK_SCORE).toBe(pickFloor.min_pick_score);
    expect(MAX_PICKS).toBe(pickFloor.max_picks);
    for (const c of pickFloor.cases) {
      expect(selectPicks([{ sellable: c.sellable, patience: c.patience, timing: c.timing, sell_score: c.score }]).length, c.name).toBe(c.pick ? 1 : 0);
    }
  });

  it('draws the thin-book line where the native side does', () => {
    expect(LIQUID_VOL).toBe(boundary.liquid_vol);
    for (const c of boundary.cases) {
      expect(clearingPrice({ vol: c.vol, low_sell: c.low_sell, median_90d: c.median_90d }), `vol ${c.vol}`).toBe(c.clearing);
    }
  });

  it('excludes rows the reserve zeroes out (sellable_qty 0)', () => {
    const zeroed = fixture.cases
      .filter((c) => sellableQty(c.count, c.reserve, c.leveled) === 0)
      .map((c) => c.slug);
    expect(zeroed).toContain('reserve_zeroes');
    for (const slug of zeroed) {
      expect(fixture.expected_order).not.toContain(slug);
    }
  });
});
