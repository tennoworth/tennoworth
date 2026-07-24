// @ts-nocheck — vitest runs these as JS-style fixtures; full TS shapes here would be busy-work without catching real bugs.
import { describe, it, expect } from 'vitest';
import { computeResults, computeAvailableTags, computeEmptyReason } from './filter-engine.js';

const baseFilters = () => ({
  minPrice: 0, minOwned: 1, typeFilter: 'all', hideAtLvl: 11,
  activeTags: new Set(), vaultOnly: false, ducatsOnly: false, minVol: 0, minMedian: 0,
});

function rec(slug, overrides = {}) {
  return { count: 1, name: slug, type: 'Mods', slug, subtype: null, kept_lvl: null, leveled: 0, ...overrides };
}

function owned(...recs) {
  return new Map(recs.map((r) => [r.slug, r]));
}

function market(items, extra = {}) {
  return { items, ...extra };
}

const item = (overrides = {}) => ({ avg: 50, low_sell: 45, top_buy: 40, vol: 10, ratio: 1, ...overrides });

describe('computeResults', () => {
  it('excludes rows below minPrice', () => {
    const out = computeResults(
      owned(rec('a')),
      market({ a: item({ avg: 3 }) }),
      { ...baseFilters(), minPrice: 5 },
      0,
    );
    expect(out).toHaveLength(0);
  });

  it('excludes rows the user owns fewer than minOwned copies of', () => {
    const out = computeResults(
      owned(rec('a', { count: 1 })),
      market({ a: item() }),
      { ...baseFilters(), minOwned: 2 },
      0,
    );
    expect(out).toHaveLength(0);
  });

  it('excludes rows above hideAtLvl (a leveled copy the user is keeping)', () => {
    const out = computeResults(
      owned(rec('a', { kept_lvl: 5 })),
      market({ a: item() }),
      { ...baseFilters(), hideAtLvl: 5 },
      0,
    );
    expect(out).toHaveLength(0);
  });

  it('requires at least one matching tag when tag chips are active', () => {
    const filters = { ...baseFilters(), activeTags: new Set(['prime']) };
    const noTag = computeResults(owned(rec('a')), market({ a: item({ tags: ['mod'] }) }), filters, 0);
    const withTag = computeResults(owned(rec('a')), market({ a: item({ tags: ['prime'] }) }), filters, 0);
    expect(noTag).toHaveLength(0);
    expect(withTag).toHaveLength(1);
  });

  it('vaultOnly keeps only vaulted / vaulting-soon rows', () => {
    const filters = { ...baseFilters(), vaultOnly: true };
    const mkt = market({ a: item(), b: item() }, { vault_status: { a: 'vaulted', b: 'available' } });
    const out = computeResults(owned(rec('a'), rec('b')), mkt, filters, 0);
    expect(out.map((r) => r.slug)).toEqual(['a']);
  });

  it('ducatsOnly excludes subtyped rows and rows with no ducat value', () => {
    const filters = { ...baseFilters(), ducatsOnly: true };
    const mkt = market({ a: item({ ducats: 45 }), b: item({ ducats: null }) });
    const out = computeResults(owned(rec('a'), rec('b', { subtype: 'radiant' })), mkt, filters, 0);
    expect(out.map((r) => r.slug)).toEqual(['a']);
  });

  it('builds a row with sellable quantity net of the reserve-copies hold-back', () => {
    const out = computeResults(owned(rec('a', { count: 5 })), market({ a: item() }), baseFilters(), 2);
    expect(out).toHaveLength(1);
    expect(out[0]).toMatchObject({ slug: 'a', owned: 5, sellable: 3 });
  });

  it('sorts by sell_score, best first', () => {
    const mkt = market({ hi: item({ avg: 100, low_sell: 100, vol: 20 }), lo: item({ avg: 10, low_sell: 10, vol: 20 }) });
    const out = computeResults(owned(rec('lo'), rec('hi')), mkt, baseFilters(), 0);
    expect(out.map((r) => r.slug)).toEqual(['hi', 'lo']);
  });
});

describe('computeAvailableTags', () => {
  it('counts tags across rows that pass every clause except the tag clause itself', () => {
    const mkt = market({ a: item({ tags: ['prime'] }), b: item({ tags: ['prime', 'mod'] }) });
    const counts = computeAvailableTags(owned(rec('a'), rec('b')), mkt, baseFilters());
    expect(Object.fromEntries(counts)).toEqual({ prime: 2, mod: 1 });
  });

  it('still respects the non-tag clauses (e.g. minPrice)', () => {
    const filters = { ...baseFilters(), minPrice: 60 };
    const mkt = market({ a: item({ avg: 10, tags: ['prime'] }) });
    const counts = computeAvailableTags(owned(rec('a')), mkt, filters);
    expect(counts).toHaveLength(0);
  });
});

describe('computeEmptyReason', () => {
  it('returns null when there are already results', () => {
    expect(computeEmptyReason(owned(rec('a')), market({ a: item() }), baseFilters(), 3, null)).toBeNull();
  });

  it('returns null when nothing is owned', () => {
    expect(computeEmptyReason(owned(), market({}), baseFilters(), 0, null)).toBeNull();
  });

  it('reports no-market when no owned slug has a market entry', () => {
    const reason = computeEmptyReason(owned(rec('missing')), market({}), baseFilters(), 0, null);
    expect(reason).toEqual({ kind: 'no-market', candidates: 0 });
  });

  it('names the clause excluding the most candidates', () => {
    const filters = { ...baseFilters(), minPrice: 999 };
    const mkt = market({ a: item({ avg: 5 }), b: item({ avg: 5 }) });
    const reason = computeEmptyReason(owned(rec('a'), rec('b')), mkt, filters, 0, 'default');
    expect(reason).toMatchObject({ kind: 'price', excluded: 2, candidates: 2, preset: 'default' });
  });
});
