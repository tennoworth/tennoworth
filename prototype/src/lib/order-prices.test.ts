import { describe, it, expect } from 'vitest';
import { orderUnitPrice, orderLotPrice, cachedUnitMarket } from './order-prices';
import cases from '../../../tests/fixtures/trade-session/prices.json';

describe('WFM unit and lot prices', () => {
  for (const c of cases) it(`${c.platinum}p / ${c.per_trade ?? 1} units`, () => {
    expect(orderUnitPrice(c.platinum, c.per_trade as number | null)).toBe(c.unit);
    if (c.unit != null) expect(orderLotPrice(c.unit, c.per_trade as number | null)).toBe(c.platinum);
  });
  it('does not present legacy bulk totals as unit asks or bids', () => {
    const m = { avg: 8, median_now: 8, low_sell: 48, top_buy: 42, low5_avg: 48,
      vol: 100, buys: 10, sells: 10, ratio: 1, tags: ['arcane_enhancement'] };
    expect(cachedUnitMarket(m).low_sell).toBe(0);
    expect(cachedUnitMarket(m).median_now).toBe(8);
    expect(cachedUnitMarket({ ...m, price_basis: 'unit' }).low_sell).toBe(48);
  });
});
