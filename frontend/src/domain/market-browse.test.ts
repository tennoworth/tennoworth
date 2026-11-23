// @ts-nocheck - hand-built market fixtures; full TS shapes would be busy-work.
import { describe, it, expect } from 'vitest';
import {
  buildBrowseIndex,
  searchItems,
  topMovers,
  vaultedTop,
  dispositionChanges,
  handoffSample,
} from './market-browse.js';
import type { Market } from '../contracts/data';
import collation from '../../../tests/fixtures/collation/ascii.json';

// Minimal snapshot: catalog (name→slug), items (stats), vault_status.
function fixture() {
  return {
    updated_at: '2026-07-19T08:00:00Z',
    catalog: {
      'mag prime set': 'mag_prime_set',
      'primed continuity': 'primed_continuity',
      'primed flow': 'primed_flow',
      'rare thing': 'rare_thing',
      'thin item': 'thin_item',
      'flat item': 'flat_item',
      'no baseline': 'no_baseline',
      'cheap riser': 'cheap_riser',
      'edge riser': 'edge_riser',
      'quest key': 'quest_key', // in catalog but NOT in items
    },
    items: {
      mag_prime_set: { avg: 120, vol: 50, median_now: 60, median_90d: 50, medians_7d: [50, 52, 55, 58, 59, 60, 60] }, // +20%
      primed_continuity: { avg: 40, vol: 200, median_now: 30, median_90d: 40 }, // -25%
      primed_flow: { avg: 35, vol: 300, median_now: 44, median_90d: 40 }, // +10%
      rare_thing: { avg: 500, vol: 40, median_now: 90, median_90d: 100 }, // -10%
      thin_item: { avg: 5, vol: 3, median_now: 30, median_90d: 10 }, // +200% but vol<20
      flat_item: { avg: 10, vol: 100, median_now: 25, median_90d: 25 }, // 0% move
      no_baseline: { avg: 8, vol: 80, median_now: 25, median_90d: 0 }, // no usable baseline
      cheap_riser: { avg: 9.9, vol: 100, median_now: 20, median_90d: 10 }, // +100% but avg<10 floor
      edge_riser: { avg: 10, vol: 100, median_now: 10.5, median_90d: 10 }, // +5%, avg==10 floor
    },
    vault_status: {
      mag_prime_set: 'vaulted',
      rare_thing: 'vaulted',
      primed_flow: 'vaulting-soon',
      primed_continuity: 'available',
    },
  };
}

describe('names', () => {
  it('upper-cases each word, including inside parens, with the ASCII word rule', () => {
    const idx = buildBrowseIndex({ catalog: { 'mutalist alad v assassinate (key)': 'key', 'höllvanian old town': 'town' }, items: { key: {}, town: {} } });
    expect(idx.nameOf('key')).toBe('Mutalist Alad V Assassinate (Key)');
    expect(idx.nameOf('town')).toBe('HöLlvanian Old Town');
  });

  // The native sorts reproduce this order (math::collate); this pins it to the engine.
  it('the collation fixture is the order localeCompare gives', () => {
    expect([...collation].reverse().sort((a, b) => a.localeCompare(b))).toEqual(collation);
  });
});

describe('buildBrowseIndex', () => {
  it('reverses the catalog to slug→name and skips un-priceable entries', () => {
    const idx = buildBrowseIndex(fixture());
    expect(idx.nameOf('mag_prime_set')).toBe('Mag Prime Set');
    // quest_key is in catalog but absent from items → excluded from search list.
    expect(idx.names.some((n) => n.slug === 'quest_key')).toBe(false);
    expect(idx.names.some((n) => n.slug === 'mag_prime_set')).toBe(true);
  });

  it('falls back to a slug-derived name for unknown slugs', () => {
    const idx = buildBrowseIndex(fixture());
    expect(idx.nameOf('some_unknown_slug')).toBe('Some Unknown Slug');
  });

  it('is empty for a null/empty snapshot', () => {
    expect(buildBrowseIndex(null).names).toEqual([]);
    expect(buildBrowseIndex({}).names).toEqual([]);
  });
});

