import { describe, expect, it } from 'vitest';

import {
  chanceAt,
  decideRelic,
  evAt,
  evLadder,
  TRACE_COST,
  type RelicRewardRow,
} from './relic-ev';
import type { Market, MarketItemEntry } from './types';

function entry(over: Partial<MarketItemEntry>): MarketItemEntry {
  return { avg: 0, low_sell: 0, top_buy: 0, vol: 40, ratio: 0, buys: 0, sells: 0, ...over };
}

/** A relic's six slots: 3 common, 2 uncommon, 1 rare. */
function rewards(): RelicRewardRow[] {
  const mk = (
    slug: string,
    rarity: string,
    chances: RelicRewardRow['chances'],
  ): RelicRewardRow => ({
    reward_slug: slug,
    reward_name: slug,
    rarity,
    chance: chances!.intact!,
    chances,
    item_count: 1,
  });
  const common = { intact: 25.33, exceptional: 23.33, flawless: 20, radiant: 16.67 };
  const uncommon = { intact: 11, exceptional: 13, flawless: 17, radiant: 20 };
  const rare = { intact: 2, exceptional: 4, flawless: 6, radiant: 10 };
  return [
    mk('c1', 'Common', common),
    mk('c2', 'Common', common),
    mk('c3', 'Common', common),
    mk('u1', 'Uncommon', uncommon),
    mk('u2', 'Uncommon', uncommon),
    mk('rare1', 'Rare', rare),
  ];
}

/** Junk commons, one valuable rare - the shape that makes radiant worth it. */
const goldRare: Market = {
  items: {
    c1: entry({ low_sell: 2, low5_avg: 2, median_90d: 2 }),
    c2: entry({ low_sell: 2, low5_avg: 2, median_90d: 2 }),
    c3: entry({ low_sell: 2, low5_avg: 2, median_90d: 2 }),
    u1: entry({ low_sell: 5, low5_avg: 5, median_90d: 5 }),
    u2: entry({ low_sell: 5, low5_avg: 5, median_90d: 5 }),
    rare1: entry({ low_sell: 250, low5_avg: 250, median_90d: 250 }),
  },
} as unknown as Market;

/** Everything cheap - refining just burns traces. */
const allJunk: Market = {
  items: {
    c1: entry({ low_sell: 2, low5_avg: 2, median_90d: 2 }),
    c2: entry({ low_sell: 2, low5_avg: 2, median_90d: 2 }),
    c3: entry({ low_sell: 2, low5_avg: 2, median_90d: 2 }),
    u1: entry({ low_sell: 3, low5_avg: 3, median_90d: 3 }),
    u2: entry({ low_sell: 3, low5_avg: 3, median_90d: 3 }),
    rare1: entry({ low_sell: 6, low5_avg: 6, median_90d: 6 }),
  },
} as unknown as Market;

describe('chanceAt', () => {
  it('reads the per-refinement chance when the snapshot has one', () => {
    expect(chanceAt(rewards()[0], 'radiant')).toBe(16.67);
  });

  it('answers intact from the bare chance on an older snapshot', () => {
    const legacy: RelicRewardRow = {
      reward_slug: 'x',
      reward_name: 'X',
      rarity: 'Common',
      chance: 25.33,
    };
    expect(chanceAt(legacy, 'intact')).toBe(25.33);
  });

  it('refuses to reuse the intact chance for other refinements', () => {
    // Four identical columns presented as real would be worse than none.
    const legacy: RelicRewardRow = {
      reward_slug: 'x',
      reward_name: 'X',
      rarity: 'Common',
      chance: 25.33,
    };
    expect(chanceAt(legacy, 'radiant')).toBeNull();
  });
});

