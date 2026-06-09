// @ts-nocheck — vitest runs these as JS-style fixtures; full TS shapes here would be busy-work without catching real bugs.
import { describe, it, expect } from 'vitest';
import { deriveSetRecos } from './set-recos.js';

// Helper: build owned Map keyed the same way App.svelte does
// (composite `${slug}|${subtype ?? ''}`).
function owned(...entries) {
  const m = new Map();
  for (const [slug, count] of entries) {
    m.set(`${slug}|`, { slug, subtype: null, count, name: slug });
  }
  return m;
}

function market({ items = {}, sets = {} } = {}) {
  return {
    items,
    set_to_parts: sets,
  };
}

const MESA_SET = {
  mesa_prime_set: {
    name: 'Mesa Prime',
    parts: [
      { slug: 'mesa_prime_blueprint', component_name: 'Blueprint' },
      { slug: 'mesa_prime_chassis_blueprint', component_name: 'Chassis' },
      { slug: 'mesa_prime_neuroptics_blueprint', component_name: 'Neuroptics' },
      { slug: 'mesa_prime_systems_blueprint', component_name: 'Systems' },
    ],
  },
};
const MESA_PRICES = {
  mesa_prime_set: { low_sell: 90 },
  mesa_prime_blueprint: { low_sell: 5 },
  mesa_prime_chassis_blueprint: { low_sell: 10 },
  mesa_prime_neuroptics_blueprint: { low_sell: 15 },
  mesa_prime_systems_blueprint: { low_sell: 18 },
};

describe('deriveSetRecos', () => {
  it('returns nothing when the user owns no parts of any set', () => {
    expect(deriveSetRecos(owned(), market({ items: MESA_PRICES, sets: MESA_SET }))).toEqual([]);
  });

  it('returns nothing when market has no set map', () => {
    expect(deriveSetRecos(owned(['mesa_prime_blueprint', 1]), market({ items: {} }))).toEqual([]);
  });

  it('flags a near-complete set (missing 1 part) when flipping beats selling parts', () => {
    // Own 3 of 4 parts (missing systems). Set sells for 90p, missing
    // costs 18p, owned-parts value 5+10+15 = 30p. Flip net = 90-18=72;
    // versus selling parts as-is = 30p. Flip wins.
    const recos = deriveSetRecos(
      owned(['mesa_prime_blueprint', 1], ['mesa_prime_chassis_blueprint', 1], ['mesa_prime_neuroptics_blueprint', 1]),
      market({ items: MESA_PRICES, sets: MESA_SET }),
    );
    expect(recos.length).toBe(1);
    expect(recos[0].kind).toBe('near-complete');
    expect(recos[0].set_name).toBe('Mesa Prime');
    expect(recos[0].missing.map((p) => p.slug)).toEqual(['mesa_prime_systems_blueprint']);
    expect(recos[0].missing_cost).toBe(18);
    expect(recos[0].net_plat).toBe(72);
  });

  it('suppresses near-complete when set has no live price', () => {
    const priced = { ...MESA_PRICES, mesa_prime_set: { low_sell: 0 } };
    const recos = deriveSetRecos(
      owned(['mesa_prime_blueprint', 1], ['mesa_prime_chassis_blueprint', 1], ['mesa_prime_neuroptics_blueprint', 1]),
      market({ items: priced, sets: MESA_SET }),
    );
    expect(recos).toEqual([]);
  });

  it('flags duplicate sets when user has a complete set + extra parts', () => {
    // Own 2 of every part = 1 set + 4 spare BPs = extras worth 5+10+15+18 = 48p.
    const recos = deriveSetRecos(
      owned(
        ['mesa_prime_blueprint', 2],
        ['mesa_prime_chassis_blueprint', 2],
        ['mesa_prime_neuroptics_blueprint', 2],
        ['mesa_prime_systems_blueprint', 2],
      ),
      market({ items: MESA_PRICES, sets: MESA_SET }),
    );
    expect(recos.length).toBe(1);
    expect(recos[0].kind).toBe('complete-with-extras');
    expect(recos[0].extras).toBe(4);
    expect(recos[0].extras_plat).toBe(5 + 10 + 15 + 18);
  });

  it('flags pure extras when user has duplicates of an incomplete set', () => {
    // Own 3 of one part, 0 of three others. Not complete, can't flip,
    // but the 2 spare copies × 10p = 20p of extras.
    const recos = deriveSetRecos(
      owned(['mesa_prime_chassis_blueprint', 3]),
      market({ items: MESA_PRICES, sets: MESA_SET }),
    );
    expect(recos.length).toBe(1);
    expect(recos[0].kind).toBe('extras');
    expect(recos[0].extras).toBe(2);
    expect(recos[0].extras_plat).toBe(20);
  });

  it('skips relic refinements when summing owned parts', () => {
    const ownedMap = new Map();
    ownedMap.set('axi_k2_relic|intact', { slug: 'axi_k2_relic', subtype: 'intact', count: 10, name: 'Axi K2 (Intact)' });
    expect(deriveSetRecos(ownedMap, market({ items: MESA_PRICES, sets: MESA_SET }))).toEqual([]);
  });

  it('sorts by net_plat desc', () => {
    const sets = {
      ...MESA_SET,
      atlas_prime_set: {
        name: 'Atlas Prime',
        parts: [
          { slug: 'atlas_prime_blueprint', component_name: 'Blueprint' },
          { slug: 'atlas_prime_chassis_blueprint', component_name: 'Chassis' },
        ],
      },
    };
    const items = {
      ...MESA_PRICES,
      atlas_prime_set: { low_sell: 40 },
      atlas_prime_blueprint: { low_sell: 3 },
      atlas_prime_chassis_blueprint: { low_sell: 4 },
    };
    const recos = deriveSetRecos(
      owned(
        // Atlas near-complete: own BP, missing chassis. Net = 40 - 4 = 36
        ['atlas_prime_blueprint', 1],
        // Mesa duplicates: 2 of one part. Extras = 1 × 10 = 10p
        ['mesa_prime_chassis_blueprint', 2],
      ),
      market({ items, sets }),
    );
    expect(recos.map((r) => r.kind)).toEqual(['near-complete', 'extras']);
    expect(recos[0].net_plat).toBeGreaterThan(recos[1].net_plat);
  });
});
