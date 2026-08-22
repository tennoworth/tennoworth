import { describe, expect, it } from 'vitest';

import { cheapestPath, humanBuildTime, planBuild, type RecipeEntry } from './build-cost';
import type { Market, MarketItemEntry, OwnedRecord } from './types';

function entry(over: Partial<MarketItemEntry>): MarketItemEntry {
  return { avg: 0, low_sell: 0, top_buy: 0, vol: 40, ratio: 0, buys: 0, sells: 0, ...over };
}

const PARTS = [
  { slug: 'nova_prime_neuroptics', component_name: 'Neuroptics' },
  { slug: 'nova_prime_chassis', component_name: 'Chassis' },
  { slug: 'nova_prime_systems', component_name: 'Systems' },
  { slug: 'nova_prime_blueprint', component_name: 'Blueprint' },
];

const MARKET: Market = {
  items: {
    nova_prime_set: entry({ low_sell: 62, low5_avg: 62, median_90d: 62 }),
    nova_prime_neuroptics: entry({ low_sell: 12, low5_avg: 12, median_90d: 12 }),
    nova_prime_chassis: entry({ low_sell: 7, low5_avg: 7, median_90d: 7 }),
    nova_prime_systems: entry({ low_sell: 9, low5_avg: 9, median_90d: 9 }),
    nova_prime_blueprint: entry({ low_sell: 5, low5_avg: 5, median_90d: 5 }),
  },
} as unknown as Market;

const RECIPES: Record<string, RecipeEntry> = {
  nova_prime_neuroptics: {
    build_price: 15000,
    build_time: 43200,
    rush_price: 25,
    ingredients: [{ name: 'Orokin Cell', count: 2 }],
  },
  nova_prime_chassis: { build_price: 15000, build_time: 43200, rush_price: 25 },
  nova_prime_systems: {
    build_price: 15000,
    build_time: 43200,
    rush_price: 25,
    ingredients: [{ name: 'Argon Crystal', count: 1 }],
  },
  nova_prime_set: { build_price: 25000, build_time: 259200, rush_price: 50 },
};

function ownedMap(entries: Array<[string, number]>): Map<string, OwnedRecord> {
  return new Map(
    entries.map(([slug, count]) => [slug, { slug, name: slug, count } as unknown as OwnedRecord]),
  );
}

