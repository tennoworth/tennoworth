import { describe, expect, it } from 'vitest';
import { compareBuyers } from './buyer-alternatives';
import type { BuyerOrder, LiveTop } from '../contracts/desktop';
import fixture from '../../../tests/fixtures/buyer-alternatives/book.json';

const now = Date.parse('2026-09-08T12:00:00Z');
const buyer = (id: string, quantity: number, platinum: number, per_trade = 1): BuyerOrder => ({
  id, user_id: id, name: id, user_slug: id, status: 'ingame', platform: 'pc', crossplay: true,
  quantity, platinum, per_trade,
});
const quote = (orders = [buyer('a', 2, 12), buyer('b', 3, 10)]): LiveTop => ({
  slug: 'part', rank: 0, subtype: null, buys: [12, 10], sells: [15], low_sell: 15, top_buy: 12,
  buyer_book: { orders, observed_at: new Date(now).toISOString(), own_orders_excluded: true },
});

describe('quantity-aware buyer alternatives', () => {
  it('consumes the same book identities and lots verified by the native parser', () => {
    const { quantity, ask, ...expected } = fixture.comparison;
    expect(compareBuyers(quote(fixture.orders), quantity, ask, now)).toMatchObject(expected);
  });
  it('does not multiply the top bid across an uncovered stack', () => {
    const result = compareBuyers(quote(), 8, 15, now);
    expect(result).toMatchObject({ error: null, covered: 5, uncovered: 3, value: 54, askValue: 75, gap: 21 });
  });
  it('compares the same covered quantity to the listing reference', () => {
    expect(compareBuyers(quote(), 4, 15, now)).toMatchObject({ covered: 4, uncovered: 0, value: 44, askValue: 60, gap: 16 });
  });
  it('only fills whole lots and retains the matching buyer and lot price', () => {
    const result = compareBuyers(quote([buyer('a', 6, 24, 2), buyer('b', 3, 10)]), 5, 15, now);
    expect(result).toMatchObject({ covered: 5, value: 58, rows: [
      { order: { id: 'a', per_trade: 2 }, quantity: 4, value: 48 },
      { order: { id: 'b', per_trade: 1 }, quantity: 1, value: 10 },
    ] });
  });
  it('leaves a smaller remainder uncovered when no whole lot fits', () => {
    expect(compareBuyers(quote([buyer('a', 6, 24, 2)]), 1, 15, now)).toMatchObject({ covered: 0, uncovered: 1, value: 0 });
  });
  it('distinguishes empty coverage from missing or failed books', () => {
    expect(compareBuyers(quote([]), 3, 15, now)).toMatchObject({ error: null, covered: 0, uncovered: 3 });
    expect(compareBuyers({ ...quote(), buyer_book: null }, 3, 15, now).error).toBeTruthy();
    expect(compareBuyers({ ...quote(), error: 'network failure' }, 3, 15, now).error).toBeTruthy();
  });
  it('does not reuse stale, future-dated or unidentified-self quotes', () => {
    expect(compareBuyers(quote(), 3, 15, now + 60_000).error).toMatch(/stale/);
    expect(compareBuyers(quote(), 3, 15, now - 1).error).toMatch(/stale/);
    const loggedOut = quote();
    loggedOut.buyer_book!.own_orders_excluded = false;
    expect(compareBuyers(loggedOut, 3, 15, now).error).toMatch(/Unlock/);
  });
  it('rejects invalid quantity and excludes duplicate buyer identities', () => {
    expect(compareBuyers(quote(), 1.5, 15, now).error).toBeTruthy();
    expect(compareBuyers(quote(), Infinity, 15, now).error).toBeTruthy();
    expect(compareBuyers(quote([buyer('a', 2, 12), buyer('a', 2, 12)]), 4, 15, now)).toMatchObject({ covered: 2, uncovered: 2 });
  });
});
