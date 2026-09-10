const sources = { loadMarket: async () => { throw new Error('offline'); }, loadCatalogs: vi.fn(), normalizeInventory: vi.fn() };
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
    const c = new ListingController({ getPendingPlan: vi.fn().mockResolvedValue(null), resumePendingPlan: async () => { throw new DesktopCmdError('needs_unlock', 'Unlock'); }, discardPendingPlan: vi.fn(), status: vi.fn(), logout: vi.fn() }, requestAuth);
    const plan = { plan_id: 'pending', started_at: '2026-09-08', items: [] } as PendingPlan;
    c.pendingPlan = plan;
    await c.doResume();
    expect(c.pendingPlan?.plan_id).toBe('pending');
    expect(c.resumePhase).toBe('idle');
    expect(requestAuth).toHaveBeenCalledWith('needs_unlock');
  });

  it('does not report a failed logout as success or discard the review', async () => {
    const c = new ListingController({ getPendingPlan: vi.fn().mockResolvedValue(null), resumePendingPlan: vi.fn(), discardPendingPlan: vi.fn(), status: async () => { throw new Error('unavailable'); }, logout: async () => { throw new Error('failed'); } }, vi.fn());
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

it('preserves the saved inventory when native normalization fails', async () => {
  const log = vi.spyOn(console, 'error').mockImplementation(() => {});
  try {
    const storage = store(snapshot);
    const c = new InventoryController(storage, {
      fetchInventory: vi.fn(), loadCachedMarket: vi.fn(), refreshMarket: vi.fn(), reportScanIssue: vi.fn(),
    }, { ...sources, normalizeInventory: async () => { throw new Error('Inventory count is invalid'); } });
    c.market = { items: {} } as import('../../contracts/data').Market;
    c.catalogs = { uniqueToInfo: new Map() };
    await c.handleInventory({ name: 'scan', data: {} });
    expect(c.phase).toBe('error');
    expect(c.error).toBe('Inventory count is invalid');
    expect(storage.saveSnapshot).not.toHaveBeenCalled();
  } finally { log.mockRestore(); }
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  return { promise: new Promise<T>((yes, no) => { resolve = yes; reject = no; }), resolve, reject };
}
function normalized(name: string) {
  return { owned: new Map([[name, { ...snapshot.owned.get('item')!, slug: name, name }]]), unresolved: {}, flatCount: 1 };
}
function normalizationController(normalizeInventory: typeof sources.normalizeInventory, storage = store()) {
  const c = new InventoryController(storage, { fetchInventory: vi.fn(), loadCachedMarket: vi.fn(), refreshMarket: vi.fn(), reportScanIssue: vi.fn() }, { ...sources, normalizeInventory });
  c.catalogs = { uniqueToInfo: new Map() };
  c.market = { items: {} } as import('../../contracts/data').Market;
  return c;
}

describe('inventory replacement races', () => {
  it('keeps the newer normalized inventory when responses arrive out of order', async () => {
    const first = deferred<ReturnType<typeof normalized>>();
    const second = deferred<ReturnType<typeof normalized>>();
    const normalize = vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const storage = store();
    const c = normalizationController(normalize, storage);
    const old = c.handleInventory({ name: 'old', data: {} });
    await vi.waitFor(() => expect(normalize).toHaveBeenCalledTimes(1));
    const newer = c.handleInventory({ name: 'new', data: {} });
    await vi.waitFor(() => expect(normalize).toHaveBeenCalledTimes(2));
    second.resolve(normalized('new'));
    await newer;
    first.resolve(normalized('old'));
    await old;
    expect(c.inventoryName).toBe('new');
    expect([...c.resolved.owned.keys()]).toEqual(['new']);
    expect(storage.saveSnapshot).toHaveBeenCalledTimes(1);
  });

  it('ignores a normalization failure after a newer import succeeds', async () => {
    const pending = deferred<ReturnType<typeof normalized>>();
    const normalize = vi.fn(() => pending.promise);
    const c = normalizationController(normalize);
    const scan = c.handleInventory({ name: 'scan', data: {} });
    await vi.waitFor(() => expect(normalize).toHaveBeenCalled());
    await c.handleImported({ invName: 'import', ts: 123, ownedMap: normalized('import').owned });
    pending.reject(new Error('stale failure'));
    await scan;
    expect(c.phase).toBe('done');
    expect(c.error).toBeNull();
    expect(c.inventoryName).toBe('import');
    expect([...c.resolved.owned.keys()]).toEqual(['import']);
  });

  it('does not persist or show a normalized inventory after clearing', async () => {
    const pending = deferred<ReturnType<typeof normalized>>();
    const normalize = vi.fn(() => pending.promise);
    const storage = store();
    const c = normalizationController(normalize, storage);
    const scan = c.handleInventory({ name: 'scan', data: {} });
    await vi.waitFor(() => expect(normalize).toHaveBeenCalled());
    await c.clear();
    pending.resolve(normalized('stale'));
    await scan;
    expect(c.phase).toBe('idle');
    expect(c.inventoryName).toBeNull();
    expect(c.resolved.owned.size).toBe(0);
    expect(storage.saveSnapshot).not.toHaveBeenCalled();
    expect(storage.clearSnapshot).toHaveBeenCalledOnce();
  });

  it('orders Clear after an already dispatched snapshot write', async () => {
    const saving = deferred<void>();
    const writes: string[] = [];
    const storage = store();
    storage.saveSnapshot = vi.fn(async () => { await saving.promise; writes.push('save'); });
    storage.clearSnapshot = vi.fn(async () => { writes.push('clear'); });
    const c = normalizationController(vi.fn(async () => normalized('scan')), storage);
    const scan = c.handleInventory({ name: 'scan', data: {} });
    await vi.waitFor(() => expect(storage.saveSnapshot).toHaveBeenCalled());
    const clearing = c.clear();
    saving.resolve();
    await Promise.all([scan, clearing]);
    expect(writes).toEqual(['save', 'clear']);
    expect(c.phase).toBe('idle');
    expect(c.resolved.owned.size).toBe(0);
  });

  it('ignores acquired inventory after a newer import without releasing the acquisition guard early', async () => {
    const acquisition = deferred<unknown>();
    const normalize = vi.fn();
    const fetchInventory = vi.fn(() => acquisition.promise);
    const c = new InventoryController(store(), { fetchInventory, loadCachedMarket: vi.fn(), refreshMarket: vi.fn(), reportScanIssue: vi.fn() }, { ...sources, normalizeInventory: normalize });
    c.market = { items: {} } as import('../../contracts/data').Market;
    const scan = c.pullInventory();
    await c.handleImported({ invName: 'import', ts: 123, ownedMap: normalized('import').owned });
    await c.pullInventory();
    expect(fetchInventory).toHaveBeenCalledOnce();
    acquisition.resolve({});
    await scan;
    expect(normalize).not.toHaveBeenCalled();
    expect(c.inventoryName).toBe('import');
    expect(c.pullingInventory).toBe(false);
  });
});

it('ignores a late startup restore after the user has imported an inventory', async () => {
  const c = normalizationController(vi.fn(), store(snapshot));
  await c.handleImported({ invName: 'import', ts: 123, ownedMap: normalized('import').owned });
  await c.restore();
  expect(c.inventoryName).toBe('import');
  expect([...c.resolved.owned.keys()]).toEqual(['import']);
});

it('cannot restore a saved snapshot after Clear while its read is pending', async () => {
  const read = deferred<Snapshot | null>();
  const storage = store();
  storage.loadSnapshot = vi.fn(() => read.promise);
  const c = normalizationController(vi.fn(), storage);
  const restoring = c.restore();
  await c.clear();
  read.resolve(snapshot);
  await restoring;
  expect(c.inventoryName).toBeNull();
  expect(c.resolved.owned.size).toBe(0);
  expect(c.phase).toBe('idle');
});

it('reports an active failed import while keeping prior inventory after superseding a scan', async () => {
  const pending = deferred<ReturnType<typeof normalized>>();
  const normalize = vi.fn(() => pending.promise);
  const storage = store(snapshot);
  storage.saveSnapshot = vi.fn(async () => { throw new Error('Snapshot write failed'); });
  const c = normalizationController(normalize, storage);
  c.resolved = { owned: snapshot.owned, unresolved: {} };
  c.inventoryName = 'saved';
  c.lastUpdated = snapshot.ts;
  const scan = c.handleInventory({ name: 'scan', data: {} });
  await vi.waitFor(() => expect(normalize).toHaveBeenCalled());
  await expect(c.handleImported({ invName: 'import', ts: 123, ownedMap: normalized('import').owned })).rejects.toThrow('Snapshot write failed');
  expect(c.phase).toBe('error');
  expect(c.error).toBe('Snapshot write failed');
  expect(c.inventoryName).toBe('saved');
  expect(c.resolved.owned).toEqual(snapshot.owned);
  expect(c.lastUpdated).toBe(snapshot.ts);
  pending.resolve(normalized('stale scan'));
  await scan;
  expect(c.phase).toBe('error');
  expect(c.resolved.owned).toEqual(snapshot.owned);
  expect(storage.saveSnapshot).toHaveBeenCalledOnce();
});

for (const outcome of ['invalid', 'empty', 'untradeable', 'persistence'] as const) {
  it(`retains the last successful inventory identity and quantities after ${outcome} replacement`, async () => {
    const log = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      const storage = store(snapshot);
      const normalize = vi.fn(async () => {
        if (outcome === 'invalid') throw new Error('Inventory count is invalid');
        if (outcome === 'empty' || outcome === 'untradeable') return { owned: new Map(), unresolved: {}, flatCount: outcome === 'empty' ? 0 : 4 };
        return normalized('replacement');
      });
      if (outcome === 'persistence') storage.saveSnapshot = vi.fn().mockRejectedValue(new Error('Storage unavailable'));
      const c = normalizationController(normalize, storage);
      await c.restore();
      await c.handleInventory({ name: 'replacement', data: {} });
      expect(c.inventoryName).toBe(snapshot.invName);
      expect(c.lastUpdated).toBe(snapshot.ts);
      expect(c.resolved.owned).toEqual(snapshot.owned);
      expect(c.error ?? c.pullError).toBeTruthy();
      expect(c.refreshFailed).toBe(true);
      c.error = null; c.pullError = null;
      expect(c.refreshFailed).toBe(true);
      expect(storage.clearSnapshot).not.toHaveBeenCalled();
      await c.clear();
      expect(c.refreshFailed).toBe(false);
    } finally { log.mockRestore(); }
  });
}

it('makes a failed snapshot read recoverable and ignores a read superseded by Clear', async () => {
  const log = vi.spyOn(console, 'error').mockImplementation(() => {});
  try {
    const storage = store();
    storage.loadSnapshot = vi.fn().mockRejectedValue(new Error('Saved inventory unavailable'));
    const c = normalizationController(vi.fn(), storage);
    await c.restore();
    expect(c.phase).toBe('error');
    expect(c.error).toBe('Saved inventory unavailable');
    const pending = deferred<Snapshot | null>();
    const nextStore = store();
    nextStore.loadSnapshot = vi.fn(() => pending.promise);
    const next = normalizationController(vi.fn(), nextStore);
    const restoring = next.restore();
    await next.clear();
    pending.reject(new Error('obsolete read'));
    await restoring;
    expect(next.phase).toBe('idle');
    expect(next.error).toBeNull();
  } finally { log.mockRestore(); }
});

it('a successful import clears the failed-refresh marker and preserves its original timestamp', async () => {
  const storage = store();
  const c = normalizationController(vi.fn(), storage);
  c.refreshFailed = true;
  await c.handleImported({ invName: 'older import', ts: 123, ownedMap: normalized('import').owned });
  expect(c.refreshFailed).toBe(false);
  expect(c.source).toBe('import');
  expect(c.lastUpdated).toBe(123);
  expect(storage.saveSnapshot).toHaveBeenCalledWith({ invName: 'older import', owned: normalized('import').owned }, 123);
});
