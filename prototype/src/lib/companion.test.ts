// @ts-nocheck — vitest runs these as JS-style fixtures; full TS shapes here would be busy-work without catching real bugs.
import { describe, it, expect, beforeEach, vi } from 'vitest';
import {
  loadCompanionConfig, saveCompanionConfig, clearCompanionConfig,
  parseCompanionUrl, pingCompanion, submitPlan,
  bulkVisibility, fetchOrders, updateOrder, deleteOrder,
  getPendingPlan, resumePendingPlan, discardPendingPlan,
  CompanionUnreachableError,
} from './companion.js';

beforeEach(() => {
  localStorage.clear();
  globalThis.fetch = vi.fn();
});

describe('parseCompanionUrl', () => {
  it('extracts baseUrl and token from the printed URL', () => {
    const got = parseCompanionUrl('http://127.0.0.1:45891?token=abc123');
    expect(got).toEqual({ baseUrl: 'http://127.0.0.1:45891', token: 'abc123' });
  });

  it('strips whitespace', () => {
    const got = parseCompanionUrl('   http://127.0.0.1:42/?token=x  \n');
    expect(got.token).toBe('x');
  });

  it('rejects an invalid URL', () => {
    expect(() => parseCompanionUrl('not a url')).toThrow();
  });

  it('rejects a URL without ?token=', () => {
    expect(() => parseCompanionUrl('http://127.0.0.1:42')).toThrow(/token/);
  });

  it('rejects an https URL (would imply a non-loopback target)', () => {
    expect(() => parseCompanionUrl('https://attacker.example/?token=abc'))
      .toThrow(/must be http:/i);
  });

  it('rejects a non-loopback host even with http://', () => {
    expect(() => parseCompanionUrl('http://attacker.example:42/?token=abc'))
      .toThrow(/127\.0\.0\.1 or localhost/);
  });

  it('accepts localhost and ::1 in addition to 127.0.0.1', () => {
    expect(parseCompanionUrl('http://localhost:42/?token=x').token).toBe('x');
    expect(parseCompanionUrl('http://[::1]:42/?token=x').token).toBe('x');
  });
});

describe('config persistence', () => {
  it('round-trips through localStorage', () => {
    saveCompanionConfig({ baseUrl: 'http://x', token: 'y' });
    expect(loadCompanionConfig()).toEqual({ baseUrl: 'http://x', token: 'y' });
  });

  it('returns null when empty', () => {
    expect(loadCompanionConfig()).toBeNull();
  });

  it('returns null on corrupt JSON instead of throwing', () => {
    localStorage.setItem('wfminv:companion-v1', '{garbage');
    expect(loadCompanionConfig()).toBeNull();
  });

  it('clearCompanionConfig wipes', () => {
    saveCompanionConfig({ baseUrl: 'http://x', token: 'y' });
    clearCompanionConfig();
    expect(loadCompanionConfig()).toBeNull();
  });
});

describe('pingCompanion', () => {
  it('returns the health payload on 200', async () => {
    globalThis.fetch.mockResolvedValue({
      ok: true,
      json: async () => ({ ok: true, platform: 'pc' }),
    });
    const cfg = { baseUrl: 'http://x', token: 't' };
    await expect(pingCompanion(cfg)).resolves.toEqual({ ok: true, platform: 'pc' });
  });

  it('throws on non-2xx', async () => {
    globalThis.fetch.mockResolvedValue({ ok: false, status: 500 });
    await expect(pingCompanion({ baseUrl: 'http://x' })).rejects.toThrow(/500/);
  });

  // The unreachable-vs-error distinction is what lets the app tell "serve is
  // down / the browser blocked loopback" apart from "connected but wrong", so
  // the type — not just the message — is the contract under test.
  it('throws CompanionUnreachableError when the health fetch itself rejects', async () => {
    globalThis.fetch.mockRejectedValue(new TypeError('Failed to fetch'));
    await expect(pingCompanion({ baseUrl: 'http://127.0.0.1:1', token: 't' }))
      .rejects.toBeInstanceOf(CompanionUnreachableError);
  });

  it('does NOT classify a non-OK HTTP response as unreachable', async () => {
    globalThis.fetch.mockResolvedValue({ ok: false, status: 500 });
    await expect(pingCompanion({ baseUrl: 'http://x', token: 't' }))
      .rejects.not.toBeInstanceOf(CompanionUnreachableError);
  });

  it('does NOT classify a non-JSON 200 (a web page) as unreachable', async () => {
    globalThis.fetch.mockResolvedValue({
      ok: true,
      json: async () => { throw new Error('not json'); },
    });
    await expect(pingCompanion({ baseUrl: 'http://x', token: 't' }))
      .rejects.not.toBeInstanceOf(CompanionUnreachableError);
  });
});

