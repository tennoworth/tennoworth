// @ts-nocheck — hand-built market fixtures; full TS shapes would be busy-work.
import { describe, it, expect } from 'vitest';
import {
  titleCase,
  buildBrowseIndex,
  itemDeltaPct,
  searchItems,
  topMovers,
  vaultedTop,
} from './market-browse.js';

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
    },
    vault_status: {
      mag_prime_set: 'vaulted',
      rare_thing: 'vaulted',
      primed_flow: 'vaulting-soon',
      primed_continuity: 'available',
    },
  };
}

describe('titleCase', () => {
  it('upper-cases each word, including inside parens', () => {
    expect(titleCase('mag prime set')).toBe('Mag Prime Set');
    expect(titleCase('mutalist alad v assassinate (key)')).toBe('Mutalist Alad V Assassinate (Key)');
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

describe('itemDeltaPct', () => {
  it('computes (now - base) / base * 100', () => {
    expect(itemDeltaPct({ median_now: 60, median_90d: 50 })).toBeCloseTo(20);
    expect(itemDeltaPct({ median_now: 30, median_90d: 40 })).toBeCloseTo(-25);
  });

  it('returns null when median_90d is missing or zero (no divide-by-junk)', () => {
    expect(itemDeltaPct({ median_now: 25 })).toBeNull();
    expect(itemDeltaPct({ median_now: 25, median_90d: 0 })).toBeNull();
    expect(itemDeltaPct({ median_now: 25, median_90d: -5 })).toBeNull();
  });

  it('returns null when median_now is missing, and on null input', () => {
    expect(itemDeltaPct({ median_90d: 40 })).toBeNull();
    expect(itemDeltaPct(null)).toBeNull();
    expect(itemDeltaPct(undefined)).toBeNull();
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

  it('applies the volume floor — thin books are excluded', () => {
    const { risers } = topMovers(m, idx, { minVol: 20 });
    // thin_item has +200% but vol 3 < 20.
    expect(risers.some((r) => r.slug === 'thin_item')).toBe(false);
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
