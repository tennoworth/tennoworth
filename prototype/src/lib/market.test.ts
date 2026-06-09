// @ts-nocheck — vitest runs these as JS-style fixtures; full TS shapes here would be busy-work without catching real bugs.
import { describe, it, expect, beforeEach, vi } from 'vitest';

// We import inside each test so we can reset module state (cached promise).
async function freshMarket() {
  vi.resetModules();
  return await import('./market.js');
}

describe('loadMarket', () => {
  beforeEach(() => {
    globalThis.fetch = vi.fn();
  });

  it('parses the JSON snapshot and returns it', async () => {
    globalThis.fetch.mockResolvedValue({
      ok: true,
      json: async () => ({
        updated_at: '2026-05-26T18:39:22Z',
        items: { axi_k2_relic: { avg: 10 } },
        catalog: { 'axi k2 relic': 'axi_k2_relic' },
      }),
    });
    const { loadMarket } = await freshMarket();
    const m = await loadMarket();
    expect(m.items.axi_k2_relic.avg).toBe(10);
  });

  it('caches the result so a second call does not refetch', async () => {
    globalThis.fetch.mockResolvedValue({
      ok: true,
      json: async () => ({ items: {}, catalog: {} }),
    });
    const { loadMarket } = await freshMarket();
    await loadMarket();
    await loadMarket();
    expect(globalThis.fetch).toHaveBeenCalledOnce();
  });

  it('throws a helpful error on HTTP failure', async () => {
    globalThis.fetch.mockResolvedValue({ ok: false, status: 404 });
    const { loadMarket } = await freshMarket();
    await expect(loadMarket()).rejects.toThrow(/404/);
  });
});

describe('lookup', () => {
  it('returns the stats entry for a known slug', async () => {
    const { lookup } = await freshMarket();
    const market = { items: { axi_k2_relic: { avg: 10, vol: 72 } } };
    expect(lookup(market, 'axi_k2_relic')).toEqual({ avg: 10, vol: 72 });
  });

  it('returns null for an unknown slug', async () => {
    const { lookup } = await freshMarket();
    expect(lookup({ items: {} }, 'nope')).toBeNull();
  });
});
