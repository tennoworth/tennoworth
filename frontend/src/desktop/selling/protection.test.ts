import { expect, it, vi } from 'vitest';
import { ProtectionController, PROTECTION_BACKSTOP_MS, PROTECTION_REFRESH_EVENTS, type ProtectionSchedule } from './protection.svelte';
import type { OwnedRecord } from '../../contracts/data';
import type { ProtectionState } from '../../contracts/protection';

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
  const listeners = new Map<string, () => void>();
  const docListeners = new Map<string, () => void>();
  let tick: (() => void) | null = null;
  const doc = {
    hidden,
    addEventListener: vi.fn((type: string, cb: () => void) => { docListeners.set(type, cb); }),
    removeEventListener: vi.fn((type: string) => { docListeners.delete(type); }),
  };
  const env = {
    listen: vi.fn((event: string, cb: () => void) => { listeners.set(event, cb); return () => { listeners.delete(event); }; }),
    document: doc as unknown as ProtectionSchedule['document'],
    setInterval: vi.fn((fn: () => void, ms: number) => { expect(ms).toBe(PROTECTION_BACKSTOP_MS); tick = fn; return 7; }),
    clearInterval: vi.fn(() => { tick = null; }),
  };
  return { env, doc, listeners, docListeners, tick: () => tick?.() };
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
  for (const cb of s.listeners.values()) cb();
  await Promise.resolve();
  expect(port.desktopProtectionState).toHaveBeenCalledTimes(PROTECTION_REFRESH_EVENTS.length);
});

it('the backstop does not read orders while the window is hidden', async () => {
  const { port, s } = await started(true);
  s.tick();
  expect(port.desktopProtectionState).not.toHaveBeenCalled();
  s.doc.hidden = false;
  s.tick();
  expect(port.desktopProtectionState).toHaveBeenCalledTimes(1);
});

it('showing the window again refreshes once', async () => {
  const { port, s } = await started(true);
  s.docListeners.get('visibilitychange')?.();
  expect(port.desktopProtectionState).not.toHaveBeenCalled();
  s.doc.hidden = false;
  s.docListeners.get('visibilitychange')?.();
  expect(port.desktopProtectionState).toHaveBeenCalledTimes(1);
});

it('stopping removes every trigger', async () => {
  const { port, s, stop } = await started(false);
  stop();
  expect(s.listeners.size).toBe(0);
  expect(s.docListeners.size).toBe(0);
  s.tick();
  expect(port.desktopProtectionState).not.toHaveBeenCalled();
});
