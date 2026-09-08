import { afterEach, describe, expect, it, vi } from 'vitest';
import { callDomain, evaluateTradeSession, evaluateAdvisor } from './domain-commands';
import { DesktopCmdError } from '../contracts/errors';

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
