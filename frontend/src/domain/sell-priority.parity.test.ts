// @ts-nocheck - fixture-driven parity check; full TS shapes here would be busy-work.
//
// Desktop ranking is native; hosted browsing and development previews still
// need this counterpart. Both implementations consume the same expected order
// so a one-sided formula change cannot silently alter their recommendations.
import { describe, it, expect } from 'vitest';
import { scoreRow, sellableQty } from './sell-priority.js';
import fixture from '../../../tests/fixtures/sell-priority/cases.json';

describe('sell-priority ranking parity (hosted and preview side)', () => {
  it('ranks the shared fixture into the golden order', () => {
    const ranked = fixture.cases
      .map((c) => {
        const sellable = sellableQty(c.count, c.reserve, c.leveled);
        const { sell_score } = scoreRow({ owned: sellable, m: c.market });
        return { slug: c.slug, sellable, sell_score };
      })
      .filter((r) => r.sellable > 0)
      .sort((a, b) => b.sell_score - a.sell_score)
      .map((r) => r.slug);

    expect(ranked).toEqual(fixture.expected_order);
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
