import { describe, it, expect } from 'vitest';
import { selectSession, validSessionLot, type SessionCandidate, type SessionMode } from './trade-session';
import { ALLOWANCE_CHANGED_EVENT } from '../contracts/events';
import lots from '../../../tests/fixtures/trade-session/lots.json';
import modes from '../../../tests/fixtures/trade-session/modes.json';
import events from '../../../tests/fixtures/trade-session/events.json';
import sets from '../../../tests/fixtures/trade-session/sets.json';

const inventory: SessionCandidate[] = modes.inventory.map(r => ({
  key: r.slug, slug: r.slug, name: r.slug, owned: r.quantity, sellable: r.quantity,
  leveled: 0, type: 'Mod', hold: false, bulk: r.bulk,
  market: { avg: r.price, low_sell: r.price, median_now: r.price, vol: r.volume, top_buy: 0, buys: 0, sells: 0, ratio: 0 },
}));

describe('Trade Session', () => {
  it('spends each component once when complete sets compete with parts', () => {
    const candidates: SessionCandidate[] = Object.entries(sets.owned).map(([slug, count]) => ({
      ...inventory[0], key: slug, slug, name: slug, owned: count, sellable: count, bulk: false,
      market: { ...inventory[0].market, low_sell: sets.part_prices[slug as keyof typeof sets.part_prices], median_now: sets.part_prices[slug as keyof typeof sets.part_prices] },
    }));
    const set: SessionCandidate = { ...inventory[0], key: 'example_set', slug: 'example_set', name: 'Example Set', owned: 2, sellable: 2,
      bulk: false, components: sets.parts, market: { ...inventory[0].market, low_sell: sets.set_price, median_now: sets.set_price } };
    const plan = selectSession([...candidates, set], 'per-trade', 2);
    const consumed: Record<string, number> = {};
    for (const row of plan.rows) for (const [slug, count] of Object.entries(row.components ?? { [row.slug]: 1 })) consumed[slug] = (consumed[slug] ?? 0) + count * row.quantity;
    for (const [slug, count] of Object.entries(consumed)) expect(count).toBeLessThanOrEqual(sets.owned[slug as keyof typeof sets.owned]);
    expect(plan.rows[0].slug).toBe('example_set');
    expect(plan.trades).toBe(2);
    const setsOnly = selectSession([...candidates, set], 'max', 2);
    expect(setsOnly.rows.map(row => [row.slug, row.quantity])).toEqual([['example_set', sets.expected_sets]]);
    expect(selectSession([...candidates, { ...set, components: { barrel: 7 } }], 'max', 2).rows.every(row => !row.components)).toBe(true);
  });
  it('pins the Rust event name', () => expect(ALLOWANCE_CHANGED_EVENT).toBe(events.allowance_changed));
  for (const row of lots.filter(r => r.per_trade != null)) {
    it(row.name, () => expect(validSessionLot(row.quantity, row.per_trade!, row.bulk_tradable)).toBe(row.valid));
  }
  for (const c of modes.cases) {
    it(`${c.mode} has its documented selection behavior`, () => {
      const plan = selectSession(inventory, c.mode as SessionMode, c.budget);
      expect(plan.rows.map(r => [r.slug, r.quantity, r.per_trade])).toEqual(c.rows);
      expect(plan.total).toBe(c.total);
      expect(plan.trades).toBe(c.budget);
      expect(selectSession([...inventory].reverse(), c.mode as SessionMode, c.budget)).toEqual(plan);
    });
  }
  it('respects protected copies and never treats hold advice as a prohibition', () => {
    const plan = selectSession([{ ...inventory[1], sellable: 3, hold: true }], 'max', 8);
    expect(plan.rows[0].quantity).toBe(3);
    expect(plan.rows[0].reason).toContain('Hold advice');
    expect(selectSession([{ ...inventory[1], sellable: 0 }], 'max', 8).rows).toEqual([]);
  });
  it('stops at the optional target, permits a bundle overshoot, and reports shortfall', () => {
    const covered = selectSession(inventory, 'max', 3, 500);
    expect(covered.trades).toBe(1);
    expect(covered.total).toBe(600);
    expect(covered.shortfall).toBe(0);
    expect(selectSession(inventory, 'max', 1, 1000).shortfall).toBe(400);
  });
  it('excludes missing prices, refinements, synthetic sets, and ambiguous identities', () => {
    const rows = [
      { ...inventory[0], market: { ...inventory[0].market, avg: 0, low_sell: 0, median_now: 0 } },
      { ...inventory[1], subtype: 'radiant' },
      { ...inventory[2], slug: 'example_set' },
      inventory[3], inventory[3],
    ];
    expect(selectSession(rows, 'max', 50).rows).toEqual([]);
  });
  it('keeps missing bids unknown and clamps thin aspirational asks', () => {
    const plan = selectSession([{ ...inventory[2], market: { ...inventory[2].market, low_sell: 2000, median_now: 20 } }], 'max', 1);
    expect(plan.rows[0].bid).toBeNull();
    expect(plan.rows[0].platinum).toBe(30);
    expect(plan.rows[0].reason).toContain('Thin market');
  });
  it('invalid budgets and lots cannot create invalid trade arithmetic', () => {
    for (const budget of [0, -1, NaN, Infinity, 1.5]) expect(selectSession(inventory, 'max', budget).rows).toEqual([]);
    for (const lot of [0, -1, NaN, Infinity, 1.5, 7]) expect(validSessionLot(12, lot, true)).toBe(false);
  });
});