describe('row delta', () => {
  const delta = (entry) => {
    const m = { catalog: { 'x item': 'x' }, items: { x: { vol: 1, ...entry } } };
    return searchItems(m, buildBrowseIndex(m), 'x')[0].deltaPct;
  };

  it('computes (now - base) / base * 100', () => {
    expect(delta({ median_now: 60, median_90d: 50 })).toBeCloseTo(20);
    expect(delta({ median_now: 30, median_90d: 40 })).toBeCloseTo(-25);
  });

  it('is null when median_90d is missing or zero (no divide-by-junk)', () => {
    expect(delta({ median_now: 25 })).toBeNull();
    expect(delta({ median_now: 25, median_90d: 0 })).toBeNull();
    expect(delta({ median_now: 25, median_90d: -5 })).toBeNull();
    expect(delta({ median_now: 25, median_90d: '40' })).toBeNull();
  });

  it('is null when median_now is missing', () => {
    expect(delta({ median_90d: 40 })).toBeNull();
  });
});

describe('searchItems', () => {
  const m = fixture();
  const idx = buildBrowseIndex(m);

  it('returns [] for empty/whitespace query', () => {
    expect(searchItems(m, idx, '')).toEqual([]);
    expect(searchItems(m, idx, '   ')).toEqual([]);
  });

  it('matches by display name substring, case-insensitively', () => {
    const rows = searchItems(m, idx, 'PRIMED');
    const slugs = rows.map((r) => r.slug);
    expect(slugs).toContain('primed_continuity');
    expect(slugs).toContain('primed_flow');
    expect(slugs).not.toContain('mag_prime_set');
  });

  it('ranks prefix matches first, then by volume', () => {
    // Both start with "primed"; primed_flow (vol 300) outranks primed_continuity (200).
    const rows = searchItems(m, idx, 'primed');
    expect(rows[0].slug).toBe('primed_flow');
    expect(rows[1].slug).toBe('primed_continuity');
  });

  it('puts a prefix match above a busier mid-word match', () => {
    const market = { catalog: { 'mag set': 'busy', 'set mag': 'quiet' }, items: { busy: { vol: 100 }, quiet: { vol: 1 } } };
    expect(searchItems(market, buildBrowseIndex(market), 'set').map((r) => r.slug)).toEqual(['quiet', 'busy']);
  });

  it('carries vault status and delta onto rows', () => {
    const rows = searchItems(m, idx, 'mag');
    expect(rows[0].vault).toBe('vaulted');
    expect(rows[0].deltaPct).toBeCloseTo(20);
  });

  it('respects the limit', () => {
    expect(searchItems(m, idx, 'item', 1).length).toBe(1);
  });
});

describe('topMovers', () => {
  const m = fixture();
  const idx = buildBrowseIndex(m);

  it('splits risers (desc) and fallers (most-negative first) by Δ%', () => {
    const { risers, fallers } = topMovers(m, idx, { minVol: 20, limit: 8 });
    expect(risers[0].slug).toBe('mag_prime_set'); // +20 beats +10
    expect(risers[1].slug).toBe('primed_flow');
    expect(fallers[0].slug).toBe('primed_continuity'); // -25 beats -10
    expect(fallers[1].slug).toBe('rare_thing');
  });

  it('applies the volume floor - thin books are excluded', () => {
    const { risers } = topMovers(m, idx, { minVol: 20 });
    // thin_item has +200% but vol 3 < 20.
    expect(risers.some((r) => r.slug === 'thin_item')).toBe(false);
  });

  it('applies the price floor - avg 9.9 excluded, avg 10 included', () => {
    const { risers } = topMovers(m, idx, { minVol: 1, minPrice: 10 });
    const slugs = risers.map((r) => r.slug);
    expect(slugs).not.toContain('cheap_riser'); // avg 9.9 < 10, despite +100%
    expect(slugs).toContain('edge_riser'); // avg 10 == floor
  });

  it('defaults the price floor to 10 platinum', () => {
    const { risers } = topMovers(m, idx, { minVol: 1 });
    expect(risers.map((r) => r.slug)).not.toContain('cheap_riser');
  });

  it('excludes zero-move and unusable-baseline items', () => {
    const all = topMovers(m, idx, { minVol: 1 });
    const slugs = [...all.risers, ...all.fallers].map((r) => r.slug);
    expect(slugs).not.toContain('flat_item'); // exactly 0% move
    expect(slugs).not.toContain('no_baseline'); // median_90d = 0
  });

  it('honours the limit on each side', () => {
    const { risers } = topMovers(m, idx, { minVol: 1, limit: 1 });
    expect(risers.length).toBe(1);
  });

  it('returns empty lists for a null snapshot', () => {
    expect(topMovers(null, buildBrowseIndex(null))).toEqual({ risers: [], fallers: [] });
  });
});

