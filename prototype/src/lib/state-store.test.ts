// @ts-nocheck — vitest fixtures; the store's TS contract is exercised by tsc.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import {
  createStateStore,
  LocalStorageStateStore,
  TauriStateStore,
} from './state-store.js';

// TauriStateStore + the desktop sniff read the Tauri globals; install/remove
// them per test so the two modes stay isolated (same pattern as transport.test).
function installTauri(invoke) {
  globalThis.__TAURI_INTERNALS__ = { invoke };
  globalThis.__TAURI__ = { core: { invoke } };
}
function removeTauri() {
  delete globalThis.__TAURI_INTERNALS__;
  delete globalThis.__TAURI__;
}

beforeEach(() => {
  localStorage.clear();
  removeTauri();
});
afterEach(() => {
  removeTauri();
  vi.restoreAllMocks();
});

describe('createStateStore selection', () => {
  it('returns the localStorage store in a browser (no Tauri)', () => {
    const s = createStateStore();
    expect(s).toBeInstanceOf(LocalStorageStateStore);
    expect(s.mode).toBe('local');
  });

  it('returns the Tauri store inside the desktop webview', () => {
    installTauri(vi.fn());
    const s = createStateStore();
    expect(s).toBeInstanceOf(TauriStateStore);
    expect(s.mode).toBe('tauri');
  });
});

describe('LocalStorageStateStore — key/shape parity with the pre-store code', () => {
  // These pin the exact localStorage keys and value encodings the SPA has
  // always written. A returning user's data lives under these names; a
  // rename here silently invalidates it, so this is the guard against that.
  const CASES = [
    ['reserve-copies', 'wfminv:reserve-copies-v1', '3'],
    ['filters-open', 'wfminv:filters-open-v1', '1'],
    ['view', 'wfminv:view-v1', 'relics'],
    ['score-explainer-dismissed', 'wfminv:score-explainer-dismissed-v1', '1'],
  ];

  it.each(CASES)('setSetting(%s) writes the historical localStorage key verbatim', async (key, lsKey, value) => {
    const s = new LocalStorageStateStore();
    await s.setSetting(key, value);
    expect(localStorage.getItem(lsKey)).toBe(value);
  });

  it.each(CASES)('getSetting(%s) reads a value written by the old direct-localStorage code', async (key, lsKey, value) => {
    // Simulate data an older build left behind.
    localStorage.setItem(lsKey, value);
    const s = new LocalStorageStateStore();
    await s.hydrate();
    expect(s.getSetting(key)).toBe(value);
  });

  it('getSetting returns null for an unset key (caller applies its default)', () => {
    const s = new LocalStorageStateStore();
    expect(s.getSetting('view')).toBeNull();
  });

  it('setSetting swallows a quota/disabled-storage write failure', async () => {
    const orig = Storage.prototype.setItem;
    Storage.prototype.setItem = () => { throw new DOMException('QuotaExceededError'); };
    try {
      const s = new LocalStorageStateStore();
      await expect(s.setSetting('view', 'sets')).resolves.toBeUndefined();
    } finally {
      Storage.prototype.setItem = orig;
    }
  });

  it('snapshot round-trips through the historical wfminv:last-owned-v5 key', async () => {
    const s = new LocalStorageStateStore();
    const owned = new Map([
      ['vitality|', { count: 51, name: 'Vitality', type: 'Mods', slug: 'vitality', subtype: null, kept_lvl: null, leveled: 0 }],
    ]);
    await s.saveSnapshot({ invName: 'inventory.json', owned });
    // Written under the exact key storage.ts uses.
    expect(localStorage.getItem('wfminv:last-owned-v5')).not.toBeNull();
    const got = await s.loadSnapshot();
    expect(got.invName).toBe('inventory.json');
    expect(got.owned).toBeInstanceOf(Map);
    expect(got.owned.get('vitality|').count).toBe(51);
    await s.clearSnapshot();
    expect(await s.loadSnapshot()).toBeNull();
  });

  it('recordImportSnapshot is a no-op in the browser (no history substrate)', async () => {
    const s = new LocalStorageStateStore();
    await expect(s.recordImportSnapshot('{"MiscItems":[]}')).resolves.toBeUndefined();
  });
});

