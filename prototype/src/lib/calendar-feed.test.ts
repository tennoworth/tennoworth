import { describe, expect, it } from 'vitest';

import { affecting, buildCalendar, vaultAffects } from './calendar-feed';
import type { Market, OwnedRecord, VaultRotation } from './types';

const NOW = Date.parse('2026-08-22T00:00:00Z');

function owned(slugs: string[]): Map<string, OwnedRecord> {
  return new Map(
    slugs.map((slug) => [
      `${slug}|`,
      { slug, name: slug, count: 1, subtype: null } as unknown as OwnedRecord,
    ]),
  );
}

const MARKET: Market = {
  baro: {
    activation: '2026-08-25T13:00:00Z',
    expiry: '2026-08-27T13:00:00Z',
    location: 'Pluto Relay',
    inventory: [
      { item: 'Primed Fury', slug: 'primed_fury', ducats: 350 },
      { item: "Ki'Teer Sekhara", ducats: 200 },
    ],
  },
  calendar: {
    primes: {
      revenant_prime_set: { name: 'Revenant Prime', vaulted: true },
      nidus_prime_set: { name: 'Nidus Prime', vaulted: true },
      volt_prime_set: { name: 'Volt Prime', vaulted: false },
    },
  },
  set_to_parts: {
    nidus_prime_set: {
      name: 'Nidus Prime',
      parts: [{ slug: 'nidus_prime_systems', component_name: 'Systems' }],
    },
  },
  de: {
    vault_rotation: [
      {
        activation: '2026-08-28T18:00:00Z',
        expiry: '2026-09-25T18:00:00Z',
        items: [
          '/Lotus/Types/StoreItems/Packages/MegaPrimeVault/MPVRevenantPrimeSinglePack',
          '/Lotus/Types/StoreItems/Packages/MegaPrimeVault/MPVNidusPrimeSinglePack',
        ],
      },
    ],
    deals: [
      {
        item: 'Lex',
        expiry: '2026-08-23T00:00:00Z',
        discount: 40,
        sale_price: 114,
      },
    ],
  },
} as unknown as Market;

describe('vaultAffects', () => {
  const rotation = MARKET.de!.vault_rotation![0] as VaultRotation;

  it('matches a held set against the bundle SKU that names it', () => {
    expect(vaultAffects(rotation, MARKET, owned(['revenant_prime_set']))).toEqual([
      'revenant_prime_set',
    ]);
  });

  it('matches when only a PART of the set is held', () => {
    expect(vaultAffects(rotation, MARKET, owned(['nidus_prime_systems']))).toEqual([
      'nidus_prime_set',
    ]);
  });

  it('does not match a set the rotation does not name', () => {
    // Telling somebody their Volt Prime is about to be unvaulted when it is
    // not is worse than saying nothing.
    expect(vaultAffects(rotation, MARKET, owned(['volt_prime_set']))).toEqual([]);
  });

  it('is empty without an inventory or a prime calendar', () => {
    expect(vaultAffects(rotation, MARKET, null)).toEqual([]);
    expect(vaultAffects(rotation, { items: {} } as unknown as Market, owned(['x']))).toEqual([]);
  });
});

describe('buildCalendar', () => {
  it('orders by start date, soonest first', () => {
    const items = buildCalendar(MARKET, owned([]), NOW);
    expect(items.map((i) => i.kind)).toEqual(['deal', 'baro', 'vault']);
  });

  it('drops events that have already finished', () => {
    // A calendar showing last week's Baro teaches people to ignore it.
    const later = Date.parse('2026-09-30T00:00:00Z');
    expect(buildCalendar(MARKET, owned([]), later)).toEqual([]);
  });

  it('keeps Baro until his window closes, not until it opens', () => {
    const midVisit = Date.parse('2026-08-26T00:00:00Z');
    const kinds = buildCalendar(MARKET, owned([]), midVisit).map((i) => i.kind);
    expect(kinds).toContain('baro');
  });

  it('marks what it holds that Baro is selling', () => {
    const items = buildCalendar(MARKET, owned(['primed_fury']), NOW);
    const baro = items.find((i) => i.kind === 'baro')!;
    expect(baro.affects).toEqual(['primed_fury']);
    expect(baro.affectsKnown).toBe(true);
  });

  it('distinguishes "touches nothing" from "we cannot tell"', () => {
    // Without an inventory the reach is unknown, and an empty list must not
    // read as reassurance.
    const items = buildCalendar(MARKET, null, NOW);
    const baro = items.find((i) => i.kind === 'baro')!;
    expect(baro.affects).toEqual([]);
    expect(baro.affectsKnown).toBe(false);

    const deal = items.find((i) => i.kind === 'deal')!;
    expect(deal.affectsKnown).toBe(true);
  });

  it('is empty rather than throwing on a snapshot with none of this', () => {
    expect(buildCalendar({ items: {} } as unknown as Market, null, NOW)).toEqual([]);
    expect(buildCalendar(null, null, NOW)).toEqual([]);
  });
});

describe('affecting', () => {
  it('keeps only rows that touch a holding, and drops unknown reach', () => {
    const items = buildCalendar(MARKET, owned(['primed_fury', 'revenant_prime_set']), NOW);
    const hot = affecting(items);
    expect(hot.map((i) => i.kind).sort()).toEqual(['baro', 'vault']);
  });

  it('is empty when nothing the user holds is involved', () => {
    expect(affecting(buildCalendar(MARKET, owned(['something_else']), NOW))).toEqual([]);
  });
});
