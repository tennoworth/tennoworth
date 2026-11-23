// @ts-nocheck - vitest runs these as JS-style fixtures; full TS shapes here would be busy-work without catching real bugs.
import { describe, it, expect } from 'vitest';
import { computeResults, computeAvailableTags, computeEmptyReason } from './filter-engine.js';

const baseFilters = () => ({
  minPrice: 0, minOwned: 1, typeFilter: 'all', hideAtLvl: 11,
  activeTags: new Set(), vaultOnly: false, ducatsOnly: false, minVol: 0, minMedian: 0,
  typesAny: [], sparesOnly: false, adviceOnly: false,
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

// score_inventory's answer for each owned row. Scoring itself is native and
// tested there (tests/fixtures/inventory-scoring.json); these tests are about
// the filter cascade and the row it builds around a fact.
const fact = (key, overrides = {}) => ({
  key, sellable: 1, clearing_price: 45, sell_score: 10, patience: false, ducats: null, plat_per_100d: null,
  potential_plat: 50, raw_value: 50, medians_7d: [], median_90d: null, delta_90d_pct: null, timing: 'neutral',
  demand: { usage: null, inherited: false, liquidity: 'unknown', band: null }, ...overrides,
});
const factsFor = (recs, overrides = {}) => new Map([...recs.keys()].map((key) => [key, fact(key, overrides[key])]));
const results = (recs, mkt, filters, overrides, advice, availability) =>
  computeResults(recs, mkt, filters, factsFor(recs, overrides), advice, availability);

describe('computeResults', () => {
  // The rows stay in the module between filter changes, keyed by their inputs.
  it('answers a new inventory, new scores or a new market with their own rows', () => {
    const mkt = market({ a: item(), b: item() });
    const first = owned(rec('a'));
    const firstFacts = factsFor(first);
    expect(computeResults(first, mkt, baseFilters(), firstFacts).map(r => r.slug)).toEqual(['a']);
    const second = owned(rec('a'), rec('b'));
    expect(computeResults(second, mkt, baseFilters(), factsFor(second)).map(r => r.slug)).toEqual(['a', 'b']);
    expect(computeResults(first, mkt, baseFilters(), factsFor(first, { a: { sell_score: 99, sellable: 3 } }))[0].sellable).toBe(3);
    expect(computeResults(first, market({ a: item({ avg: 1 }) }), { ...baseFilters(), minPrice: 5 }, firstFacts)).toEqual([]);
    // The same inputs again, under another filter, read the rows they put.
    expect(computeResults(first, mkt, { ...baseFilters(), minPrice: 60 }, firstFacts)).toEqual([]);
    expect(computeResults(first, mkt, baseFilters(), firstFacts).map(r => r.slug)).toEqual(['a']);
  });

  it('rebuilds a row set it let go of', () => {
    const mkt = market({ a: item() });
    const inventories = Array.from({ length: 8 }, (_, i) => owned(rec('a', { count: i + 1 })));
    const facts = inventories.map(o => factsFor(o));
    inventories.forEach((o, i) => computeResults(o, mkt, baseFilters(), facts[i]));
    expect(computeResults(inventories[0], mkt, { ...baseFilters(), minOwned: 1 }, facts[0])[0].owned).toBe(1);
    expect(computeAvailableTags(inventories[7], market({ a: item({ tags: ['mod'] }) }), baseFilters())).toEqual([['mod', 1]]);
  });

  it('excludes rows below minPrice', () => {
    const out = results(
      owned(rec('a')),
      market({ a: item({ avg: 3 }) }),
      { ...baseFilters(), minPrice: 5 },
    );
    expect(out).toHaveLength(0);
  });

  it('excludes rows the user owns fewer than minOwned copies of', () => {
    const out = results(
      owned(rec('a', { count: 1 })),
      market({ a: item() }),
      { ...baseFilters(), minOwned: 2 },
    );
    expect(out).toHaveLength(0);
  });

  it('excludes rows above hideAtLvl (a leveled copy the user is keeping)', () => {
    const out = results(
      owned(rec('a', { kept_lvl: 5 })),
      market({ a: item() }),
      { ...baseFilters(), hideAtLvl: 5 },
    );
    expect(out).toHaveLength(0);
  });

  it('requires at least one matching tag when tag chips are active', () => {
    const filters = { ...baseFilters(), activeTags: new Set(['prime']) };
    const noTag = results(owned(rec('a')), market({ a: item({ tags: ['mod'] }) }), filters);
    const withTag = results(owned(rec('a')), market({ a: item({ tags: ['prime'] }) }), filters);
    expect(noTag).toHaveLength(0);
    expect(withTag).toHaveLength(1);
  });

  it('vaultOnly keeps only vaulted / vaulting-soon rows', () => {
    const filters = { ...baseFilters(), vaultOnly: true };
    const mkt = market({ a: item(), b: item() }, { vault_status: { a: 'vaulted', b: 'available' } });
    const out = results(owned(rec('a'), rec('b')), mkt, filters);
    expect(out.map((r) => r.slug)).toEqual(['a']);
  });

  it('ducatsOnly excludes subtyped rows and rows with no ducat value', () => {
    const filters = { ...baseFilters(), ducatsOnly: true };
    const mkt = market({ a: item({ ducats: 45 }), b: item({ ducats: null }) });
    const out = results(owned(rec('a'), rec('b', { subtype: 'radiant' })), mkt, filters);
    expect(out.map((r) => r.slug)).toEqual(['a']);
  });

  it('shows only rows the native scorer answered for', () => {
    const recs = owned(rec('a'), rec('b'));
    const facts = new Map([['a', fact('a')]]);
    expect(computeResults(recs, market({ a: item(), b: item() }), baseFilters(), facts).map((r) => r.slug)).toEqual(['a']);
    expect(computeResults(recs, market({ a: item(), b: item() }), baseFilters(), new Map())).toEqual([]);
  });

  it('carries the native score and sorts by it, best first', () => {
    const recs = owned(rec('lo'), rec('hi'));
    const out = results(recs, market({ hi: item(), lo: item() }), baseFilters(), { hi: { sell_score: 125 }, lo: { sell_score: 75 } });
    expect(out.map((r) => [r.slug, r.sell_score])).toEqual([['hi', 125], ['lo', 75]]);
  });

  it('caps the native quantity at the protected allocation and revalues it', () => {
    const recs = owned(rec('a', { count: 5 }));
    const out = results(recs, market({ a: item({ avg: 50, low5_avg: 60 }) }), baseFilters(), { a: { sellable: 4 } }, undefined, new Map([['a', 2]]));
    expect(out[0]).toMatchObject({ sellable: 2, potential_plat: 100, raw_value: 120 });
    const unknown = results(recs, market({ a: item() }), baseFilters(), { a: { sellable: 4 } }, undefined, new Map());
    expect(unknown[0].sellable).toBe(0);
  });
});

describe('Spares preset (typesAny + sparesOnly)', () => {
  const spares = () => ({ ...baseFilters(), typesAny: ['Mods', 'Arcanes'], sparesOnly: true });

  it('keeps only Mods/Arcanes with at least one spare copy', () => {
    const out = results(
      owned(
        rec('hammer_shot', { count: 4, kept_lvl: 5 }),        // ranked copy owned → 4 spares
        rec('acid_shells', { count: 3, kept_lvl: null }),     // no ranked copy → keep one → 2 spares
        rec('lonely_mod', { count: 1, kept_lvl: null }),      // single unranked copy → 0 spares
        rec('arcane_x', { count: 2, type: 'Arcanes' }),       // arcane counts too → 1 spare
        rec('volt_prime_set', { count: 3, type: 'Warframes' }), // wrong type
      ),
      market({ hammer_shot: item(), acid_shells: item(), lonely_mod: item(), arcane_x: item(), volt_prime_set: item() }),
      spares(),
    );
    expect(out.map((r) => r.slug).sort()).toEqual(['acid_shells', 'arcane_x', 'hammer_shot']);
  });

  it('never counts leveled (XP > 0, untradeable) copies as spares', () => {
    const out = results(
      owned(rec('galv_x', { count: 3, kept_lvl: 10, leveled: 3 })),
      market({ galv_x: item() }),
      spares(),
    );
    expect(out).toHaveLength(0);
  });

  it('reports "spares" as the empty reason when duplicates are what is missing', () => {
    const o = owned(rec('a', { count: 1 }), rec('b', { count: 1 }));
    const r = computeEmptyReason(o, market({ a: item(), b: item() }), spares(), 0, 'spares');
    expect(r.kind).toBe('spares');
  });
});

describe('Hold/Sell preset (adviceOnly)', () => {
  it('keeps only rows with a verdict and attaches it to the row', () => {
    const advice = new Map([
      ['gauss_prime_chassis', { advice: 'hold', reasons: ['vaulted 71 d ago'] }],
    ]);
    const out = results(
      owned(rec('gauss_prime_chassis'), rec('ash_prime_blueprint')),
      market({ gauss_prime_chassis: item(), ash_prime_blueprint: item() }),
      { ...baseFilters(), adviceOnly: true },
      undefined,
      advice,
    );
    expect(out).toHaveLength(1);
    expect(out[0].slug).toBe('gauss_prime_chassis');
    expect(out[0].advice).toBe('hold');
    expect(out[0].advice_reasons).toEqual(['vaulted 71 d ago']);
  });

  it('reports "advice" as the empty reason when no verdicts exist', () => {
    const o = owned(rec('a'), rec('b'));
    const r = computeEmptyReason(o, market({ a: item(), b: item() }), { ...baseFilters(), adviceOnly: true }, 0, 'holdsell', new Map());
    expect(r.kind).toBe('advice');
  });

  it('a sell-now verdict replaces the near-low hold instead of sitting beside it', () => {
    const timing = (verdict) => results(
      owned(rec('a')), market({ a: item() }), baseFilters(), { a: { timing: 'hold' } },
      verdict ? new Map([['a', verdict]]) : undefined,
    )[0].timing;
    expect(timing()).toBe('hold');
    expect(timing({ advice: 'hold', reasons: [] })).toBe('hold');
    expect(timing({ advice: 'sell_now', reasons: ['released 9 d ago'] })).toBe('neutral');
  });

  it('rows outside the preset still carry no verdict noise', () => {
    const out = results(owned(rec('a')), market({ a: item() }), baseFilters());
    expect(out[0].advice).toBeNull();
  });
});

describe('computeAvailableTags', () => {
  it('counts tags across rows that pass every clause except the tag clause itself', () => {
    const mkt = market({ a: item({ tags: ['prime'] }), b: item({ tags: ['prime', 'mod'] }) });
    const counts = computeAvailableTags(owned(rec('a'), rec('b')), mkt, baseFilters());
    expect(Object.fromEntries(counts)).toEqual({ prime: 2, mod: 1 });
  });

  it('folds a breed into its species, for counts and for the filter', () => {
    const mkt = market({ a: item({ tags: ['mod', 'huras_kubrow'] }), b: item({ tags: ['mod', 'kubrow', 'sunika_kubrow'] }), c: item({ tags: ['kubrow_claws'] }) });
    const recs = owned(rec('a'), rec('b'), rec('c'));
    expect(Object.fromEntries(computeAvailableTags(recs, mkt, baseFilters()))).toEqual({ mod: 2, kubrow: 2, kubrow_claws: 1 });
    const out = results(recs, mkt, { ...baseFilters(), activeTags: new Set(['kubrow']) });
    expect(out.map((row) => row.slug).sort()).toEqual(['a', 'b']);
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