describe('TauriStateStore — command mapping', () => {
  it('hydrate() reads every scalar setting via get_setting and caches it', async () => {
    const invoke = vi.fn(async (_cmd, args) => {
      const table = {
        'reserve-copies': '2',
        view: 'baro',
      };
      return table[args.key] ?? null;
    });
    installTauri(invoke);
    const s = new TauriStateStore();
    await s.hydrate();
    expect(invoke).toHaveBeenCalledWith('get_setting', { key: 'reserve-copies' });
    expect(invoke).toHaveBeenCalledWith('get_setting', { key: 'filters-open' });
    expect(invoke).toHaveBeenCalledWith('get_setting', { key: 'view' });
    expect(invoke).toHaveBeenCalledWith('get_setting', { key: 'score-explainer-dismissed' });
    // getSetting is synchronous after hydrate, served from the cache.
    expect(s.getSetting('reserve-copies')).toBe('2');
    expect(s.getSetting('view')).toBe('baro');
    expect(s.getSetting('filters-open')).toBeNull();
  });

  it('hydrate() never rejects — a failing read leaves the key null (default applies)', async () => {
    const invoke = vi.fn().mockRejectedValue('db error');
    installTauri(invoke);
    const s = new TauriStateStore();
    await expect(s.hydrate()).resolves.toBeUndefined();
    expect(s.getSetting('view')).toBeNull();
  });

  it('setSetting invokes set_setting and updates the cache immediately', async () => {
    const invoke = vi.fn().mockResolvedValue(undefined);
    installTauri(invoke);
    const s = new TauriStateStore();
    await s.setSetting('view', 'relics');
    expect(invoke).toHaveBeenCalledWith('set_setting', { key: 'view', value: 'relics' });
    // Synchronous read reflects the write without another IPC round-trip.
    expect(s.getSetting('view')).toBe('relics');
  });

  it('saveSnapshot serializes to the last-owned setting; loadSnapshot deserializes it back', async () => {
    let stored = null;
    const invoke = vi.fn(async (cmd, args) => {
      if (cmd === 'set_setting' && args.key === 'last-owned') { stored = args.value; return; }
      if (cmd === 'get_setting' && args.key === 'last-owned') return stored;
      return null;
    });
    installTauri(invoke);
    const s = new TauriStateStore();
    const owned = new Map([
      ['broken_war|', { count: 3, name: 'Broken War', type: 'Melee', slug: 'broken_war', subtype: null, kept_lvl: null, leveled: 2 }],
    ]);
    await s.saveSnapshot({ invName: 'inv.json', owned });
    expect(typeof stored).toBe('string');
    const got = await s.loadSnapshot();
    expect(got.invName).toBe('inv.json');
    expect(got.owned.get('broken_war|').leveled).toBe(2);
  });

  it('loadSnapshot returns null when the setting is unset', async () => {
    const invoke = vi.fn().mockResolvedValue(null);
    installTauri(invoke);
    const s = new TauriStateStore();
    expect(await s.loadSnapshot()).toBeNull();
  });

  it('clearSnapshot writes an empty last-owned value (which deserializes to null)', async () => {
    let stored = 'something';
    const invoke = vi.fn(async (cmd, args) => {
      if (cmd === 'set_setting' && args.key === 'last-owned') { stored = args.value; return; }
      if (cmd === 'get_setting' && args.key === 'last-owned') return stored;
      return null;
    });
    installTauri(invoke);
    const s = new TauriStateStore();
    await s.clearSnapshot();
    expect(stored).toBe('');
    expect(await s.loadSnapshot()).toBeNull();
  });

  it('recordImportSnapshot invokes import_snapshot with the camelCase inventoryJson arg', async () => {
    const invoke = vi.fn().mockResolvedValue(7);
    installTauri(invoke);
    const s = new TauriStateStore();
    await s.recordImportSnapshot('{"MiscItems":[]}');
    expect(invoke).toHaveBeenCalledWith('import_snapshot', { inventoryJson: '{"MiscItems":[]}' });
  });
});
