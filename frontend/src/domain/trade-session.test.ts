import { describe, it, expect } from 'vitest';
import { validSessionLot } from './trade-session';
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
});
