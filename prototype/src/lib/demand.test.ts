import { describe, expect, it } from 'vitest';

import {
  DEAD_SHARE,
  liquidityWeight,
  masteryBand,
  POPULAR_SHARE,
  readDemand,
  usageFor,
  type UsageEntry,
} from './demand';
import type { Market } from './types';

/** A usage curve peaking at `peak`, with a little mass either side. */
function curve(peak: number, height = 9): number[] {
  return Array.from({ length: 34 }, (_, mr) => {
    const d = Math.abs(mr - peak);
    return Math.max(0, height - d * 1.2);
  });
}

function usage(over: Partial<UsageEntry> = {}): UsageEntry {
  return {
    name: 'Braton Prime',
    category: 'Primary',
    year: 2025,
    share: 3.42,
    peak_mr: 10,
    by_mr: curve(10),
    ...over,
  };
}

const MARKET: Market = {
  usage: {
    braton_prime_set: usage(),
    torid: usage({ name: 'Torid', share: 9.62, peak_mr: 22, by_mr: curve(22) }),
    ancient_thing_set: usage({ name: 'Ancient Thing', share: 0.04, by_mr: curve(3) }),
  },
  set_to_parts: {
    braton_prime_set: {
      name: 'Braton Prime',
      parts: [
        { slug: 'braton_prime_receiver', component_name: 'Receiver' },
        { slug: 'braton_prime_stock', component_name: 'Stock' },
      ],
    },
    ancient_thing_set: {
      name: 'Ancient Thing',
      parts: [{ slug: 'ancient_thing_barrel', component_name: 'Barrel' }],
    },
  },
} as unknown as Market;

describe('usageFor', () => {
  it('reads a direct hit without claiming inheritance', () => {
    const hit = usageFor('torid', MARKET)!;
    expect(hit.entry.name).toBe('Torid');
    expect(hit.inherited).toBe(false);
  });

  it('walks a part up to its parent set and says that it did', () => {
    // A part showing its parent's number without saying so would imply
    // per-part telemetry that does not exist.
    const hit = usageFor('braton_prime_receiver', MARKET)!;
    expect(hit.entry.name).toBe('Braton Prime');
    expect(hit.inherited).toBe(true);
  });

  it('is null for something with no usage anywhere', () => {
    expect(usageFor('mystery_item', MARKET)).toBeNull();
  });

  it('is null on a snapshot with no usage surface at all', () => {
    expect(usageFor('torid', { items: {} } as unknown as Market)).toBeNull();
  });
});

describe('masteryBand', () => {
  it('brackets the peak rather than reporting a single spike', () => {
    const b = masteryBand(curve(10))!;
    expect(b.from).toBeLessThan(10);
    expect(b.to).toBeGreaterThan(10);
  });

  it('separates a low-MR weapon from a high-MR one', () => {
    const low = masteryBand(curve(5))!;
    const high = masteryBand(curve(28))!;
    expect(low.to).toBeLessThan(high.from);
  });

  it('clamps at the ends of the rank range', () => {
    const b = masteryBand(curve(0))!;
    expect(b.from).toBe(0);
    const top = masteryBand(curve(33))!;
    expect(top.to).toBe(33);
  });

  it('is null for an empty curve rather than dividing by zero', () => {
    expect(masteryBand(new Array(34).fill(0))).toBeNull();
    expect(masteryBand([])).toBeNull();
  });
});

describe('readDemand', () => {
  const traded = { vol: 40, price: 12, baseline: 12 };

  it('calls a played, trading part sells-today', () => {
    expect(readDemand('braton_prime_receiver', MARKET, traded).liquidity).toBe('sells-today');
  });

  it('flags a played part priced under its own baseline as underpriced', () => {
    const r = readDemand('braton_prime_receiver', MARKET, { vol: 40, price: 8, baseline: 12 });
    expect(r.liquidity).toBe('underpriced');
  });

  it('calls an unplayed part slow even when it is trading', () => {
    const r = readDemand('ancient_thing_barrel', MARKET, traded);
    expect(r.usage!.share).toBeLessThan(DEAD_SHARE);
    expect(r.liquidity).toBe('slow');
  });

  it('lets volume override popularity - a buyer tonight is not implied by fame', () => {
    const r = readDemand('braton_prime_receiver', MARKET, { vol: 1, price: 12, baseline: 12 });
    expect(r.liquidity).toBe('thin');
  });

  it('is unknown, not slow, when there is simply no usage data', () => {
    const r = readDemand('mystery_item', MARKET, traded);
    expect(r.liquidity).toBe('unknown');
    expect(r.usage).toBeNull();
  });

  it('carries the mastery band a seller can price against', () => {
    const r = readDemand('torid', MARKET, traded);
    expect(r.band!.from).toBeGreaterThan(14);
  });
});

describe('liquidityWeight', () => {
  it('is neutral for an item with no usage data', () => {
    // Absent data must not be punished - that would rank every unmatched item
    // below every matched one for a reason that has nothing to do with demand.
    expect(liquidityWeight(null)).toBe(1);
  });

  it('rewards a popular parent and penalises a dead one, gently', () => {
    expect(liquidityWeight(usage({ share: POPULAR_SHARE + 1 }))).toBe(1.25);
    expect(liquidityWeight(usage({ share: 0.01 }))).toBe(0.75);
  });

  it('interpolates between the thresholds', () => {
    const mid = liquidityWeight(usage({ share: (POPULAR_SHARE + DEAD_SHARE) / 2 }));
    expect(mid).toBeGreaterThan(0.75);
    expect(mid).toBeLessThan(1.25);
  });

  it('never swings hard enough to drown the live price signal', () => {
    for (const share of [0, 0.1, 0.5, 1, 5, 50]) {
      const w = liquidityWeight(usage({ share }));
      expect(w).toBeGreaterThanOrEqual(0.75);
      expect(w).toBeLessThanOrEqual(1.25);
    }
  });
});
