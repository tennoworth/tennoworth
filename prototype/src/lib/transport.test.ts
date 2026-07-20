// @ts-nocheck — vitest fixtures; the transport's TS contract is exercised by tsc.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import {
  isDesktopRuntime,
  createTransport,
  HttpCompanionTransport,
  TauriTransport,
  NotImplementedError,
} from './transport.js';
import { CompanionUnreachableError } from './companion.js';

// The desktop sniff and TauriTransport read the Tauri globals; install/remove
// them per test so the two modes are isolated.
function installTauri(invoke) {
  globalThis.__TAURI_INTERNALS__ = { invoke };
  globalThis.__TAURI__ = { core: { invoke } };
}
function removeTauri() {
  delete globalThis.__TAURI_INTERNALS__;
  delete globalThis.__TAURI__;
}

afterEach(() => {
  removeTauri();
  vi.restoreAllMocks();
});

describe('isDesktopRuntime', () => {
  it('is false with no Tauri runtime injected', () => {
    removeTauri();
    expect(isDesktopRuntime()).toBe(false);
  });

  it('is true once __TAURI_INTERNALS__ is present', () => {
    installTauri(vi.fn());
    expect(isDesktopRuntime()).toBe(true);
  });
});

describe('createTransport selection', () => {
  it('returns the HTTP transport in a browser (no Tauri)', () => {
    removeTauri();
    const t = createTransport(() => ({ baseUrl: 'http://x', token: 't' }));
    expect(t).toBeInstanceOf(HttpCompanionTransport);
    expect(t.mode).toBe('http');
  });

  it('returns the Tauri transport inside the desktop webview', () => {
    installTauri(vi.fn());
    const t = createTransport(() => null);
    expect(t).toBeInstanceOf(TauriTransport);
    expect(t.mode).toBe('tauri');
  });
});

