// @ts-nocheck — vitest runs these as JS-style fixtures; full TS shapes here would be busy-work without catching real bugs.
import { describe, it, expect, beforeEach, vi } from 'vitest';

// Mock IndexedDB cache so tests don't touch real storage / fetch.
vi.mock('./catalog-cache.js', () => ({
  readCached: vi.fn().mockResolvedValue(null),
  writeCached: vi.fn().mockResolvedValue(undefined),
  clearCached: vi.fn().mockResolvedValue(undefined),
}));

import { loadCatalogs, resolvePath } from './resolver.js';

function fakeCatalogs(entries) {
  return { uniqueToInfo: new Map(entries) };
}

function fakeMarket(catalog = {}, path_to_info = {}) {
  return { catalog, path_to_info };
}

describe('resolvePath', () => {
  it('returns nulls for an unknown path', () => {
    const r = resolvePath('/Lotus/Unknown', fakeCatalogs([]), fakeMarket());
    expect(r).toEqual({ name: null, slug: null, category: null, subtype: null });
  });

  it('looks up direct path -> name -> slug', () => {
    const catalogs = fakeCatalogs([
      ['/Lotus/Frame', { name: 'Zephyr Prime', category: 'Warframes' }],
    ]);
    const market = fakeMarket({ 'zephyr prime': 'zephyr_prime_set' });
    expect(resolvePath('/Lotus/Frame', catalogs, market)).toEqual({
      name: 'Zephyr Prime',
      slug: 'zephyr_prime_set',
      category: 'Warframes',
      subtype: null,
    });
  });

  it('falls back to slug-guessing when name is not in WFM catalog', () => {
    const catalogs = fakeCatalogs([
      ['/Lotus/Weird', { name: "Sister's Cool Hammer", category: 'Melee' }],
    ]);
    const market = fakeMarket();  // empty WFM catalog
    const r = resolvePath('/Lotus/Weird', catalogs, market);
    expect(r.slug).toBe('sisters_cool_hammer');  // punctuation stripped, snake_case
    expect(r.category).toBe('Melee');
  });

  it('strips Component / Blueprint suffix when needed', () => {
    const catalogs = fakeCatalogs([
      ['/Lotus/Foo', { name: 'Foo Prime', category: 'Misc' }],
    ]);
    const market = fakeMarket({ 'foo prime': 'foo_prime' });
    expect(resolvePath('/Lotus/FooComponent', catalogs, market).slug).toBe('foo_prime');
    expect(resolvePath('/Lotus/FooBlueprint', catalogs, market).slug).toBe('foo_prime');
  });

  it('maps each relic refinement to the same slug but keeps the subtype', () => {
    const catalogs = fakeCatalogs([
      ['/Lotus/.../T3VoidProjectionZephyrPrimeBBronze',   { name: 'Neo Z2 Intact',      category: 'Relics' }],
      ['/Lotus/.../T3VoidProjectionZephyrPrimeBSilver',   { name: 'Neo Z2 Exceptional', category: 'Relics' }],
      ['/Lotus/.../T3VoidProjectionZephyrPrimeBGold',     { name: 'Neo Z2 Flawless',    category: 'Relics' }],
      ['/Lotus/.../T3VoidProjectionZephyrPrimeBPlatinum', { name: 'Neo Z2 Radiant',     category: 'Relics' }],
    ]);
    const market = fakeMarket({ 'neo z2 relic': 'neo_z2_relic' });
    const cases = [
      ['/Lotus/.../T3VoidProjectionZephyrPrimeBBronze',   'Neo Z2 Relic (Intact)',      'intact'],
      ['/Lotus/.../T3VoidProjectionZephyrPrimeBSilver',   'Neo Z2 Relic (Exceptional)', 'exceptional'],
      ['/Lotus/.../T3VoidProjectionZephyrPrimeBGold',     'Neo Z2 Relic (Flawless)',    'flawless'],
      ['/Lotus/.../T3VoidProjectionZephyrPrimeBPlatinum', 'Neo Z2 Relic (Radiant)',     'radiant'],
    ];
    for (const [path, expectedName, expectedSubtype] of cases) {
      const r = resolvePath(path, catalogs, market);
      expect(r.slug).toBe('neo_z2_relic');
      expect(r.name).toBe(expectedName);
      expect(r.category).toBe('Relics');
      expect(r.subtype).toBe(expectedSubtype);
    }
  });

  it('does NOT treat a non-relic with a refinement-shaped name as a relic', () => {
    // "Something Intact" without a matching "<base> relic" slug → falls
    // through to the normal lookup.
    const catalogs = fakeCatalogs([
      ['/Lotus/Misc', { name: 'Mysterious Intact', category: 'Misc' }],
    ]);
    const market = fakeMarket({ 'mysterious intact': 'mysterious_intact' });
    const r = resolvePath('/Lotus/Misc', catalogs, market);
    expect(r.slug).toBe('mysterious_intact');
    expect(r.name).toBe('Mysterious Intact');
    expect(r.subtype).toBeNull();
  });

  it('short-circuits on path_to_info for prime-part components warframestat omits', () => {
    // Reproducer: /Lotus/Types/Recipes/.../VoltPrimeChassisComponent is
    // absent from warframestat's bulk /items/ but the scraper pre-walks
    // /warframes and bakes it into market.path_to_info.
    const path = '/Lotus/Types/Recipes/WarframeRecipes/VoltPrimeChassisComponent';
    const market = fakeMarket(
      { 'volt prime chassis': 'volt_prime_chassis' },
      { [path]: { name: 'Volt Prime Chassis', slug: 'volt_prime_chassis', category: 'Warframes' } }
    );
    const r = resolvePath(path, fakeCatalogs([]), market);
    expect(r).toEqual({
      name: 'Volt Prime Chassis',
      slug: 'volt_prime_chassis',
      category: 'Warframes',
      subtype: null,
    });
  });

  it('handles missing market gracefully (no crash, uses slugGuess)', () => {
    const catalogs = fakeCatalogs([
      ['/Lotus/X', { name: 'Test Item', category: 'Misc' }],
    ]);
    const r = resolvePath('/Lotus/X', catalogs, /* market */ null);
    expect(r.slug).toBe('test_item');
  });

  // New primes hit WFM's catalog day-one but lag in warframestat for weeks —
  // the path-derived name guess bridges that window (real case: a brand-new
  // prime's part blueprints sat unresolved while WFM already traded them).
  it('resolves a path absent from BOTH catalogs via a WFM-catalog name guess', () => {
    const path = '/Lotus/Types/Recipes/Weapons/WeaponParts/SagekPrimeBarrelBlueprint';
    const market = fakeMarket({ 'sagek prime barrel': 'sagek_prime_barrel' });
    const r = resolvePath(path, fakeCatalogs([]), market);
    expect(r.slug).toBe('sagek_prime_barrel');
    expect(r.name).toBe('Sagek Prime Barrel');
    expect(r.category).toBeNull(); // caller falls back to the inventory key
  });

  it('name guess is strict — no WFM-catalog hit means still unresolved', () => {
    const path = '/Lotus/Types/Recipes/Components/FormaBlueprint';
    const market = fakeMarket({ 'sagek prime barrel': 'sagek_prime_barrel' });
    const r = resolvePath(path, fakeCatalogs([]), market);
    expect(r).toEqual({ name: null, slug: null, category: null, subtype: null });
  });
});

