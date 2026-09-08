// @ts-nocheck
import { describe, it, expect } from 'vitest';
import { assessListing, assessListings, summarize, ownedKey } from './listing-health.js';

const live = (over = {}) => ({
  slug: 'x', sells: [], buys: [], low_sell: null, top_buy: null, own_ask: null, own_bid: null, error: null, ...over,
});
const order = (over = {}) => ({
  id: 'o1', slug: 'x', name: 'X', platinum: 20, quantity: 1, type: 'sell', live: null, owned: null, ...over,
});

describe('assessListing', () => {
  it('flags overpriced when another online ask is lower, suggesting a match (not an undercut)', () => {
    const [i] = assessListing(order({ platinum: 20, live: live({ low_sell: 15, sells: [15, 18] }) }));
    expect(i.kind).toBe('overpriced');
    expect(i.suggested).toBe(15);
    expect(i.why).toContain('15p');
  });

  it('flags underbid when a live buyer bids above the ask', () => {
    const [i] = assessListing(order({ platinum: 10, live: live({ low_sell: 12, top_buy: 11 }) }));
    expect(i.kind).toBe('underbid');
    expect(i.suggested).toBe(11);
  });

  it('is quiet when the ask is competitive', () => {
    expect(assessListing(order({ platinum: 12, live: live({ low_sell: 12, top_buy: 9 }) }))).toEqual([]);
    expect(assessListing(order({ platinum: 12, live: live({ low_sell: 14, top_buy: 9 }) }))).toEqual([]);
  });

  it('ignores live rows that failed, and never assesses buy orders', () => {
    expect(assessListing(order({ platinum: 99, live: live({ low_sell: 1, error: 'HTTP 404' }) }))).toEqual([]);
    expect(assessListing(order({ type: 'buy', platinum: 1, live: live({ low_sell: 5, top_buy: 4 }) }))).toEqual([]);
  });

  it('flags quantity above what the scan found, and ghosts when nothing is owned', () => {
    const [q] = assessListing(order({ quantity: 5, owned: 2 }));
    expect(q.kind).toBe('excess-qty');
    expect(q.suggested).toBe(2);
    const [g] = assessListing(order({ quantity: 1, owned: 0 }));
    expect(g.kind).toBe('not-owned');
    expect(g.suggested).toBe(0);
    expect(assessListing(order({ quantity: 1, owned: null }))).toEqual([]);
    expect(assessListing(order({ quantity: 2, owned: 2 }))).toEqual([]);
  });

  it('a listing can carry a price issue and a quantity issue at once', () => {
    const issues = assessListing(order({ platinum: 20, quantity: 3, owned: 1, live: live({ low_sell: 15 }) }));
    expect(issues.map((i) => i.kind).sort()).toEqual(['excess-qty', 'overpriced']);
  });
});

describe('summarize / ownedKey', () => {
  it('counts by kind', () => {
    const issues = assessListings([
      order({ id: 'a', platinum: 20, live: live({ low_sell: 15 }) }),
      order({ id: 'b', platinum: 5, live: live({ low_sell: 9, top_buy: 7 }) }),
      order({ id: 'c', quantity: 3, owned: 0 }),
    ]);
    expect(summarize(issues)).toEqual({ overpriced: 1, underbid: 1, excessQty: 0, notOwned: 1, total: 3 });
  });
  it('keys owned quantities by slug + refinement, not rank', () => {
    expect(ownedKey('lith_c5_relic', 'intact')).toBe('lith_c5_relic|intact');
    expect(ownedKey('primed_flow', null)).toBe('primed_flow|');
    expect(ownedKey('primed_flow', undefined)).toBe('primed_flow|');
  });
});
