// @ts-nocheck — vitest fixtures; the transport's TS contract is exercised by tsc.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import {
  isDesktopRuntime,
  createTransport,
  HttpCompanionTransport,
  TauriTransport,
  DesktopCmdError,
  desktopWfmStatus,
  desktopWfmLogin,
  desktopWfmUnlock,
  desktopWfmLogout,
} from './transport.js';
import { CompanionUnreachableError } from './companion.js';
import { AssistantError } from './assistant.js';

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

  // ---- Listing / order ops: op → command mapping ------------------------

  it('submitPlan() invokes `submit_plan` with the items and returns the PlanResponse', async () => {
    const resp = { plan_id: 'p1', results: [{ slug: 'loki_prime_set', status: 'ok', message: null, order_id: 'o1' }] };
    const invoke = vi.fn().mockResolvedValue(resp);
    installTauri(invoke);
    const items = [{ slug: 'loki_prime_set', platinum: 120, quantity: 1, order_type: 'sell', visible: false }];
    await expect(new TauriTransport().submitPlan(items)).resolves.toEqual(resp);
    expect(invoke).toHaveBeenCalledWith('submit_plan', { items });
  });

  it('a CmdError rejection surfaces as DesktopCmdError with its code (needs_login)', async () => {
    const invoke = vi.fn().mockRejectedValue({ code: 'needs_login', message: 'Log in to warframe.market to create or edit listings.' });
    installTauri(invoke);
    const err = await new TauriTransport().submitPlan([]).catch((e) => e);
    expect(err).toBeInstanceOf(DesktopCmdError);
    expect(err.code).toBe('needs_login');
    expect(err.message).toMatch(/Log in to warframe.market/);
  });

  it('needs_unlock rejections keep their code too (locked session, login on disk)', async () => {
    const invoke = vi.fn().mockRejectedValue({ code: 'needs_unlock', message: 'Enter your passphrase.' });
    installTauri(invoke);
    const err = await new TauriTransport().resumePendingPlan().catch((e) => e);
    expect(err).toBeInstanceOf(DesktopCmdError);
    expect(err.code).toBe('needs_unlock');
  });

  it('getPendingPlan() maps the command null to null (no pending plan)', async () => {
    const invoke = vi.fn().mockResolvedValue(null);
    installTauri(invoke);
    await expect(new TauriTransport().getPendingPlan()).resolves.toBeNull();
    expect(invoke).toHaveBeenCalledWith('get_pending_plan');
  });

  it('updateOrder()/deleteOrder() pass orderId + patch through', async () => {
    const invoke = vi.fn().mockResolvedValue({ order_id: 'o9', status: 'ok', message: null });
    installTauri(invoke);
    const t = new TauriTransport();
    await t.updateOrder('o9', { platinum: 25 });
    expect(invoke).toHaveBeenCalledWith('update_order', { orderId: 'o9', patch: { platinum: 25 } });
    invoke.mockResolvedValue(null);
    await t.deleteOrder('o9');
    expect(invoke).toHaveBeenCalledWith('delete_order', { orderId: 'o9' });
  });

  it('bulkVisibility() wraps the results array to match the HTTP response shape', async () => {
    const invoke = vi.fn().mockResolvedValue([{ order_id: 'o1', status: 'ok', message: null }]);
    installTauri(invoke);
    const res = await new TauriTransport().bulkVisibility(['o1'], true);
    expect(invoke).toHaveBeenCalledWith('bulk_visibility', { orderIds: ['o1'], visible: true });
    expect(res).toEqual({ results: [{ order_id: 'o1', status: 'ok', message: null }] });
  });

  it('askAssistant() maps CmdError codes onto the AssistantError contract', async () => {
    const t = new TauriTransport();
    installTauri(vi.fn().mockRejectedValue({ code: 'no_api_key', message: 'no key' }));
    let err = await t.askAssistant('q', [], null).catch((e) => e);
    expect(err).toBeInstanceOf(AssistantError);
    expect(err.code).toBe('no_api_key');
    installTauri(vi.fn().mockRejectedValue({ code: 'upstream', message: 'HTTP 500' }));
    err = await t.askAssistant('q', [], null).catch((e) => e);
    expect(err.code).toBe('upstream');
    expect(err.detail).toBe('HTTP 500');
    installTauri(vi.fn().mockRejectedValue({ code: 'rate_limited', message: 'Too many advisor requests' }));
    err = await t.askAssistant('q', [], null).catch((e) => e);
    expect(err.code).toBe('unknown');
    expect(err.detail).toMatch(/Too many/);
  });

  it('askAssistant() resolves with the command answer on success', async () => {
    const invoke = vi.fn().mockResolvedValue({ answer: 'sell the set', usage: { prompt_tokens: 10, completion_tokens: 5 } });
    installTauri(invoke);
    await expect(new TauriTransport().askAssistant('q', [{ role: 'user', content: 'hi' }], 'ctx')).resolves.toEqual({
      answer: 'sell the set',
      usage: { prompt_tokens: 10, completion_tokens: 5 },
    });
    expect(invoke).toHaveBeenCalledWith('ask_assistant', { question: 'q', history: [{ role: 'user', content: 'hi' }], context: 'ctx' });
  });
});

describe('desktop WFM auth ops', () => {
  it('desktopWfmStatus() invokes wfm_auth_status', async () => {
    const invoke = vi.fn().mockResolvedValue({ logged_in: true, unlocked: false });
    installTauri(invoke);
    await expect(desktopWfmStatus()).resolves.toEqual({ logged_in: true, unlocked: false });
    expect(invoke).toHaveBeenCalledWith('wfm_auth_status');
  });

  it('desktopWfmLogin() passes credentials through and resolves void', async () => {
    const invoke = vi.fn().mockResolvedValue(null);
    installTauri(invoke);
    await desktopWfmLogin('me@example.com', 'pw', 'a-long-enough-passphrase', 'pc');
    expect(invoke).toHaveBeenCalledWith('wfm_login', {
      email: 'me@example.com',
      password: 'pw',
      passphrase: 'a-long-enough-passphrase',
      platform: 'pc',
    });
  });

  it('desktopWfmUnlock() surfaces bad_passphrase as a typed DesktopCmdError', async () => {
    const invoke = vi.fn().mockRejectedValue({ code: 'bad_passphrase', message: 'Wrong passphrase, or the login file was modified.' });
    installTauri(invoke);
    const err = await desktopWfmUnlock('nope').catch((e) => e);
    expect(err).toBeInstanceOf(DesktopCmdError);
    expect(err.code).toBe('bad_passphrase');
  });

  it('desktopWfmLogout() invokes wfm_logout', async () => {
    const invoke = vi.fn().mockResolvedValue(null);
    installTauri(invoke);
    await desktopWfmLogout();
    expect(invoke).toHaveBeenCalledWith('wfm_logout');
  });

  it('a non-CmdError rejection (plain string) still becomes an Error, not a swallow', async () => {
    const invoke = vi.fn().mockRejectedValue('boom');
    installTauri(invoke);
    const err = await desktopWfmStatus().catch((e) => e);
    expect(err).toBeInstanceOf(Error);
    expect(err).not.toBeInstanceOf(DesktopCmdError);
    expect(err.message).toBe('boom');
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