describe('loadCatalogs', () => {
  let cacheModule;

  beforeEach(async () => {
    cacheModule = await import('./catalog-cache.js');
    cacheModule.readCached.mockReset();
    cacheModule.writeCached.mockReset();
    globalThis.fetch = vi.fn();
  });

  it('returns cached data when present', async () => {
    cacheModule.readCached.mockResolvedValue([
      ['/Lotus/A', { name: 'A', category: 'Misc' }],
    ]);
    const { uniqueToInfo } = await loadCatalogs();
    expect(uniqueToInfo.get('/Lotus/A')).toEqual({ name: 'A', category: 'Misc' });
    expect(globalThis.fetch).not.toHaveBeenCalled();
  });

  it('fetches the baked slim catalog and writes cache when miss', async () => {
    cacheModule.readCached.mockResolvedValue(null);
    // wfstat-catalog.json ships pre-slimmed [uniqueName, info] pairs.
    globalThis.fetch.mockResolvedValue({
      ok: true,
      json: async () => ([
        ['/Lotus/B', { name: 'B', category: 'Misc' }],
        ['/Lotus/C', { name: 'C', category: null }],
      ]),
    });
    const { uniqueToInfo } = await loadCatalogs();
    expect(globalThis.fetch).toHaveBeenCalledWith('/wfstat-catalog.json');
    expect(uniqueToInfo.get('/Lotus/B')).toEqual({ name: 'B', category: 'Misc' });
    expect(uniqueToInfo.get('/Lotus/C')).toEqual({ name: 'C', category: null });
    expect(cacheModule.writeCached).toHaveBeenCalledOnce();
  });

  it('throws a meaningful error on HTTP failure', async () => {
    cacheModule.readCached.mockResolvedValue(null);
    globalThis.fetch.mockResolvedValue({ ok: false, status: 503 });
    await expect(loadCatalogs()).rejects.toThrow(/503/);
  });

  it('rejects a non-array payload instead of resolving garbage', async () => {
    cacheModule.readCached.mockResolvedValue(null);
    globalThis.fetch.mockResolvedValue({ ok: true, json: async () => ({ error: 'nope' }) });
    await expect(loadCatalogs()).rejects.toThrow(/not an array/);
  });
});