describe('evAt', () => {
  it('weights each reward by its chance at that refinement', () => {
    // 3×25.33%×2p + 2×11%×5p + 2%×250p = 1.52 + 1.10 + 5.00
    expect(evAt(rewards(), goldRare, 'intact')).toBeCloseTo(7.62, 2);
  });

  it('rises with refinement when the rare carries the table', () => {
    const intact = evAt(rewards(), goldRare, 'intact')!;
    const radiant = evAt(rewards(), goldRare, 'radiant')!;
    expect(radiant).toBeGreaterThan(intact);
  });

  it('is null for a refinement the snapshot cannot answer', () => {
    const legacy: RelicRewardRow[] = [
      { reward_slug: 'c1', reward_name: 'c1', rarity: 'Common', chance: 25.33 },
    ];
    expect(evAt(legacy, goldRare, 'radiant')).toBeNull();
    expect(evAt(legacy, goldRare, 'intact')).not.toBeNull();
  });

  it('prices a missing reward at zero rather than producing NaN', () => {
    const orphan: RelicRewardRow[] = [
      {
        reward_slug: 'not_in_snapshot',
        reward_name: 'Orphan',
        rarity: 'Common',
        chance: 25.33,
        chances: { intact: 25.33 },
      },
    ];
    expect(evAt(orphan, goldRare, 'intact')).toBe(0);
  });
});

describe('evLadder', () => {
  it('reports the trace cost of each rung', () => {
    const ladder = evLadder(rewards(), goldRare);
    expect(ladder.map((l) => l.refinement)).toEqual([
      'intact',
      'exceptional',
      'flawless',
      'radiant',
    ]);
    expect(ladder[3].traces).toBe(TRACE_COST.radiant);
    expect(ladder[0].platPerTrace).toBeNull();
  });

  it('measures gain against intact, not against the previous rung', () => {
    const ladder = evLadder(rewards(), goldRare);
    const intact = ladder[0].ev;
    expect(ladder[3].gainOverIntact).toBeCloseTo(ladder[3].ev - intact, 6);
  });
});

describe('decideRelic', () => {
  it('recommends refining when the extra plat pays for the traces', () => {
    const d = decideRelic(rewards(), undefined, goldRare);
    expect(d.verdict).toBe('refine');
    expect(d.best?.refinement).toBe('radiant');
    expect(d.best!.platPerTrace!).toBeGreaterThan(0.15);
  });

  it('breaks the near-tie on EV, because per trace the rungs are equivalent', () => {
    // The rare slot buys 0.08% per trace at EVERY refinement (2→4→6→10% for
    // 25→50→100 traces), so plat-per-trace barely separates them and ranking
    // on it would flip the recommendation on rounding.
    const ladder = evLadder(rewards(), goldRare);
    const perTrace = ladder.filter((l) => l.platPerTrace != null).map((l) => l.platPerTrace!);
    const spread = Math.max(...perTrace) - Math.min(...perTrace);
    expect(spread).toBeLessThan(0.01);

    const d = decideRelic(rewards(), undefined, goldRare);
    expect(d.best!.ev).toBe(Math.max(...ladder.map((l) => l.ev)));
  });

  it('stays intact when refining would only burn traces', () => {
    const d = decideRelic(rewards(), undefined, allJunk);
    expect(d.best?.refinement).toBe('intact');
    expect(d.verdict).toBe('crack');
  });

  it('says sell the relic when it is worth more than its contents', () => {
    const d = decideRelic(rewards(), entry({ low_sell: 45, low5_avg: 45, median_90d: 45 }), allJunk);
    expect(d.verdict).toBe('sell-intact');
    expect(d.sellNow).toBeGreaterThan(d.best!.ev);
  });

  it('flags a thin table regardless of how good the EV looks', () => {
    // Same gold rare, but nothing has traded - the EV is an ask, not a market.
    const dead = {
      items: Object.fromEntries(
        Object.entries(goldRare.items!).map(([k, v]) => [k, { ...v, vol: 0 }]),
      ),
    } as unknown as Market;
    const d = decideRelic(rewards(), undefined, dead);
    expect(d.verdict).toBe('thin');
  });

  it('is unknown when the snapshot cannot price anything', () => {
    const d = decideRelic([], undefined, goldRare);
    expect(d.verdict).toBe('unknown');
    expect(d.best).toBeNull();
  });

  it('counts moving rewards for the trap where the chart is gold and nobody buys', () => {
    const half = {
      items: {
        ...goldRare.items,
        c1: entry({ low_sell: 2, low5_avg: 2, vol: 0 }),
        c2: entry({ low_sell: 2, low5_avg: 2, vol: 0 }),
      },
    } as unknown as Market;
    const d = decideRelic(rewards(), undefined, half);
    expect(d.movingCount).toBe(4);
    expect(d.totalRewards).toBe(6);
  });
});