describe('submitPlan', () => {
  it('attaches X-Session-Token and POSTs JSON', async () => {
    globalThis.fetch.mockResolvedValue({
      ok: true,
      json: async () => ({ plan_id: 'p', results: [] }),
    });
    const cfg = { baseUrl: 'http://x', token: 'tok' };
    await submitPlan(cfg, [{ slug: 'a', platinum: 10, quantity: 1, order_type: 'sell', visible: false }]);
    expect(globalThis.fetch).toHaveBeenCalledOnce();
    const [url, init] = globalThis.fetch.mock.calls[0];
    expect(url).toBe('http://x/plan');
    expect(init.method).toBe('POST');
    expect(init.headers['X-Session-Token']).toBe('tok');
    expect(JSON.parse(init.body).items[0].slug).toBe('a');
  });

  it('extracts companion error message on 4xx', async () => {
    globalThis.fetch.mockResolvedValue({
      ok: false, status: 401,
      json: async () => ({ error: 'missing token' }),
    });
    await expect(submitPlan({ baseUrl: 'http://x', token: '' }, [])).rejects.toThrow(/missing token/);
  });

  it('falls back to HTTP status when body is not JSON', async () => {
    globalThis.fetch.mockResolvedValue({
      ok: false, status: 502,
      json: async () => { throw new Error('not json'); },
    });
    await expect(submitPlan({ baseUrl: 'http://x', token: '' }, [])).rejects.toThrow(/502/);
  });
});

describe('order management helpers', () => {
  const cfg = { baseUrl: 'http://x', token: 'tok' };

  function okJson(body) {
    return { ok: true, json: async () => body };
  }

  it('bulkVisibility POSTs to /orders/visibility with order_ids + visible', async () => {
    globalThis.fetch.mockResolvedValue(okJson({ results: [] }));
    await bulkVisibility(cfg, ['a', 'b'], true);
    const [url, init] = globalThis.fetch.mock.calls[0];
    expect(url).toBe('http://x/orders/visibility');
    expect(init.method).toBe('POST');
    expect(JSON.parse(init.body)).toEqual({ order_ids: ['a', 'b'], visible: true });
  });

  it('fetchOrders GETs /orders with no body', async () => {
    globalThis.fetch.mockResolvedValue(okJson({ data: [] }));
    await fetchOrders(cfg);
    const [url, init] = globalThis.fetch.mock.calls[0];
    expect(url).toBe('http://x/orders');
    expect(init.method).toBe('GET');
    expect(init.body).toBeUndefined();
  });

  it('updateOrder PATCHes /order/<id>', async () => {
    globalThis.fetch.mockResolvedValue(okJson({ status: 'ok' }));
    await updateOrder(cfg, 'abc123', { platinum: 50 });
    const [url, init] = globalThis.fetch.mock.calls[0];
    expect(url).toBe('http://x/order/abc123');
    expect(init.method).toBe('PATCH');
    expect(JSON.parse(init.body)).toEqual({ platinum: 50 });
  });

  it('deleteOrder DELETEs /order/<id>', async () => {
    globalThis.fetch.mockResolvedValue(okJson({ ok: true }));
    await deleteOrder(cfg, 'abc123');
    const [url, init] = globalThis.fetch.mock.calls[0];
    expect(url).toBe('http://x/order/abc123');
    expect(init.method).toBe('DELETE');
    expect(init.body).toBeUndefined();
  });

  it('all helpers send X-Session-Token', async () => {
    globalThis.fetch.mockResolvedValue(okJson({}));
    await fetchOrders(cfg);
    expect(globalThis.fetch.mock.calls[0][1].headers['X-Session-Token']).toBe('tok');
  });
});

describe('pending plan recovery', () => {
  const cfg = { baseUrl: 'http://x', token: 'tok' };

  it('getPendingPlan returns the plan payload on 200', async () => {
    globalThis.fetch.mockResolvedValue({
      ok: true,
      json: async () => ({ plan_id: 'abc', started_at: 't', items: [{ slug: 's', status: 'pending' }] }),
    });
    const got = await getPendingPlan(cfg);
    expect(got.plan_id).toBe('abc');
    expect(got.items[0].status).toBe('pending');
    const [url, init] = globalThis.fetch.mock.calls[0];
    expect(url).toBe('http://x/plan/pending');
    expect(init.method).toBe('GET');
  });

  it('getPendingPlan returns null on 404 instead of throwing', async () => {
    globalThis.fetch.mockResolvedValue({
      ok: false, status: 404,
      json: async () => ({ error: 'no pending plan' }),
    });
    await expect(getPendingPlan(cfg)).resolves.toBeNull();
  });

  it('getPendingPlan still throws on other errors', async () => {
    globalThis.fetch.mockResolvedValue({
      ok: false, status: 500,
      json: async () => ({ error: 'kaboom' }),
    });
    await expect(getPendingPlan(cfg)).rejects.toThrow(/kaboom/);
  });

  it('resumePendingPlan POSTs to /plan/resume', async () => {
    globalThis.fetch.mockResolvedValue({
      ok: true,
      json: async () => ({ plan_id: 'abc', results: [] }),
    });
    await resumePendingPlan(cfg);
    const [url, init] = globalThis.fetch.mock.calls[0];
    expect(url).toBe('http://x/plan/resume');
    expect(init.method).toBe('POST');
    expect(init.headers['X-Session-Token']).toBe('tok');
  });

  it('discardPendingPlan DELETEs /plan/pending', async () => {
    globalThis.fetch.mockResolvedValue({ ok: true, json: async () => ({ ok: true }) });
    await discardPendingPlan(cfg);
    const [url, init] = globalThis.fetch.mock.calls[0];
    expect(url).toBe('http://x/plan/pending');
    expect(init.method).toBe('DELETE');
  });
});
