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

const EVENT_MARKET = {
  event_rewards: {
    goals: {
      complete: {
        id: 'complete',
        source: 'goal',
        title: 'Community Goal',
        starts_at: '2026-08-24T00:00:00Z',
        ends_at: '2026-08-30T00:00:00Z',
        completeness: 'complete',
        groups: [{
          kind: 'final',
          rewards: [{ unique: '/Lotus/PrimedFury', name: 'Primed Fury', slug: 'primed_fury', quantity: 1 }],
        }],
      },
      partial: {
        id: 'partial',
        source: 'goal',
        title: 'Milestone Goal',
        starts_at: '2026-08-25T00:00:00Z',
        ends_at: '2026-08-31T00:00:00Z',
        completeness: 'partial',
        groups: [{
          kind: 'milestone',
          threshold: 100,
          rewards: [
            { unique: '/Lotus/RevenantPrimeSet', name: 'Revenant Prime Set', slug: 'revenant_prime_set', quantity: 1 },
            { unique: '/Lotus/Unknown', name: 'Unknown Reward', quantity: 1 },
          ],
        }],
      },
    },
  },
  surface_provenance: {
    'world.goals': {
      disposition: 'used_current',
      attempted_at: '2026-08-22T00:00:00Z',
      data_fetched_at: '2026-08-22T00:00:00Z',
    },
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

  it('does not flag a prime whose name is a substring of the one in the pack', () => {
    // Seven real prime names are letter-substrings of another: Bronco Prime
    // inside Akbronco Prime, Bo Prime inside Limbo Prime, Lex inside Aklex,
    // and four more. A raw containment test flags every one of them.
    const market = {
      calendar: {
        primes: {
          bronco_prime_set: { name: 'Bronco Prime', vaulted: true },
          akbronco_prime_set: { name: 'Akbronco Prime', vaulted: true },
          bo_prime_set: { name: 'Bo Prime', vaulted: true },
          limbo_prime_set: { name: 'Limbo Prime', vaulted: true },
        },
      },
    } as unknown as Market;
    const pack: VaultRotation = {
      activation: '2026-08-28T18:00:00Z',
      items: [
        '/Lotus/Types/StoreItems/Packages/MegaPrimeVault/MPVAkbroncoPrimeSinglePack',
        '/Lotus/Types/StoreItems/Packages/MegaPrimeVault/MPVLimboPrimeSinglePack',
      ],
    };
    const held = owned(['bronco_prime_set', 'bo_prime_set', 'akbronco_prime_set']);
    const hits = vaultAffects(pack, market, held);
    expect(hits).toEqual(['akbronco_prime_set']);
    expect(hits).not.toContain('bronco_prime_set');
    expect(hits).not.toContain('bo_prime_set');
  });

  it('matches a name whose ampersand DE spells out', () => {
    // Cobra & Crane Prime is MPVCobraAndCranePrimeSinglePack. Treating "&" as
    // punctuation drops the word and the two sides stop lining up — a silent
    // miss on a set the user really does hold.
    const market = {
      calendar: {
        primes: {
          cobra_and_crane_prime_set: { name: 'Cobra & Crane Prime', vaulted: true },
        },
      },
    } as unknown as Market;
    const pack: VaultRotation = {
      activation: '2026-08-28T18:00:00Z',
      items: ['/Lotus/Types/StoreItems/Packages/MegaPrimeVault/MPVCobraAndCranePrimeSinglePack'],
    };
    expect(vaultAffects(pack, market, owned(['cobra_and_crane_prime_set']))).toEqual([
      'cobra_and_crane_prime_set',
    ]);
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

  it('uses explicit event dates and asks for a scan when inventory is absent', () => {
    const events = buildCalendar(EVENT_MARKET, null, NOW);
    expect(events.map((item) => [item.title, item.at, item.until, item.reach])).toEqual([
      ['Community Goal', '2026-08-24T00:00:00Z', '2026-08-30T00:00:00Z', 'scan'],
      ['Milestone Goal', '2026-08-25T00:00:00Z', '2026-08-31T00:00:00Z', 'scan'],
    ]);
  });

  it('never turns partial coverage into false reassurance', () => {
    const miss = buildCalendar(EVENT_MARKET, owned(['something_else']), NOW);
    expect(miss.find((item) => item.title === 'Community Goal')?.reach).toBe('none');
    expect(miss.find((item) => item.title === 'Milestone Goal')?.reach).toBe('unknown');

    const hit = buildCalendar(EVENT_MARKET, owned(['revenant_prime_set']), NOW)
      .find((item) => item.title === 'Milestone Goal')!;
    expect(hit.reach).toBe('partial-hits');
    expect(hit.affectsKnown).toBe(false);
    expect(affecting([hit])).toEqual([hit]);
  });

  it('flags each source against its own freshness stamp', () => {
    const market = structuredClone(EVENT_MARKET) as Market;
    market.event_rewards!.events = {
      event: {
        ...market.event_rewards!.goals!.complete,
        id: 'event',
        source: 'event',
        title: 'Event Reward',
      },
    };
    market.surface_provenance!['world.events'] = {
      disposition: 'carried_prior',
      attempted_at: '2026-08-22T00:00:00Z',
      data_fetched_at: '2026-08-01T00:00:00Z',
    };
    const rows = buildCalendar(market, owned([]), NOW);
    expect(rows.find((item) => item.title === 'Community Goal')?.stale).toBe(false);
    expect(rows.find((item) => item.title === 'Event Reward')?.stale).toBe(true);
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
