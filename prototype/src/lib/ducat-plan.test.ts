import { describe, expect, it } from 'vitest';

import { KEEP_ABOVE_PLAT, planDucats, scrapCandidates, spareCopies } from './ducat-plan';
import type { Market, MarketItemEntry, OwnedRecord } from './types';

function entry(over: Partial<MarketItemEntry>): MarketItemEntry {
  return { avg: 0, low_sell: 0, top_buy: 0, vol: 20, ratio: 0, buys: 0, sells: 0, ...over };
}

/** Keyed exactly as the app keys it. */
function owned(rows: Array<[string, string, number]>): Map<string, OwnedRecord> {
  return new Map(
    rows.map(([slug, name, count]) => [
      `${slug}|`,
      { slug, name, count, subtype: null } as unknown as OwnedRecord,
    ]),
  );
}

const MARKET: Market = {
  items: {
    // 45 ducats for 3p — the ideal scrap.
    braton_prime_receiver: entry({ low_sell: 3, low5_avg: 3, median_90d: 3, ducats: 45 }),
    // 45 ducats for 1p.
    bo_prime_ornament: entry({ low_sell: 1, low5_avg: 1, median_90d: 1, ducats: 45 }),
    // 65 ducats but 18p — worth more sold.
    nova_prime_neuroptics: entry({ low_sell: 18, low5_avg: 18, median_90d: 18, ducats: 65 }),
    // Nobody lists it: pure ducat gain.
    forgotten_part: entry({ ducats: 25 }),
    // Tradeable but no ducat value at all.
    primed_continuity: entry({ low_sell: 40, low5_avg: 40, median_90d: 40, ducats: 0 }),
  },
} as unknown as Market;

describe('spareCopies', () => {
  it('counts everything past the copy you keep', () => {
    expect(spareCopies(owned([['x', 'X', 4]]), 'x')).toBe(3);
    expect(spareCopies(owned([['x', 'X', 1]]), 'x')).toBe(0);
  });

  it('sums across subtypes rather than missing on a bare-slug lookup', () => {
    const m = new Map([
      ['axi_a1_relic|intact', { slug: 'axi_a1_relic', name: 'r', count: 2, subtype: 'intact' }],
      ['axi_a1_relic|radiant', { slug: 'axi_a1_relic', name: 'r', count: 3, subtype: 'radiant' }],
    ] as unknown as Array<[string, OwnedRecord]>);
    expect(spareCopies(m, 'axi_a1_relic')).toBe(4);
  });
});

describe('scrapCandidates', () => {
  it('ranks by ducats given up per plat, not by ducats', () => {
    // Sorting by ducats alone would put the 65-ducat Neuroptics first, which
    // is exactly backwards — it is the one worth keeping.
    const c = scrapCandidates(
      owned([
        ['braton_prime_receiver', 'Braton Prime Receiver', 5],
        ['nova_prime_neuroptics', 'Nova Prime Neuroptics', 3],
        ['bo_prime_ornament', 'Bo Prime Ornament', 4],
      ]),
      MARKET,
    );
    expect(c.map((x) => x.slug)).toEqual([
      'bo_prime_ornament',
      'braton_prime_receiver',
      'nova_prime_neuroptics',
    ]);
  });

  it('puts an unsellable part first — scrapping it costs nothing', () => {
    const c = scrapCandidates(
      owned([
        ['forgotten_part', 'Forgotten Part', 3],
        ['braton_prime_receiver', 'Braton Prime Receiver', 3],
      ]),
      MARKET,
    );
    expect(c[0].slug).toBe('forgotten_part');
    expect(c[0].plat).toBe(0);
  });

  it('ignores items with no ducat value', () => {
    const c = scrapCandidates(owned([['primed_continuity', 'Primed Continuity', 4]]), MARKET);
    expect(c).toEqual([]);
  });

  it('ignores items you hold only one of', () => {
    const c = scrapCandidates(owned([['braton_prime_receiver', 'Braton Prime Receiver', 1]]), MARKET);
    expect(c).toEqual([]);
  });

  it('survives a missing inventory or snapshot', () => {
    expect(scrapCandidates(null, MARKET)).toEqual([]);
    expect(scrapCandidates(owned([['x', 'X', 3]]), null)).toEqual([]);
  });
});

describe('planDucats', () => {
  const candidates = () =>
    scrapCandidates(
      owned([
        ['braton_prime_receiver', 'Braton Prime Receiver', 6],
        ['bo_prime_ornament', 'Bo Prime Ornament', 5],
        ['nova_prime_neuroptics', 'Nova Prime Neuroptics', 3],
      ]),
      MARKET,
    );

  it('reaches the target with the cheapest parts first', () => {
    const plan = planDucats(candidates(), 200);
    expect(plan.ducats).toBeGreaterThanOrEqual(200);
    expect(plan.short).toBe(0);
    expect(plan.picks[0].slug).toBe('bo_prime_ornament');
  });

  it('takes only the copies the target needs, not the whole stack', () => {
    // 4 spare ornaments at 45 ducats covers 90 with two, not four.
    const plan = planDucats(candidates(), 90);
    expect(plan.picks[0].spare).toBe(2);
    expect(plan.ducats).toBe(90);
  });

  it('holds back a part worth more sold than scrapped, and says so', () => {
    const plan = planDucats(candidates(), 10_000);
    expect(plan.picks.every((p) => p.slug !== 'nova_prime_neuroptics')).toBe(true);
    expect(plan.heldBack.map((h) => h.slug)).toContain('nova_prime_neuroptics');
    expect(plan.short).toBeGreaterThan(0);
  });

  it('respects a caller who raises the keep threshold', () => {
    const plan = planDucats(candidates(), 10_000, KEEP_ABOVE_PLAT + 100);
    expect(plan.picks.some((p) => p.slug === 'nova_prime_neuroptics')).toBe(true);
    expect(plan.heldBack).toEqual([]);
  });

  it('reports the market value given up alongside the ducats gained', () => {
    const plan = planDucats(candidates(), 90);
    // Two ornaments at 1p each.
    expect(plan.platGivenUp).toBe(2);
  });

  it('reports the shortfall when nothing can close it', () => {
    const plan = planDucats([], 500);
    expect(plan.ducats).toBe(0);
    expect(plan.short).toBe(500);
    expect(plan.picks).toEqual([]);
  });

  it('does nothing when the target is already met', () => {
    const plan = planDucats(candidates(), 0);
    expect(plan.picks).toEqual([]);
    expect(plan.short).toBe(0);
  });
});
