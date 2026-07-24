// @ts-nocheck — vitest runs these as JS-style fixtures; full TS shapes here would be busy-work without catching real bugs.
import { describe, it, expect, vi, beforeEach } from 'vitest';

const { pingCompanion, getPendingPlan, CompanionUnreachableError } = vi.hoisted(() => {
  class CompanionUnreachableError extends Error {
    constructor(message) {
      super(message);
      this.name = 'CompanionUnreachableError';
    }
  }
  return { pingCompanion: vi.fn(), getPendingPlan: vi.fn(), CompanionUnreachableError };
});

vi.mock('./companion.js', () => ({ pingCompanion, getPendingPlan, CompanionUnreachableError }));

const { verifyCompanionConnection, probeLoopbackDenied } = await import('./companion-connect.js');

const cfg = { baseUrl: 'http://127.0.0.1:12345', token: 'tok' };

beforeEach(() => {
  pingCompanion.mockReset();
  getPendingPlan.mockReset();
});

describe('verifyCompanionConnection', () => {
  it('reports connected with platform + assistant + pendingPlan from a successful ping', async () => {
    pingCompanion.mockResolvedValue({ ok: true, platform: 'pc', assistant: true });
    getPendingPlan.mockResolvedValue({ plan_id: 'p1', items: [] });
    const result = await verifyCompanionConnection(cfg);
    expect(result).toEqual({
      status: 'connected', platform: 'pc', assistant: true, pendingPlan: { plan_id: 'p1', items: [] },
    });
  });

  it('treats a companion with the assistant marked unavailable as connected with assistant: false', async () => {
    pingCompanion.mockResolvedValue({ ok: true, platform: 'pc' }); // pre-v0.1.6, no `assistant` field
    getPendingPlan.mockResolvedValue(null);
    const result = await verifyCompanionConnection(cfg);
    expect(result).toMatchObject({ status: 'connected', assistant: false, pendingPlan: null });
  });

  it('classifies an HTTPS-origin unreachable ping as unreachable', async () => {
    const restore = stubProtocol('https:');
    try {
      pingCompanion.mockRejectedValue(new CompanionUnreachableError("Couldn't reach the companion"));
      const result = await verifyCompanionConnection(cfg);
      expect(result.status).toBe('unreachable');
    } finally {
      restore();
    }
  });

  it('classifies the SAME unreachable error as a plain error on http origin', async () => {
    const restore = stubProtocol('http:');
    try {
      pingCompanion.mockRejectedValue(new CompanionUnreachableError("Couldn't reach the companion"));
      const result = await verifyCompanionConnection(cfg);
      expect(result.status).toBe('error');
    } finally {
      restore();
    }
  });

  it('rewrites a 401/token error into an actionable re-copy-the-URL message', async () => {
    pingCompanion.mockRejectedValue(new Error('HTTP 401: bad token'));
    const result = await verifyCompanionConnection(cfg);
    expect(result).toEqual({ status: 'error', message: expect.stringMatching(/re-copy the full URL/) });
  });

  it('passes through a non-token error verbatim', async () => {
    pingCompanion.mockRejectedValue(new Error('Health check failed: HTTP 500'));
    const result = await verifyCompanionConnection(cfg);
    expect(result).toEqual({ status: 'error', message: 'Health check failed: HTTP 500' });
  });

  it('never reports unreachable from a getPendingPlan failure — only /health can', async () => {
    // A raw TypeError post-/health (not CompanionUnreachableError) must stay 'error'.
    pingCompanion.mockResolvedValue({ ok: true, platform: 'pc', assistant: true });
    getPendingPlan.mockRejectedValue(new Error("didn't answer /plan/pending in time"));
    const result = await verifyCompanionConnection(cfg);
    expect(result.status).toBe('error');
  });
});

describe('probeLoopbackDenied', () => {
  it('resolves true when the Permissions API reports denied', async () => {
    const restore = stubPermissions({ state: 'denied' });
    try {
      expect(await probeLoopbackDenied()).toBe(true);
    } finally {
      restore();
    }
  });

  it('resolves false when granted', async () => {
    const restore = stubPermissions({ state: 'granted' });
    try {
      expect(await probeLoopbackDenied()).toBe(false);
    } finally {
      restore();
    }
  });

  it('resolves false (not throws) when the Permissions API is unsupported', async () => {
    const restore = stubPermissions(null);
    try {
      expect(await probeLoopbackDenied()).toBe(false);
    } finally {
      restore();
    }
  });
});

function stubProtocol(protocol) {
  const original = globalThis.location;
  Object.defineProperty(globalThis, 'location', { value: { protocol }, writable: true, configurable: true });
  return () => Object.defineProperty(globalThis, 'location', { value: original, writable: true, configurable: true });
}

function stubPermissions(status) {
  const original = globalThis.navigator?.permissions;
  Object.defineProperty(globalThis.navigator, 'permissions', {
    value: { query: () => (status ? Promise.resolve(status) : Promise.reject(new Error('unsupported'))) },
    writable: true, configurable: true,
  });
  return () => Object.defineProperty(globalThis.navigator, 'permissions', {
    value: original, writable: true, configurable: true,
  });
}
