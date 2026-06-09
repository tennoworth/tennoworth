// @ts-nocheck — vitest runs these as JS-style fixtures; full TS shapes here would be busy-work without catching real bugs.
import { describe, it, expect } from 'vitest';
import { deriveRelicPlan } from './relic-planner.js';

function ownedRelics(...entries) {
  const m = new Map();
  for (const [slug, count] of entries) {
    m.set(`${slug}|intact`, { slug, subtype: 'intact', count, name: `${slug} (Intact)` });
  }
  return m;
}

function market({ items = {}, rewards = {} } = {}) {
  return { items, relic_rewards: rewards };
}

describe('deriveRelicPlan', () => {
  it('returns nothing when the user owns no relics', () => {
    expect(deriveRelicPlan(new Map(), market({ rewards: { neo_b2_relic: [] } }))).toEqual([]);
  });

  it('returns nothing when no drop tables are available', () => {
    expect(deriveRelicPlan(ownedRelics(['neo_b2_relic', 5]), market({ items: {} }))).toEqual([]);
  });

  it('computes EPP as a chance-weighted sum of low_sell', () => {
    const m = market({
      rewards: {
        neo_b2_relic: [
          { reward_slug: 'a_set', reward_name: 'A Set', rarity: 'Rare', chance: 2 },
          { reward_slug: 'b_bp',  reward_name: 'B BP',  rarity: 'Uncommon', chance: 11 },
          { reward_slug: 'c_bp',  reward_name: 'C BP',  rarity: 'Common', chance: 25 },
        ],
      },
      items: {
        a_set: { low_sell: 100, vol: 20 },
        b_bp:  { low_sell: 30, vol: 8 },
        c_bp:  { low_sell: 4, vol: 200 },
      },
    });
    const plan = deriveRelicPlan(ownedRelics(['neo_b2_relic', 3]), m);
    expect(plan.length).toBe(1);
    // EPP = 0.02 * 100 + 0.11 * 30 + 0.25 * 4 = 2 + 3.3 + 1 = 6.3
    expect(plan[0].epp).toBeCloseTo(6.3, 1);
    expect(plan[0].epp_owned).toBeCloseTo(6.3 * 3, 1);
    expect(plan[0].owned).toBe(3);
    // All three rewards have vol_48h ≥ 5
    expect(plan[0].moving_count).toBe(3);
    expect(plan[0].total_rewards).toBe(3);
    expect(plan[0].rewards[0].chance).toBe(25);  // sorted by chance desc
  });

  it('skips relics the user owns only at non-intact refinements', () => {
    const ownedMap = new Map();
    ownedMap.set('neo_b2_relic|radiant', { slug: 'neo_b2_relic', subtype: 'radiant', count: 4, name: 'Neo B2 Relic (Radiant)' });
    const m = market({
      rewards: { neo_b2_relic: [{ reward_slug: 'x', reward_name: 'X', rarity: 'Rare', chance: 5 }] },
      items: { x: { low_sell: 100 } },
    });
    expect(deriveRelicPlan(ownedMap, m)).toEqual([]);
  });

  it('flags rewards under the moving threshold', () => {
    const m = market({
      rewards: { r: [
        { reward_slug: 'fresh', reward_name: 'Fresh', rarity: 'Rare', chance: 10 },
        { reward_slug: 'dead',  reward_name: 'Dead',  rarity: 'Rare', chance: 90 },
      ] },
      items: {
        fresh: { low_sell: 50, vol: 50 },
        dead:  { low_sell: 100, vol: 1 },  // valuable on paper, never trades
      },
    });
    const plan = deriveRelicPlan(ownedRelics(['r', 1]), m);
    expect(plan[0].moving_count).toBe(1);   // only `fresh` is moving
    expect(plan[0].total_rewards).toBe(2);
    // A high-EPP relic with only 1 of 2 rewards moving is exactly the
    // trap the volume signal is supposed to surface.
  });

  it('sorts by EPP desc and caps to limit', () => {
    const m = market({
      rewards: {
        cheap_relic: [{ reward_slug: 'a', reward_name: 'A', rarity: 'Common', chance: 100 }],
        rich_relic:  [{ reward_slug: 'b', reward_name: 'B', rarity: 'Rare', chance: 100 }],
        mid_relic:   [{ reward_slug: 'c', reward_name: 'C', rarity: 'Uncommon', chance: 100 }],
      },
      items: {
        a: { low_sell: 5 },
        b: { low_sell: 80 },
        c: { low_sell: 25 },
      },
    });
    const plan = deriveRelicPlan(ownedRelics(
      ['cheap_relic', 5],
      ['rich_relic', 1],
      ['mid_relic', 3],
    ), m, 2);
    expect(plan.length).toBe(2);
    expect(plan[0].relic_slug).toBe('rich_relic');
    expect(plan[1].relic_slug).toBe('mid_relic');
  });

  it('drops relics whose rewards have no live prices', () => {
    const m = market({
      rewards: { dead_relic: [{ reward_slug: 'unknown', reward_name: 'Unknown', rarity: 'Common', chance: 100 }] },
      items: {},
    });
    expect(deriveRelicPlan(ownedRelics(['dead_relic', 5]), m)).toEqual([]);
  });
});
