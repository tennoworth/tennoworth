// @ts-nocheck — vitest fixtures; the transport's TS contract is exercised by tsc.
import { describe, it, expect, afterEach, vi } from 'vitest';
import { installTauri, removeTauri } from './test-utils.js';
import {
  isDesktopRuntime,
  createTransport,
  HostedTransport,
  TauriTransport,
  DesktopCmdError,
  desktopWfmStatus,
  desktopWfmLogin,
  desktopWfmUnlock,
  desktopTrySilentUnlock,
} from './transport.js';

// The desktop sniff and TauriTransport read the Tauri globals; install/remove
// them per test so the two modes are isolated.

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
  it('returns HostedTransport on the informational site (no Tauri)', () => {
    removeTauri();
    expect(createTransport()).toBeInstanceOf(HostedTransport);
  });

  it('returns the Tauri transport inside the desktop webview', () => {
    installTauri(vi.fn());
    expect(createTransport()).toBeInstanceOf(TauriTransport);
  });
});

describe('HostedTransport — the informational site has no interactive ops', () => {
  it('loadCachedMarket() is a null no-op that makes NO fetch (hosted rule)', async () => {
    await expect(new HostedTransport().loadCachedMarket()).resolves.toBeNull();
  });

  it('refreshMarket() is an updated:false no-op that makes NO third-party fetch', async () => {
    await expect(new HostedTransport().refreshMarket()).resolves.toEqual({
      updated: false,
      updatedAt: null,
      etag: null,
    });
  });

  it('interactive ops throw — the site is informational only', async () => {
    const t = new HostedTransport();
    await expect(t.fetchInventory()).rejects.toThrow(/informational site/);
    await expect(t.submitPlan([])).rejects.toThrow(/informational site/);
    await expect(t.fetchOrders()).rejects.toThrow(/informational site/);
    await expect(t.updateOrder('o', { visible: false })).rejects.toThrow(/informational site/);
    await expect(t.deleteOrder('o')).rejects.toThrow(/informational site/);
    await expect(t.bulkVisibility([], true)).rejects.toThrow(/informational site/);
    await expect(t.resumePendingPlan()).rejects.toThrow(/informational site/);
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

  it('reportScanIssue() invokes `report_scan_issue` with the error and returns the report', async () => {
    const calls: Array<[string, unknown]> = [];
    installTauri((cmd: string, args: unknown) => {
      calls.push([cmd, args]);
      return Promise.resolve({ url: 'https://github.com/x/y/issues/new?title=z', opened: true });
    });
    const t = new TauriTransport();
    await expect(t.reportScanIssue('boom')).resolves.toEqual({
      url: 'https://github.com/x/y/issues/new?title=z',
      opened: true,
    });
    expect(calls[0][0]).toBe('report_scan_issue');
    expect(calls[0][1]).toEqual({ error: 'boom' });
    removeTauri();
  });

  it('reportScanIssue() reports a failed open as data, not a rejection', async () => {
    // The URL is still filable by hand, so this must not reject — the UI shows
    // it as a copyable link instead of a dead button.
    installTauri(() => Promise.resolve({ url: 'https://github.com/x', opened: false }));
    const t = new TauriTransport();
    const r = await t.reportScanIssue(null);
    expect(r.opened).toBe(false);
    expect(r.url).toBe('https://github.com/x');
    removeTauri();
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
    await desktopWfmLogin('me@example.com', 'pw', 'a-long-enough-passphrase', 'pc', true);
    expect(invoke).toHaveBeenCalledWith('wfm_login', {
      email: 'me@example.com',
      password: 'pw',
      passphrase: 'a-long-enough-passphrase',
      platform: 'pc',
      remember: true,
    });
  });

  it('desktopWfmUnlock() surfaces bad_passphrase as a typed DesktopCmdError', async () => {
    const invoke = vi.fn().mockRejectedValue({ code: 'bad_passphrase', message: 'Wrong passphrase, or the login file was modified.' });
    installTauri(invoke);
    const err = await desktopWfmUnlock('nope', false).catch((e) => e);
    expect(err).toBeInstanceOf(DesktopCmdError);
    expect(err.code).toBe('bad_passphrase');
  });

  it('desktopWfmUnlock() forwards the remember flag', async () => {
    const invoke = vi.fn().mockResolvedValue(null);
    installTauri(invoke);
    await desktopWfmUnlock('a-long-enough-passphrase', true);
    expect(invoke).toHaveBeenCalledWith('unlock_jwt', {
      passphrase: 'a-long-enough-passphrase',
      remember: true,
    });
  });

  it('desktopTrySilentUnlock() resolves the keyring verdict as a plain boolean', async () => {
    const invoke = vi.fn().mockResolvedValue(false);
    installTauri(invoke);
    await expect(desktopTrySilentUnlock()).resolves.toBe(false);
    expect(invoke).toHaveBeenCalledWith('try_silent_unlock');
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
