const sources = { loadMarket: async () => { throw new Error('offline'); }, loadCatalogs: vi.fn() };
import { describe, expect, it, vi } from 'vitest';
import { InventoryController } from './controller.svelte';
import { ListingController } from '../selling/controller.svelte';
import { FilterController } from '../selling/filters.svelte';
import { DesktopCmdError } from '../../contracts/errors';
import type { StateStore } from '../../contracts/state-store';
import type { Snapshot } from '../../adapters/state-store';
import type { PendingPlan } from '../../contracts/data';


function store(snapshot: Snapshot | null = null, settings: Record<string, string> = {}): StateStore {
  return {
    mode: 'local', hydrate: async () => {}, getSetting: key => settings[key] ?? null,
    setSetting: async (key, value) => { settings[key] = value; },
    loadSnapshot: async () => snapshot, saveSnapshot: vi.fn(), clearSnapshot: vi.fn(),
  };
}
const snapshot: Snapshot = { ts: 100, invName: 'saved', rivens: [], owned: new Map([
  ['item', { slug: 'item', name: 'Item', type: 'Mods', count: 3, leveled: 1, kept_lvl: 5, subtype: null }],
]) };

describe('inventory lifecycle', () => {
  it('keeps restored quantities and ranked copies visible when prices cannot load', async () => {
    const log = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      const c = new InventoryController(store(snapshot), {
        loadCachedMarket: async () => null, refreshMarket: async () => ({ updated: false, updatedAt: null, etag: null }),
        fetchInventory: vi.fn(), reportScanIssue: vi.fn(),
      }, sources);
      await c.restore();
      expect(c.phase).toBe('done');
      expect(c.resolved.owned.get('item')).toEqual(snapshot.owned.get('item'));
      expect(c.marketLoadError).toContain('saved inventory');
    } finally { log.mockRestore(); }
  });

  it('rejects overlapping scans and keeps an actionable failure for retry', async () => {
    let fail!: (error: Error) => void;
    const fetchInventory = vi.fn(() => new Promise<unknown>((_, reject) => { fail = reject; }));
    const c = new InventoryController(store(), { fetchInventory, loadCachedMarket: vi.fn(), refreshMarket: vi.fn(), reportScanIssue: vi.fn() }, sources);
    const pending = c.pullInventory();
    await c.pullInventory();
    expect(fetchInventory).toHaveBeenCalledTimes(1);
    fail(new Error('Warframe is not running'));
    await pending;
    expect(c.pullingInventory).toBe(false);
    expect(c.pullError).toBe('Warframe is not running');
  });
});

describe('listing lifecycle', () => {
  it('preserves the pending plan when resuming requires authentication', async () => {
    const requestAuth = vi.fn();
    const c = new ListingController({ resumePendingPlan: async () => { throw new DesktopCmdError('needs_unlock', 'Unlock'); }, discardPendingPlan: vi.fn(), status: vi.fn(), logout: vi.fn() }, requestAuth);
    const plan = { plan_id: 'pending', started_at: '2026-09-08', items: [] } as PendingPlan;
    c.pendingPlan = plan;
    await c.doResume();
    expect(c.pendingPlan?.plan_id).toBe('pending');
    expect(c.resumePhase).toBe('idle');
    expect(requestAuth).toHaveBeenCalledWith('needs_unlock');
  });

  it('does not report a failed logout as success or discard the review', async () => {
    const c = new ListingController({ resumePendingPlan: vi.fn(), discardPendingPlan: vi.fn(), status: async () => { throw new Error('unavailable'); }, logout: async () => { throw new Error('failed'); } }, vi.fn());
    c.wfmStatus = { logged_in: true, unlocked: true };
    c.listingOpen = true;
    await expect(c.handleWfmLogout()).rejects.toThrow('failed');
    expect(c.wfmStatus?.unlocked).toBe(true);
    expect(c.listingOpen).toBe(true);
  });
});

it('restores filter preferences and preserves reserves when changing presets', async () => {
  const settings = { 'reserve-copies': '3', view: 'orders' };
  const c = new FilterController(store(null, settings));
  expect(c.reserveCopies).toBe(3);
  expect(c.view).toBe('orders');
  c.applyPreset('default');
  expect(c.reserveCopies).toBe(3);
  c.setReserveCopies(-1);
  expect(c.reserveCopies).toBe(0);
  expect(settings['reserve-copies']).toBe('0');
});
