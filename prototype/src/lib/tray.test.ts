// @ts-nocheck — vitest fixtures; the module's TS contract is exercised by tsc.
import { describe, it, expect, afterEach, vi } from 'vitest';
import { installTauri, removeTauri } from './test-utils.js';
import { listenForTauriEvent, TRAY_HINT_EVENT } from './desktop-update.js';

afterEach(() => {
  removeTauri();
  vi.restoreAllMocks();
});

// Pin the literal both sides of the channel use — if App.svelte ever drifts to
// a different event name, the Rust emit is silently lost. (The Rust-side
// EVENT_TRAY_HINT const is not reachable from TS, so this pins the TS half.)
describe('TRAY_HINT_EVENT', () => {
  it('is the exact event name the Rust close-with-tray path emits', () => {
    expect(TRAY_HINT_EVENT).toBe('tray-hint');
  });
});

describe('listenForTauriEvent', () => {
  it('registers on the event and forwards the payload', async () => {
    const handlers = {};
    const listen = vi.fn((name, h) => {
      handlers[name] = h;
      return Promise.resolve(() => {});
    });
    installTauri(vi.fn(), listen);
    const seen = [];
    listenForTauriEvent(TRAY_HINT_EVENT, (p) => seen.push(p));
    expect(listen).toHaveBeenCalledTimes(1);
    expect(listen).toHaveBeenCalledWith(TRAY_HINT_EVENT, expect.any(Function));
    handlers[TRAY_HINT_EVENT]({ payload: { ran: true } });
    expect(seen).toEqual([{ ran: true }]);
  });

  it('is a no-op without the event API (never throws)', () => {
    installTauri(vi.fn()); // no event.listen
    expect(() => listenForTauriEvent(TRAY_HINT_EVENT, () => {})).not.toThrow();
  });

  it('swallows a rejected listen registration', async () => {
    const listen = vi.fn(() => Promise.reject(new Error('acl denied')));
    installTauri(vi.fn(), listen);
    expect(() => listenForTauriEvent(TRAY_HINT_EVENT, () => {})).not.toThrow();
    await Promise.resolve(); // let the rejection settle — must not surface
  });

  it('unregisters exactly once after registration completes', async () => {
    const unlisten = vi.fn();
    installTauri(vi.fn(), vi.fn(() => Promise.resolve(unlisten)));
    const stop = listenForTauriEvent(TRAY_HINT_EVENT, () => {});
    await Promise.resolve();
    stop();
    stop();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it('handles unmount before registration resolves and ignores late events', async () => {
    let resolveRegistration;
    let handler;
    const unlisten = vi.fn();
    const registration = new Promise((resolve) => { resolveRegistration = resolve; });
    installTauri(vi.fn(), vi.fn((_name, h) => {
      handler = h;
      return registration;
    }));
    const seen = [];
    const stop = listenForTauriEvent(TRAY_HINT_EVENT, (payload) => seen.push(payload));
    stop();
    handler({ payload: { late: true } });
    resolveRegistration(unlisten);
    await registration;
    await Promise.resolve();
    expect(seen).toEqual([]);
    expect(unlisten).toHaveBeenCalledTimes(1);
  });
});
