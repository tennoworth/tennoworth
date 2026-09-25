// @ts-nocheck
import { describe, it, expect } from 'vitest';
import { assessListing, assessListings, summarize, ownedKey, ownedEvidence } from './listing-health.js';

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

describe('ownedEvidence', () => {
  const owned = new Map([['primed_flow|', 3], ['lith_c5_relic|intact', 2], ['ash_prime_set|', 0]]);
  const composed = new Set(['ash_prime_set']);

  it('reports nothing assessable without a scan', () => {
    expect(ownedEvidence('primed_flow', null, null, composed)).toBeNull();
    expect(ownedEvidence('primed_flow', null, undefined, composed)).toBeNull();
  });

  it('uses a count the scan actually carries, including zero', () => {
    expect(ownedEvidence('primed_flow', null, owned, composed)).toBe(3);
    // Zero is evidence: the scan looked at this slug and found none.
    expect(ownedEvidence('ash_prime_set', null, owned, composed)).toBe(0);
  });

  it('honours relic refinement when keying', () => {
    expect(ownedEvidence('lith_c5_relic', 'intact', owned, composed)).toBe(2);
    expect(ownedEvidence('lith_c5_relic', 'radiant', owned, composed)).toBe(0);
  });

  it('treats an absent composed item as unassessable, not as zero owned', () => {
    // A set is assembled from parts, so a scan of items never reports it:
    // absent from the map is no evidence either way.
    const withoutSet = new Map([['ash_prime_blueprint|', 1]]);
    expect(ownedEvidence('ash_prime_set', null, withoutSet, composed)).toBeNull();
    // When the map does carry the set, that evidence is used as it stands.
    expect(ownedEvidence('ash_prime_set', null, owned, composed)).toBe(0);
  });

  it('still reports an ordinary item as unowned when the scan does not list it', () => {
    // Only composed items get the benefit of the doubt; absent really does mean
    // absent for everything a scan can see.
    expect(ownedEvidence('primed_flow', null, new Map(), composed)).toBe(0);
    expect(ownedEvidence('lith_c5_relic', 'radiant', new Map(), composed)).toBe(0);
  });

  it('assesses no absence until the market says which items are composed', () => {
    // Before the snapshot loads, a listed set looks like any other absent item.
    expect(ownedEvidence('ash_prime_set', null, new Map(), null)).toBeNull();
    expect(ownedEvidence('primed_flow', null, new Map(), null)).toBeNull();
    // A count the scan carries does not depend on the market.
    expect(ownedEvidence('primed_flow', null, owned, null)).toBe(3);
  });
});
