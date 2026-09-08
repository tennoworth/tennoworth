import { describe, expect, it } from 'vitest';
import { evaluateDomainPreview } from './domain-preview';
import type { DomainRequest, OwnedItem } from '../contracts/generated/domain';
import numericBoundaries from '../../../tests/fixtures/advisor/numeric-boundaries.json';
import selectors from '../../../tests/fixtures/trade-session/selector.json';
import advisors from '../../../tests/fixtures/advisor/verdicts.json';
import histories from '../../../tests/fixtures/advisor/history.json';
import normalizations from '../../../tests/fixtures/inventory-normalization/cases.json';
import scores from '../../../tests/fixtures/inventory-scoring/cases.json';
import planners from '../../../tests/fixtures/planners/cases.json';

describe('development domain host', () => {
  // These fixtures record legacy output separately from deliberate native
  // rejection of malformed snapshots and overflowing arithmetic.
  for (const fixture of numericBoundaries) it(fixture.name, () => {
    expect(evaluateDomainPreview({ operation: fixture.operation, input: fixture.input } as unknown as DomainRequest)).toEqual(fixture.legacy);
  });
  for (const [operation, cases] of [
    ['trade_session', selectors], ['advisor', advisors], ['history', histories],
    ['normalize_inventory', normalizations.cases], ['score_inventory', scores.cases],
  ] as const) {
    for (const fixture of cases) it(`${operation}: ${fixture.name}`, () => {
      expect(evaluateDomainPreview({ operation, input: fixture.request } as unknown as DomainRequest))
        .toEqual({ operation, result: fixture.expected });
    });
  }
  const operations = { sets: 'set_recos', relic: 'relic_plan', ducat: 'ducat_plan', build: 'build_plan' } as const;
  for (const fixture of planners) it(`planner: ${fixture.name}`, () => {
    const operation = operations[fixture.operation as keyof typeof operations];
    expect(evaluateDomainPreview({ operation, input: fixture.input } as unknown as DomainRequest))
      .toEqual({ operation, result: fixture.expected });
  });
  it('omits null optional trade identity flags and private candidate fields', () => {
    const input = structuredClone(selectors[0].request);
    Object.assign(input.candidates[0], { subtype: null, supported: null, privateField: true });
    const response = evaluateDomainPreview({ operation: 'trade_session', input } as unknown as DomainRequest) as { result: { rows: unknown[] } };
    expect(response.result.rows[0]).not.toHaveProperty('subtype');
    expect(response.result.rows[0]).not.toHaveProperty('supported');
    expect(response.result.rows[0]).not.toHaveProperty('privateField');
  });
  it('rejects an unknown operation instead of returning an empty successful result', () => {
    expect(() => evaluateDomainPreview({ operation: 'not-supported', input: {} } as unknown as DomainRequest)).toThrow('Unknown domain preview operation');
  });
  it('serializes the unknown ducat ratio as null and preserves the default keep threshold', () => {
    const response = evaluateDomainPreview({ operation: 'ducat_plan', input: {
      owned: [{ slug: 'unpriced', name: 'Unpriced', count: 2, subtype: null }, { slug: 'valuable', name: 'Valuable', count: 2, subtype: null }],
      market: { items: { unpriced: { ducats: 45 }, valuable: { ducats: 100, low_sell: 20, vol: 10 } } },
      target: 200, keepAbove: null,
    } });
    expect(response).toMatchObject({ operation: 'ducat_plan', result: {
      candidates: [expect.objectContaining({ slug: 'unpriced', ducatsPerPlat: null }), expect.anything()],
      plan: { ducats: 45, heldBack: [expect.objectContaining({ slug: 'valuable' })] },
    } });
  });
  it('retains distinct owned rows and uses null planner limits as defaults', () => {
    const owned = Array.from({ length: 5 }, (_, i) => ({ slug: `relic_${i}`, name: `Relic ${i}`, count: 1, subtype: 'intact' }));
    const market = { items: { reward: { low_sell: 10, vol: 10 } }, relic_rewards: Object.fromEntries(owned.map(row => [row.slug,
      [{ reward_slug: 'reward', reward_name: 'Reward', rarity: 'Common', chance: 100 }]])) };
    const result = evaluateDomainPreview({ operation: 'relic_plan', input: { owned, market, limit: null } }) as { result: unknown[] };
    expect(result.result).toHaveLength(3);
    const unlimited = evaluateDomainPreview({ operation: 'relic_plan', input: { owned, market, limit: 5 } }) as { result: unknown[] };
    expect(unlimited.result).toHaveLength(5);
  });
  it('keeps a price-bearing entry when optional average and volume are absent', () => {
    const response = evaluateDomainPreview({ operation: 'score_inventory', input: {
      owned: [['item|', { slug: 'item', name: 'Item', count: 1, leveled: 0, kept_lvl: null, subtype: null, type: 'Mod' }]],
      market: { items: { item: { low_sell: 10 } }, usage: {}, set_to_parts: {} }, reserve_copies: 0, spares_only: false,
    } } as unknown as DomainRequest);
    expect(response).toMatchObject({ result: [{ key: 'item|', clearing_price: 10, potential_plat: 0, demand: { liquidity: 'thin' } }] });
  });
  it('returns zero-spare facts and keeps score output in request order', () => {
    const input = scores.cases[0].request;
    const original = input.owned[0][1] as unknown as OwnedItem;
    const owned = [['first', { ...original, count: 1, leveled: 0, kept_lvl: null }], ['second', { ...original, count: 10, leveled: 0, kept_lvl: null }]];
    const response = evaluateDomainPreview({ operation: 'score_inventory', input: { ...input, owned, spares_only: true } } as unknown as DomainRequest);
    expect(response).toMatchObject({ operation: 'score_inventory', result: [{ key: 'first', sellable: 0 }, { key: 'second', sellable: 9 }] });
  });
});