describe('dispositionChanges', () => {
  it('returns the log newest first, capped, dropping malformed rows', () => {
    const market = { items: {}, rivens: { changes: [
      { slug: 'braton', name: 'Braton', from: 1.1, to: 1.15, seen_at: '2026-08-16T12:00:00Z' },
      { slug: 'lato', name: 'Lato', from: 1.35, to: 1.4, seen_at: '2026-07-01T00:00:00Z' },
      { slug: 'kulstar', name: 'Kulstar', from: 1.3, to: 1.35, seen_at: '2026-08-16T12:00:00Z' },
      { slug: 'bad', name: 'Bad', from: 'x', to: 1, seen_at: '2026-08-16T12:00:00Z' },
    ] } } as unknown as Market;
    expect(dispositionChanges(market).map((c) => c.slug)).toEqual(['braton', 'kulstar', 'lato']);
    expect(dispositionChanges(market, 1).map((c) => c.slug)).toEqual(['braton']);
  });
  it('breaks seen_at ties by slug in localeCompare order', () => {
    const changes = [...collation].reverse().map((slug) => ({ slug, name: slug, from: 1, to: 2, seen_at: '2026-08-16T12:00:00Z' }));
    const market = { items: {}, rivens: { changes } } as unknown as Market;
    expect(dispositionChanges(market, collation.length).map((c) => c.slug)).toEqual(collation);
  });
  it('is empty without the surface', () => {
    expect(dispositionChanges({ items: {} } as unknown as Market)).toEqual([]);
    expect(dispositionChanges(null)).toEqual([]);
  });
});

describe('vaultedTop', () => {
  const m = fixture();
  const idx = buildBrowseIndex(m);

  it('joins vault_status × items for only "vaulted", sorted by avg desc', () => {
    const rows = vaultedTop(m, idx);
    expect(rows.map((r) => r.slug)).toEqual(['rare_thing', 'mag_prime_set']); // 500 > 120
  });

  it('excludes vaulting-soon and available items', () => {
    const rows = vaultedTop(m, idx);
    const slugs = rows.map((r) => r.slug);
    expect(slugs).not.toContain('primed_flow'); // vaulting-soon
    expect(slugs).not.toContain('primed_continuity'); // available
  });

  it('respects the limit', () => {
    expect(vaultedTop(m, idx, 1).length).toBe(1);
  });

  it('returns [] when vault_status is absent', () => {
    expect(vaultedTop({ items: {} }, idx)).toEqual([]);
    expect(vaultedTop(null, idx)).toEqual([]);
  });
});

describe('handoffSample', () => {
  const m = fixture();
  const idx = buildBrowseIndex(m);
  const picks = [
    { slug: 'rare_thing', owned: 3, score: 75 },
    { slug: 'primed_continuity', owned: 1, score: 42 },
    { slug: 'mag_prime_set', owned: 2, score: 38 },
  ];

  it('renders the published picks in order with their native score and the stack value', () => {
    const rows = handoffSample({ ...m, handoff_sample: picks }, idx);
    expect(rows.map((r) => [r.slug, r.owned, r.score])).toEqual(picks.map((p) => [p.slug, p.owned, p.score]));
    for (const r of rows) expect(r.potential).toBe(Math.round(r.owned * m.items[r.slug].avg));
    expect(rows[0]).toMatchObject({ name: 'Rare Thing', vault: m.vault_status.rare_thing });
  });

  it('leaves out a pick it cannot back rather than inventing numbers', () => {
    const rows = handoffSample({ ...m, handoff_sample: [
      { slug: 'not_priced', owned: 1, score: 10 },
      { slug: 'rare_thing', owned: 0, score: 10 },
      { slug: 'rare_thing', owned: 1.5, score: 10 },
      { slug: 'primed_continuity', owned: 1, score: Number.NaN },
      { slug: 'primed_continuity', owned: 1, score: -1 },
      { slug: 'primed_continuity', score: 10 },
      { slug: 'primed_continuity', owned: 1 },
      null,
      { slug: 'mag_prime_set', owned: 2, score: 0 },
    ] }, idx);
    expect(rows.map((r) => r.slug)).toEqual(['mag_prime_set']);
  });

  it('rounds the stack value half up, as Math.round does', () => {
    const market = { ...m, items: { ...m.items, rare_thing: { ...m.items.rare_thing, avg: 2.25 } }, handoff_sample: [{ slug: 'rare_thing', owned: 2, score: 5 }] };
    expect(handoffSample(market, idx)[0].potential).toBe(5);
  });

  it('is empty on a snapshot without picks', () => {
    expect(handoffSample(m, idx)).toEqual([]);
    expect(handoffSample({ ...m, handoff_sample: null }, idx)).toEqual([]);
    expect(handoffSample(null, buildBrowseIndex(null))).toEqual([]);
  });
});
