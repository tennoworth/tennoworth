import { afterEach, describe, expect, it, vi } from 'vitest';
import { baroValue, buildPlan, ducatPlan, relicPlan, setRecos } from './planner-domain';
import marketKeys from '../../../tests/fixtures/planners/market-keys.json';
import type { Market } from '../contracts/data';

afterEach(() => vi.unstubAllGlobals());

describe('planner requests', () => {
  it('send each planner only the snapshot keys the native tests check it reads', async () => {
    const invoke = vi.fn(async (_cmd: string, args: { request: { operation: string } }) => ({
      operation: args.request.operation,
      result: args.request.operation === 'ducat_plan' ? { candidates: [], plan: { picks: [], heldBack: [] } } : [],
    }));
    vi.stubGlobal('__TAURI_INTERNALS__', { invoke });
    const market = { updated_at: 'now', items: { a: {} }, set_to_parts: {}, relic_rewards: {}, catalog: {}, path_to_info: {}, recipes: {}, usage_history: {} } as unknown as Market;
    const owned = new Map([['a', { slug: 'a', name: 'A', count: 1, subtype: null, type: 'Misc', kept_lvl: null, leveled: 0 }]]);
    await relicPlan(owned, market);
    await setRecos(owned, market);
    await ducatPlan(owned, market, 0);
    await baroValue([], market, owned);
    await buildPlan('a_set', 'A', [], market, owned, null);
    const sent = Object.fromEntries(invoke.mock.calls.map(([, { request }]) => [request.operation, Object.keys((request as unknown as { input: { market: object } }).input.market).sort()]));
    expect(sent).toEqual(Object.fromEntries(Object.entries(marketKeys).map(([operation, keys]) => [operation, [...keys].sort()])));
  });

  it('keeps a missing snapshot missing', async () => {
    const invoke = vi.fn(async () => ({ operation: 'relic_plan', result: [] }));
    vi.stubGlobal('__TAURI_INTERNALS__', { invoke });
    await relicPlan(null, null);
    expect(invoke).toHaveBeenCalledWith('evaluate_domain', { request: { operation: 'relic_plan', input: { owned: [], market: null, limit: 3 } } });
  });
});