describe('TauriTransport op → invoke mapping', () => {
  it('health() invokes the `health` command and returns its payload', async () => {
    const invoke = vi.fn().mockResolvedValue({ ok: true, platform: 'linux' });
    installTauri(invoke);
    const t = new TauriTransport();
    await expect(t.health()).resolves.toEqual({ ok: true, platform: 'linux' });
    expect(invoke).toHaveBeenCalledWith('health');
  });

  it('fetchInventory() invokes `scan_inventory` and JSON-parses the returned string', async () => {
    const invoke = vi.fn().mockResolvedValue('{"Suits":[{"a":1}]}');
    installTauri(invoke);
    const t = new TauriTransport();
    await expect(t.fetchInventory()).resolves.toEqual({ Suits: [{ a: 1 }] });
    expect(invoke).toHaveBeenCalledWith('scan_inventory');
  });

  it('fetchInventory() surfaces the command rejection verbatim (graceful no-game text)', async () => {
    const invoke = vi
      .fn()
      .mockRejectedValue("Warframe doesn't appear to be running.\nStart the game, log past the title screen, then retry.");
    installTauri(invoke);
    const t = new TauriTransport();
    await expect(t.fetchInventory()).rejects.toThrow(/Warframe doesn't appear to be running/);
  });

  it('prefers window.__TAURI__.core.invoke over the internals shim', async () => {
    const publicInvoke = vi.fn().mockResolvedValue({ ok: true });
    const internalInvoke = vi.fn().mockResolvedValue({ ok: false });
    globalThis.__TAURI_INTERNALS__ = { invoke: internalInvoke };
    globalThis.__TAURI__ = { core: { invoke: publicInvoke } };
    const t = new TauriTransport();
    await t.health();
    expect(publicInvoke).toHaveBeenCalledWith('health');
    expect(internalInvoke).not.toHaveBeenCalled();
  });

  it('loadCachedMarket() invokes `cached_market` and JSON-parses the body', async () => {
    const invoke = vi.fn().mockResolvedValue('{"updated_at":"2026-07-20T10:00:00Z","items":{}}');
    installTauri(invoke);
    const t = new TauriTransport();
    await expect(t.loadCachedMarket()).resolves.toEqual({
      updated_at: '2026-07-20T10:00:00Z',
      items: {},
    });
    expect(invoke).toHaveBeenCalledWith('cached_market');
  });

  it('loadCachedMarket() returns null on a first run (command returns null)', async () => {
    const invoke = vi.fn().mockResolvedValue(null);
    installTauri(invoke);
    await expect(new TauriTransport().loadCachedMarket()).resolves.toBeNull();
  });

  it('loadCachedMarket() returns null on a corrupt cache rather than throwing', async () => {
    const invoke = vi.fn().mockResolvedValue('{not json');
    installTauri(invoke);
    await expect(new TauriTransport().loadCachedMarket()).resolves.toBeNull();
  });

  it('refreshMarket() on a 200 parses body into market and reports updated+etag', async () => {
    const invoke = vi.fn().mockResolvedValue({
      updated: true,
      updated_at: '2026-07-20T10:00:00Z',
      etag: '"e1"',
      body: '{"updated_at":"2026-07-20T10:00:00Z","items":{"x":{"avg":9}}}',
    });
    installTauri(invoke);
    const res = await new TauriTransport().refreshMarket();
    expect(invoke).toHaveBeenCalledWith('refresh_market');
    expect(res.updated).toBe(true);
    expect(res.updatedAt).toBe('2026-07-20T10:00:00Z');
    expect(res.etag).toBe('"e1"');
    expect(res.market).toEqual({ updated_at: '2026-07-20T10:00:00Z', items: { x: { avg: 9 } } });
  });

  it('refreshMarket() on a 304/offline no-op reports updated:false with no market', async () => {
    const invoke = vi.fn().mockResolvedValue({
      updated: false,
      updated_at: '2026-07-20T10:00:00Z',
      etag: '"e1"',
      body: null,
    });
    installTauri(invoke);
    const res = await new TauriTransport().refreshMarket();
    expect(res.updated).toBe(false);
    expect(res.market).toBeUndefined();
    expect(res.updatedAt).toBe('2026-07-20T10:00:00Z');
  });

  it.each([
    ['submitPlan', (t) => t.submitPlan([])],
    ['getPendingPlan', (t) => t.getPendingPlan()],
    ['resumePendingPlan', (t) => t.resumePendingPlan()],
    ['discardPendingPlan', (t) => t.discardPendingPlan()],
    ['fetchOrders', (t) => t.fetchOrders()],
    ['updateOrder', (t) => t.updateOrder('id', {})],
    ['deleteOrder', (t) => t.deleteOrder('id')],
    ['bulkVisibility', (t) => t.bulkVisibility([], true)],
    ['askAssistant', (t) => t.askAssistant('q', [], null)],
  ])('%s throws NotImplementedError without touching invoke', (op, call) => {
    const invoke = vi.fn();
    installTauri(invoke);
    const t = new TauriTransport();
    try {
      call(t);
      throw new Error('expected NotImplementedError');
    } catch (e) {
      expect(e).toBeInstanceOf(NotImplementedError);
      expect(e.op).toBe(op);
    }
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe('HttpCompanionTransport delegates to companion.ts with the current config', () => {
  beforeEach(() => {
    removeTauri();
    globalThis.fetch = vi.fn();
  });

  it('fetchInventory() GETs /inventory with the token from the getter', async () => {
    globalThis.fetch.mockResolvedValue({ ok: true, json: async () => ({ Suits: [] }) });
    const t = new HttpCompanionTransport(() => ({ baseUrl: 'http://x', token: 'tok' }));
    await t.fetchInventory();
    const [url, init] = globalThis.fetch.mock.calls[0];
    expect(url).toBe('http://x/inventory');
    expect(init.method).toBe('GET');
    expect(init.headers['X-Session-Token']).toBe('tok');
    expect(init.targetAddressSpace).toBe('loopback');
  });

  it('health() delegates to pingCompanion (loopback LNA option preserved)', async () => {
    globalThis.fetch.mockResolvedValue({ ok: true, json: async () => ({ ok: true, platform: 'pc' }) });
    const t = new HttpCompanionTransport(() => ({ baseUrl: 'http://x', token: 't' }));
    await expect(t.health()).resolves.toEqual({ ok: true, platform: 'pc' });
    const [url, init] = globalThis.fetch.mock.calls[0];
    expect(url).toBe('http://x/health');
    expect(init.targetAddressSpace).toBe('loopback');
  });

  it('health() still throws CompanionUnreachableError when the fetch rejects', async () => {
    globalThis.fetch.mockRejectedValue(new TypeError('Failed to fetch'));
    const t = new HttpCompanionTransport(() => ({ baseUrl: 'http://127.0.0.1:1', token: 't' }));
    await expect(t.health()).rejects.toBeInstanceOf(CompanionUnreachableError);
  });

  it('reads config lazily — a getter returning null rejects before any fetch', () => {
    const t = new HttpCompanionTransport(() => null);
    expect(() => t.fetchInventory()).toThrow(/Not connected/);
    expect(globalThis.fetch).not.toHaveBeenCalled();
  });

  it('loadCachedMarket() is a null no-op that makes NO fetch (hosted rule)', async () => {
    const t = new HttpCompanionTransport(() => null);
    await expect(t.loadCachedMarket()).resolves.toBeNull();
    expect(globalThis.fetch).not.toHaveBeenCalled();
  });

  it('refreshMarket() is an updated:false no-op that makes NO third-party fetch', async () => {
    const t = new HttpCompanionTransport(() => null);
    await expect(t.refreshMarket()).resolves.toEqual({
      updated: false,
      updatedAt: null,
      etag: null,
    });
    expect(globalThis.fetch).not.toHaveBeenCalled();
  });
});
