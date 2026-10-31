import { expect, it, vi } from 'vitest';
import defaults from '../../../../tests/fixtures/pacing.json';
import { type AccessStatus, WFM_ACCESS_EVENT } from '../../contracts/generated/desktop';

import { ProtectionController, PROTECTION_BACKSTOP_MS, PROTECTION_REVIEW_MS, type ProtectionSchedule } from './protection.svelte';
import type { OwnedRecord } from '../../contracts/data';
import type { ProtectionState } from '../../contracts/protection';

const accessStatus = (): AccessStatus => ({
  revision: 0, reason: '', cooldown_until_ms: 0, queue_count: 0, outstanding: 0,
  requests: 0, throttles: 0, cache_hits: 0, cache_misses: 0, queue_rejections: 0,
  restrictions: { ...defaults },
});

const settle = () => new Promise(resolve => setTimeout(resolve, 0));

const inventory = (count: number) => new Map<string, OwnedRecord>([['part', { slug: 'part', name: 'Part', count, leveled: 0, subtype: null, kept_lvl: null, type: 'MiscItems' }]]);
const allocation = (count: number): ProtectionState => ({ plan: { reserves: {}, goal: null }, snapshot_id: 1,
  items: { part: { owned: count, protected: 0, estimated: count, listed: 0, available: count } }, issues: [] });

it('a replacement inventory cannot reuse an outstanding allocation response', async () => {
  let finish!: (value: ProtectionState) => void;
  const port = { desktopProtectionState: vi.fn().mockImplementationOnce(() => new Promise<ProtectionState>(resolve => { finish = resolve; })).mockResolvedValue(allocation(1)), desktopSaveProtectionPlan: vi.fn() };
  const c = new ProtectionController(port);
  const old = inventory(6);
  const newer = inventory(1);
  const pending = c.setInventory(old, 1);
  await settle();
  expect(c.matchesInventory(newer, null)).toBe(false);
  await c.setInventory(newer, null);
  finish(allocation(6));
  await pending;
  expect(c.state?.items.part.estimated).toBe(1);
  expect(c.matchesInventory(newer, null)).toBe(true);
  expect(c.matchesInventory(old, 1)).toBe(false);
  expect(port.desktopProtectionState).toHaveBeenLastCalledWith({ snapshot_id: null, items: { part: { count: 1, leveled: 0 } } });
});

it('malformed quantities invalidate a previous valid allocation and retry restores it', async () => {
  const port = { desktopProtectionState: vi.fn().mockResolvedValue(allocation(2)), desktopSaveProtectionPlan: vi.fn() };
  const c = new ProtectionController(port);
  await c.setInventory(inventory(2), 1);
  const invalid = allocation(2);
  invalid.items.part.estimated = Number.NaN;
  port.desktopProtectionState.mockResolvedValue(invalid);
  await c.refresh();
  expect(c.state).toBeNull();
  expect(c.error).toContain('invalid');
  port.desktopProtectionState.mockResolvedValue(allocation(2));
  await c.refresh();
  expect(c.state?.items.part.estimated).toBe(2);
  expect(c.error).toBeNull();
});

it('keeps a native missing row absent for the shell to mark unavailable', async () => {
  const port = { desktopProtectionState: vi.fn().mockResolvedValue({ ...allocation(0), items: {} }), desktopSaveProtectionPlan: vi.fn() };
  const c = new ProtectionController(port);
  await c.setInventory(inventory(2), null);
  expect(c.state?.items.part).toBeUndefined();
});


it('a failed quantity refresh retains saved keep rules for editing without retaining sale permission', async () => {
  const state = allocation(5);
  state.plan = { reserves: { part: 2 }, goal: null };
  const port = { desktopProtectionState: vi.fn().mockResolvedValueOnce(state).mockRejectedValue(new Error('Inventory quantities are invalid')), desktopSaveProtectionPlan: vi.fn() };
  const c = new ProtectionController(port);
  await c.setInventory(inventory(5), 1);
  await c.refresh();
  expect(c.state).toBeNull();
  expect(c.savedPlan).toEqual({ reserves: { part: 2 }, goal: null });
  expect(c.error).toContain('invalid');
});

function schedule(hidden: boolean) {
  const listeners = new Map<string, (payload?: unknown) => void>();
  const docListeners = new Map<string, () => void>();
  let tick: (() => void) | null = null;
  const state = { now: 0, reviewing: false };
  const doc = {
    hidden,
    addEventListener: vi.fn((type: string, cb: () => void) => { docListeners.set(type, cb); }),
    removeEventListener: vi.fn((type: string) => { docListeners.delete(type); }),
  };
  const env: ProtectionSchedule = {
    listen: vi.fn(<T>(event: string, cb: (payload: T) => void) => { listeners.set(event, payload => cb(payload as T)); return () => { listeners.delete(event); }; }),
    document: doc as unknown as ProtectionSchedule['document'],
    setInterval: vi.fn((fn: () => void, ms: number) => { expect(ms).toBe(PROTECTION_REVIEW_MS); tick = fn; return 7; }),
    clearInterval: vi.fn(() => { tick = null; }),
    now: () => state.now,
    reviewing: () => state.reviewing,
  };
  // One interval tick, `ms` after the previous one.
  const advance = (ms: number) => { state.now += ms; tick?.(); };
  return { env, doc, state, listeners, docListeners, advance };
}

async function started(hidden: boolean) {
  const port = { desktopProtectionState: vi.fn().mockResolvedValue(allocation(1)), desktopSaveProtectionPlan: vi.fn() };
  const c = new ProtectionController(port);
  await c.setInventory(inventory(1), 1);
  port.desktopProtectionState.mockClear();
  const s = schedule(hidden);
  const stop = c.start(s.env);
  return { c, port, s, stop };
}