describe('planBuild', () => {
  it('splits parts into held and missing', () => {
    const plan = planBuild(
      'nova_prime_set',
      'Nova Prime',
      PARTS,
      MARKET,
      ownedMap([
        ['nova_prime_systems', 1],
        ['nova_prime_blueprint', 2],
      ]),
      RECIPES,
    );
    expect(plan.have.map((h) => h.slug)).toEqual(['nova_prime_systems', 'nova_prime_blueprint']);
    expect(plan.missing.map((m) => m.slug)).toEqual([
      'nova_prime_neuroptics',
      'nova_prime_chassis',
    ]);
  });

  it('costs the build path from the missing parts only', () => {
    const plan = planBuild(
      'nova_prime_set',
      'Nova Prime',
      PARTS,
      MARKET,
      ownedMap([
        ['nova_prime_systems', 1],
        ['nova_prime_blueprint', 1],
      ]),
      RECIPES,
    );
    const build = plan.paths.find((p) => p.kind === 'buy-parts-build')!;
    expect(build.plat).toBe(19); // 12 + 7
    expect(build.savingVsSet).toBe(43); // 62 - 19
  });

  it('sums credits and foundry time across every recipe in the set', () => {
    const plan = planBuild('nova_prime_set', 'Nova Prime', PARTS, MARKET, null, RECIPES);
    const build = plan.paths.find((p) => p.kind === 'buy-parts-build')!;
    // three components at 15k + the set at 25k
    expect(build.credits).toBe(70000);
    expect(build.seconds).toBe(43200 * 3 + 259200);
  });

  it('lists resources it could not verify instead of ignoring them', () => {
    const plan = planBuild('nova_prime_set', 'Nova Prime', PARTS, MARKET, null, RECIPES);
    const build = plan.paths.find((p) => p.kind === 'buy-parts-build')!;
    expect(build.unverified.map((i) => i.name)).toEqual(['Orokin Cell', 'Argon Crystal']);
  });

  it('prices the rush path above the build path by the rush plat', () => {
    const plan = planBuild('nova_prime_set', 'Nova Prime', PARTS, MARKET, null, RECIPES);
    const build = plan.paths.find((p) => p.kind === 'buy-parts-build')!;
    const rush = plan.paths.find((p) => p.kind === 'buy-parts-rush')!;
    expect(rush.plat - build.plat).toBe(125); // 25 × 3 + 50
    expect(rush.seconds).toBe(0);
  });

  it('offers selling spares, which the other paths hide', () => {
    const plan = planBuild(
      'nova_prime_set',
      'Nova Prime',
      PARTS,
      MARKET,
      ownedMap([['nova_prime_neuroptics', 3]]),
      RECIPES,
    );
    const sell = plan.paths.find((p) => p.kind === 'sell-spares')!;
    // Two spares beyond the one the set needs.
    expect(sell.plat).toBe(-24);
  });

  it('flags itself incomplete when a missing part has no price', () => {
    const thin = { items: { ...MARKET.items, nova_prime_chassis: entry({}) } } as unknown as Market;
    const plan = planBuild('nova_prime_set', 'Nova Prime', PARTS, thin, null, RECIPES);
    expect(plan.incomplete).toBe(true);
  });

  it('works with no recipes at all, on a snapshot that predates them', () => {
    const plan = planBuild('nova_prime_set', 'Nova Prime', PARTS, MARKET, null, null);
    const build = plan.paths.find((p) => p.kind === 'buy-parts-build')!;
    expect(build.credits).toBe(0);
    expect(build.seconds).toBe(0);
    expect(plan.paths.some((p) => p.kind === 'buy-parts-rush')).toBe(false);
  });
});

describe('cheapestPath', () => {
  it('picks buying parts when it beats the set', () => {
    const plan = planBuild('nova_prime_set', 'Nova Prime', PARTS, MARKET, null, RECIPES);
    expect(cheapestPath(plan)?.kind).toBe('buy-parts-build');
  });

  it('picks the set when the parts cost more than it does', () => {
    const inverted = {
      items: {
        ...MARKET.items,
        nova_prime_set: entry({ low_sell: 20, low5_avg: 20, median_90d: 20 }),
      },
    } as unknown as Market;
    const plan = planBuild('nova_prime_set', 'Nova Prime', PARTS, inverted, null, RECIPES);
    expect(cheapestPath(plan)?.kind).toBe('buy-set');
  });

  it('refuses to recommend anything when a part is unpriced', () => {
    // A "43p cheaper" computed with a part silently valued at 0 is exactly the
    // confidently-wrong output this feature exists to avoid.
    const thin = { items: { ...MARKET.items, nova_prime_chassis: entry({}) } } as unknown as Market;
    const plan = planBuild('nova_prime_set', 'Nova Prime', PARTS, thin, null, RECIPES);
    expect(cheapestPath(plan)).toBeNull();
  });

  it('never recommends selling spares as an acquisition path', () => {
    const plan = planBuild(
      'nova_prime_set',
      'Nova Prime',
      PARTS,
      MARKET,
      ownedMap([['nova_prime_neuroptics', 5]]),
      RECIPES,
    );
    expect(cheapestPath(plan)?.kind).not.toBe('sell-spares');
  });
});

describe('humanBuildTime', () => {
  it('reads in the units the foundry shows', () => {
    expect(humanBuildTime(0)).toBe('instant');
    expect(humanBuildTime(43200)).toBe('12h');
    expect(humanBuildTime(259200)).toBe('3d');
    expect(humanBuildTime(259200 + 43200)).toBe('3d 12h');
  });
});
