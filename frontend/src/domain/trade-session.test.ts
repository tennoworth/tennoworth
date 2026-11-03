import { describe, it, expect } from 'vitest';
import { validSessionLot, tradesToSell, splitByTrades } from './trade-session';
import { MAX_PER_TRADE } from './limits';
import { ALLOWANCE_CHANGED_EVENT } from '../contracts/events';
import lots from '../../../tests/fixtures/trade-session/lots.json';
import events from '../../../tests/fixtures/trade-session/events.json';

// Selection itself is native (market_domain::trade_session, which reads the
// modes/sets/selector fixtures); the review modal still checks lots locally.
describe('Trade Session', () => {
  it('pins the Rust event name', () => expect(ALLOWANCE_CHANGED_EVENT).toBe(events.allowance_changed));
  for (const row of lots.filter(r => r.per_trade != null)) {
    it(row.name, () => expect(validSessionLot(row.quantity, row.per_trade!, row.bulk_tradable)).toBe(row.valid));
  }
  it('invalid lots cannot create invalid trade arithmetic', () => {
    for (const lot of [0, -1, NaN, Infinity, 1.5, 7]) expect(validSessionLot(12, lot, true)).toBe(false);
  });

  it('counts trades by the lot rule: bulk items six at a time, others one each', () => {
    expect(tradesToSell(9, true)).toBe(2);
    expect(tradesToSell(9, false)).toBe(9);
    expect(tradesToSell(6, true)).toBe(1);
    expect(tradesToSell(0, true)).toBe(0);
    expect(tradesToSell(2.5, false)).toBe(0);
    // The count implies a lot the planner accepts for every whole quantity.
    for (const bulk of [true, false]) for (let q = 1; q <= 30; q++) {
      const lot = bulk ? Math.min(MAX_PER_TRADE, q) : 1;
      expect(tradesToSell(q, bulk)).toBeGreaterThanOrEqual(Math.ceil(q / lot));
      expect(validSessionLot(lot, lot, bulk)).toBe(true);
    }
  });

  it('splits at the trades left without reordering, and not at all when the allowance is unknown', () => {
    const rows = [{ n: 'a', t: 2 }, { n: 'b', t: 3 }, { n: 'c', t: 1 }];
    const by = (r: { t: number }) => r.t;
    expect(splitByTrades(rows, by, 4)).toEqual({ today: [rows[0]], later: [rows[1], rows[2]], used: 2, laterTrades: 4 });
    expect(splitByTrades(rows, by, 6)).toEqual({ today: rows, later: [], used: 6, laterTrades: 0 });
    expect(splitByTrades(rows, by, 0).today).toEqual([]);
    expect(splitByTrades(rows, by, null)).toEqual({ today: rows, later: [], used: 6, laterTrades: 0 });
  });
});