it('native changes refresh the quantities whether or not the window is shown', async () => {
  const { port, s } = await started(true);
  expect([...s.listeners.keys()].sort()).toEqual(['trade-allowance-changed', 'trade-detected', 'wfm-access-changed']);
  for (const [event, cb] of s.listeners) cb(event === WFM_ACCESS_EVENT ? accessStatus() : undefined);
  await settle();
  expect(port.desktopProtectionState).toHaveBeenCalledTimes(1);
});

it('outside a review, a shown window refreshes on the five-minute backstop only', async () => {
  const { port, s } = await started(false);
  s.advance(PROTECTION_REVIEW_MS);
  expect(port.desktopProtectionState).not.toHaveBeenCalled();
  s.advance(PROTECTION_BACKSTOP_MS - PROTECTION_REVIEW_MS);
  await settle();
  expect(port.desktopProtectionState).toHaveBeenCalledTimes(1);
  s.advance(PROTECTION_REVIEW_MS);
  expect(port.desktopProtectionState).toHaveBeenCalledTimes(1);
});

it('the backstop does not read orders while the window is hidden', async () => {
  const { port, s } = await started(true);
  s.advance(PROTECTION_BACKSTOP_MS);
  s.state.reviewing = true;
  s.advance(PROTECTION_REVIEW_MS);
  expect(port.desktopProtectionState).not.toHaveBeenCalled();
});

it('an open review refreshes every tick, so a changed snapshot blocks it', async () => {
  const { port, s } = await started(false);
  s.state.reviewing = true;
  s.advance(PROTECTION_REVIEW_MS);
  await new Promise(resolve => setTimeout(resolve, 0));
  s.advance(PROTECTION_REVIEW_MS);
  await settle();
  expect(port.desktopProtectionState).toHaveBeenCalledTimes(2);
});

it('showing the window again refreshes once', async () => {
  const { port, s } = await started(true);
  s.docListeners.get('visibilitychange')?.();
  expect(port.desktopProtectionState).not.toHaveBeenCalled();
  s.doc.hidden = false;
  s.docListeners.get('visibilitychange')?.();
  await settle();
  expect(port.desktopProtectionState).toHaveBeenCalledTimes(1);
});

it('stopping removes every trigger', async () => {
  const { port, s, stop } = await started(false);
  s.state.reviewing = true;
  stop();
  expect(s.listeners.size).toBe(0);
  expect(s.docListeners.size).toBe(0);
  s.advance(PROTECTION_BACKSTOP_MS);
  expect(port.desktopProtectionState).not.toHaveBeenCalled();
});

it('market traffic cannot recursively schedule quantity reads', async () => {
  const { port, s, c } = await started(false);
  const status = accessStatus();
  const emit = (payload: AccessStatus) => s.listeners.get(WFM_ACCESS_EVENT)?.(payload);
  emit(status);
  await settle();
  port.desktopProtectionState.mockClear();
  for (let i = 1; i <= 150; i++) {
    emit({ ...status, queue_count: Math.min(i, 128), outstanding: 2, requests: i, cache_misses: i, queue_rejections: Math.max(0, i - 128) });
    await settle();
  }
  expect(port.desktopProtectionState).toHaveBeenCalledTimes(0);
  expect(c.loading).toBe(false);
});

it('market policy and cooldown changes still refresh quantities', async () => {
  const { port, s } = await started(false);
  const status = accessStatus();
  const emit = (payload: AccessStatus) => s.listeners.get(WFM_ACCESS_EVENT)?.(payload);
  emit(status);
  await settle();
  port.desktopProtectionState.mockClear();
  emit({ ...status, revision: 1, restrictions: { ...status.restrictions, pause_all: true } });
  await settle();
  expect(port.desktopProtectionState).toHaveBeenCalledTimes(1);
  emit({ ...status, revision: 2 });
  await settle();
  expect(port.desktopProtectionState).toHaveBeenCalledTimes(2);
  emit({ ...status, revision: 2, cooldown_until_ms: 60000 });
  await settle();
  expect(port.desktopProtectionState).toHaveBeenCalledTimes(3);
  emit({ ...status, revision: 2 });
  await settle();
  expect(port.desktopProtectionState).toHaveBeenCalledTimes(4);
});

it('traffic after cooldown expiry refreshes once without feeding the counters back', async () => {
  const { port, s } = await started(false);
  const status = { ...accessStatus(), cooldown_until_ms: 60000 };
  const emit = (payload: AccessStatus) => s.listeners.get(WFM_ACCESS_EVENT)?.(payload);
  emit(status);
  await settle();
  port.desktopProtectionState.mockClear();
  s.state.now = 60000;
  emit({ ...status, requests: 1 });
  await settle();
  emit({ ...status, requests: 2 });
  await settle();
  expect(port.desktopProtectionState).toHaveBeenCalledTimes(1);
});

it('coalesces synchronous refresh events and bounds an in-flight burst to one follow-up', async () => {
  const { c, port, s } = await started(false);
  let finish!: (value: ProtectionState) => void;
  port.desktopProtectionState.mockImplementationOnce(() => new Promise<ProtectionState>(resolve => { finish = resolve; }));
  for (const cb of [...s.listeners].filter(([event]) => event !== WFM_ACCESS_EVENT).map(([, cb]) => cb)) cb();
  await settle();
  expect(port.desktopProtectionState).toHaveBeenCalledTimes(1);
  const waits = Array.from({ length: 50 }, () => c.refresh());
  expect(port.desktopProtectionState).toHaveBeenCalledTimes(1);
  finish(allocation(1));
  await Promise.all(waits);
  expect(port.desktopProtectionState).toHaveBeenCalledTimes(2);
  expect(c.loading).toBe(false);
});
