import { afterEach, describe, expect, it, vi } from 'vitest';
import { baroValue, buildPlan, callDomain, ducatPlan, evaluateAdvisor, evaluateTradeSession, relicPlan, setRecos } from './domain';
import { DesktopCmdError } from '../contracts/errors';
import marketKeys from '../../../tests/fixtures/planners/market-keys.json';
import type { Market } from '../contracts/data';

afterEach(() => vi.unstubAllGlobals());
function invokeMock() {
  const invoke = vi.fn();
  vi.stubGlobal('__TAURI_INTERNALS__', { invoke });
  return invoke;
}
describe('native domain transport', () => {
  it('uses the generated operation envelope and validates the returned operation', async () => {
    const invoke = invokeMock();
    invoke.mockResolvedValue({ operation: 'trade_session', result: { rows: [] } });
    const input = { candidates: [], mode: 'fast' as const, budget: 0, target: null };
    expect(await callDomain('trade_session', input)).toEqual({ rows: [] });
    expect(invoke).toHaveBeenCalledWith('evaluate_domain', { request: { operation: 'trade_session', input } });
    invoke.mockResolvedValue({ operation: 'advisor', result: {} });
    await expect(callDomain('trade_session', input)).rejects.toThrow('unexpected response');
    invoke.mockResolvedValue(null);
    await expect(callDomain('trade_session', input)).rejects.toThrow('unexpected response');
  });
  it('preserves typed native rejections without running a fallback', async () => {
    const invoke = invokeMock();
    invoke.mockRejectedValue({ code: 'busy', message: 'Try again.' });
    await expect(callDomain('advisor', { slugs: [], market: {}, history: null, now_ms: 0 })).rejects.toBeInstanceOf(DesktopCmdError);
    expect(invoke).toHaveBeenCalledTimes(1);
  });
  it('rejects trade rows that do not belong to the requested candidates', async () => {
    const invoke = invokeMock();
    invoke.mockResolvedValue({ operation: 'trade_session', result: { rows: [{ key: 'unexpected' }] } });
    await expect(evaluateTradeSession({ candidates: [], mode: 'fast', budget: 1, target: null })).rejects.toThrow('unknown item');
  });
  it('sends only requested history medians from a public-sized archive', async () => {
    const invoke = invokeMock();
    invoke.mockResolvedValue({ operation: 'advisor', result: {} });
    const median = Array.from({ length: 365 }, () => 20);
    const items = Object.fromEntries(Array.from({ length: 4_000 }, (_, i) => [`item_${i}`, { median, volume: median, subtype: null }]));
    await evaluateAdvisor({ slugs: ['item_1', 'item_1'], now_ms: 0,
      market: { items: { item_1: { median_now: 20 }, unrelated: { median_now: 50 } }, usage: { unrelated: 'large' }, calendar: {}, set_to_parts: {} },
      history: { start: '2025-01-01', items } });
    const input = invoke.mock.calls[0][1].request.input;
    expect(input.slugs).toEqual(['item_1']);
    expect(input.history).toEqual({ start: '2025-01-01', items: { item_1: { median } } });
    expect(input.market).toEqual({ items: { item_1: { median_now: 20 } }, calendar: {}, set_to_parts: {} });
    expect(JSON.stringify(input).length).toBeLessThan(2_000);
  });

});

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
