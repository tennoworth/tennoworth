import { describe, expect, it } from 'vitest';

import {
  baroPhase,
  byPlatPerDucat,
  currentPrice,
  ducatBasket,
  priceManifest,
  stockIsCurrent,
  type BaroRow,
} from './baro-board';
import type { Market, MarketItemEntry } from './types';

function entry(over: Partial<MarketItemEntry>): MarketItemEntry {
  return {
    avg: 0,
    low_sell: 0,
    top_buy: 0,
    vol: 40,
    ratio: 0,
    buys: 0,
    sells: 0,
    ...over,
  };
}

function market(items: Record<string, MarketItemEntry>): Market {
  return { items } as unknown as Market;
}

describe('currentPrice', () => {
  it('prefers the depth-aware price so one troll listing cannot set it', () => {
    expect(currentPrice(entry({ low_sell: 5, low5_avg: 42 }))).toBe(42);
  });

  it('falls back to the lowest ask when the snapshot predates depth', () => {
    expect(currentPrice(entry({ low_sell: 30 }))).toBe(30);
  });

  it('is null rather than zero when there is no price at all', () => {
    expect(currentPrice(entry({}))).toBeNull();
    expect(currentPrice(undefined)).toBeNull();
  });
});

describe('priceManifest', () => {
  const snapshot = market({
    primed_fury: entry({ low5_avg: 142, median_90d: 128, vol: 41 }),
    primed_reload: entry({ low5_avg: 96, median_90d: 104, vol: 22 }),
    prisma_skana: entry({ low5_avg: 72, median_90d: 75, vol: 3 }),
    prisma_angstrum: entry({ low5_avg: 44, median_90d: 47, vol: 30 }),
  });

  it('calls it a flip when the price sits above its own baseline', () => {
    const [row] = priceManifest(
      [{ item: 'Primed Fury', slug: 'primed_fury', ducats: 350, credits: 200000 }],
      snapshot,
    );
    expect(row.platPerDucat).toBeCloseTo(142 / 350, 5);
    expect(row.verdict).toBe('flip');
  });

  it('calls it a hold when his arrival has pushed it under baseline', () => {
    const [row] = priceManifest(
      [{ item: 'Primed Reload Speed', slug: 'primed_reload', ducats: 375 }],
      snapshot,
    );
    expect(row.verdict).toBe('hold');
  });

  it('reports thin volume instead of ranking on a meaningless price', () => {
    const [row] = priceManifest(
      [{ item: 'Prisma Skana', slug: 'prisma_skana', ducats: 400 }],
      snapshot,
    );
    expect(row.verdict).toBe('thin');
  });

  it('skips a line whose plat does not justify its ducats', () => {
    const [row] = priceManifest(
      [{ item: 'Prisma Angstrum', slug: 'prisma_angstrum', ducats: 400 }],
      snapshot,
    );
    // 44p / 400 ducats = 0.11 - under the floor.
    expect(row.verdict).toBe('skip');
  });

  it('keeps a cosmetic visible and unpriced rather than treating it as free plat', () => {
    const [row] = priceManifest([{ item: "Ki'Teer Sekhara", ducats: 200 }], snapshot);
    expect(row.verdict).toBe('unpriced');
    expect(row.price).toBeNull();
    expect(row.platPerDucat).toBeNull();
  });

  it('survives a snapshot that failed to load', () => {
    const rows = priceManifest([{ item: 'Primed Fury', slug: 'primed_fury', ducats: 350 }], null);
    expect(rows[0].verdict).toBe('unpriced');
  });
});

describe('byPlatPerDucat', () => {
  it('sinks unpriceable rows instead of sorting them as zero', () => {
    const rows = [
      { item: 'Cosmetic', platPerDucat: null, verdict: 'unpriced' },
      { item: 'Cheap', platPerDucat: 0.05, verdict: 'skip' },
      { item: 'Good', platPerDucat: 0.41, verdict: 'flip' },
    ] as BaroRow[];
    expect(byPlatPerDucat(rows).map((r) => r.item)).toEqual(['Good', 'Cheap', 'Cosmetic']);
  });
});

describe('ducatBasket', () => {
  const rows = [
    { item: 'A', ducats: 350, platPerDucat: 0.41, baseline: 128, verdict: 'flip' },
    { item: 'B', ducats: 500, platPerDucat: 0.34, baseline: 151, verdict: 'flip' },
    { item: 'C', ducats: 375, platPerDucat: 0.26, baseline: 104, verdict: 'hold' },
    { item: 'D', ducats: 400, platPerDucat: 0.18, baseline: 75, verdict: 'thin' },
  ] as BaroRow[];

  it('counts only what the board actually recommends buying', () => {
    // D is thin and must not inflate the basket.
    expect(ducatBasket(rows, 0).needed).toBe(1225);
    expect(ducatBasket(rows, 0).count).toBe(3);
  });

  it('reports coverage from scrapping, best value first', () => {
    const b = ducatBasket(rows, 900);
    expect(b.coveredByScrapping).toBe(2);
  });

  it('reports full coverage when the spares would pay for everything', () => {
    const b = ducatBasket(rows, 2000);
    expect(b.coveredByScrapping).toBe(3);
    expect(b.resale).toBe(383);
  });

  it('exposes no notion of a balance we cannot observe', () => {
    // Ducats are account state, never visible to an inventory scan. If an
    // "affordable" or "short" field reappears, something is pretending to
    // know a balance again - and the scrap planner will double-count.
    expect(Object.keys(ducatBasket(rows, 500)).sort()).toEqual([
      'count',
      'coveredByScrapping',
      'needed',
      'resale',
    ]);
  });
});

describe('baroPhase', () => {
  const start = Date.parse('2026-08-21T13:00:00Z');
  const end = Date.parse('2026-08-23T13:00:00Z');

  it('counts down to an announced arrival', () => {
    const p = baroPhase('2026-08-21T13:00:00Z', '2026-08-23T13:00:00Z', start - 3_600_000);
    expect(p.phase).toBe('incoming');
    expect(p.windowMs).toBe(3_600_000);
  });

  it('counts down to departure while he is here', () => {
    const p = baroPhase('2026-08-21T13:00:00Z', '2026-08-23T13:00:00Z', end - 7_200_000);
    expect(p.phase).toBe('here');
    expect(p.windowMs).toBe(7_200_000);
  });

  it('knows when the window has closed', () => {
    expect(baroPhase('2026-08-21T13:00:00Z', '2026-08-23T13:00:00Z', end + 1).phase).toBe('gone');
  });

  it('is unknown without a schedule', () => {
    expect(baroPhase(undefined, undefined, Date.now()).phase).toBe('unknown');
  });
});

describe('stockIsCurrent', () => {
  it('rejects stock carried over from a previous visit', () => {
    expect(stockIsCurrent('2026-08-07T13:00:00Z', '2026-08-21T13:00:00Z')).toBe(false);
  });

  it('accepts stock captured during the visit on screen', () => {
    expect(stockIsCurrent('2026-08-21T13:00:00Z', '2026-08-21T13:00:00Z')).toBe(true);
  });

  it('treats a missing stamp as not current', () => {
    expect(stockIsCurrent(undefined, '2026-08-21T13:00:00Z')).toBe(false);
  });
});
