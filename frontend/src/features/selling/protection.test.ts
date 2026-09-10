import { expect, it, vi } from 'vitest';
import { ProtectionController } from './protection.svelte';
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
