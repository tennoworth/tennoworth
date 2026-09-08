// @ts-nocheck - shared JSON intentionally includes malformed/nonfinite sentinels.
import { describe, expect, it } from 'vitest';
import fixture from '../../../tests/fixtures/usage-weighted-score/cases.json';
import { scoreRow, sellableQty, usageWeight, usageWeightTier } from './sell-priority';

function share(value: unknown): unknown {
  if (value === 'NaN') return Number.NaN;
  if (value === 'Infinity') return Number.POSITIVE_INFINITY;
  return value;
}

describe('usage-weighted sell score parity', () => {
  it('matches every exact shared score, tier, and final order', () => {
    const rows = fixture.cases.map((testCase) => {
      const usageShare = share(testCase.usage_share);
      const sellable = sellableQty(testCase.count, testCase.reserve, testCase.leveled);
      const neutral = scoreRow({ owned: sellable, m: testCase.market }).sell_score;
      const weighted = scoreRow({ owned: sellable, m: testCase.market, usageShare }).sell_score;
      expect(neutral, testCase.id).toBe(testCase.expected_base_score);
      expect(usageWeight(usageShare), testCase.id).toBe(testCase.expected_weight);
      expect(usageWeightTier(usageShare), testCase.id).toBe(testCase.expected_tier);
      expect(weighted, testCase.id).toBe(testCase.expected_score);
      return { id: testCase.id, sellable, weighted };
    });
    const order = rows.filter((row) => row.sellable > 0)
      .sort((a, b) => b.weighted - a.weighted || a.id.localeCompare(b.id))
      .map((row) => row.id);
    expect(order).toEqual(fixture.expected_order);
    expect(order.indexOf('flip_usage_winner')).toBeLessThan(order.indexOf('flip_base_leader'));
  });
});
