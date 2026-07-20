// State-store abstraction: the SPA's single seam between "persist to
// localStorage" (hosted / `serve` browser build) and "persist to the canonical
// SQLite store over Tauri IPC" (desktop build). Selected ONCE at boot by the
// same runtime sniff the transport uses — see `createStateStore()`.
//
// It covers EXACTLY the state the SPA persists today, no more:
//   - four scalar settings (reserve-copies, filters-open, view,
//     score-explainer-dismissed), each a short string;
//   - the last-owned inventory snapshot (the reload-restore copy).
//
// The desktop `snapshot` / `snapshot_item` history tables are a separate
// concern — appended by the `scan_inventory` / `import_snapshot` commands, not
// by this store's snapshot methods, which persist the display-restore copy.
// `recordImportSnapshot` is the one bridge to that history: a file-drop in the
// desktop build appends an `import` history row (a no-op in the browser, which
// has no history substrate).
//
// LocalStorageStateStore is byte-for-byte the pre-store behaviour: the same
// localStorage keys, the same value encodings, and the snapshot round-trip
// delegated verbatim to storage.ts. TauriStateStore persists the SAME serialized
// snapshot bytes (serializeSnapshot) into the SQLite `setting` table — only the
// backing store differs.

import {
  serializeSnapshot,
  deserializeSnapshot,
  loadSnapshot as loadLocalSnapshot,
  saveSnapshot as saveLocalSnapshot,
  clearSnapshot as clearLocalSnapshot,
  type Snapshot,
  type SaveSnapshotInput,
} from './storage';
import { isDesktopRuntime, resolveInvoke } from './transport';

export type { Snapshot, SaveSnapshotInput } from './storage';

// The scalar settings the SPA persists. The value is always a short string; the
// caller owns parsing/validation (the parseInt guard, the VALID_VIEWS set), so
// the store stays a dumb, byte-faithful key/value — it never interprets a value.
export type SettingKey =
  | 'reserve-copies'
  | 'filters-open'
  | 'view'
  | 'score-explainer-dismissed';

// The historical localStorage key each setting has always used. Only the
// browser store carries these `wfminv:…-vN` names (so existing data keeps
// loading); the desktop store keys its SQLite `setting` rows off the bare
// SettingKey. Bump a suffix here (not the SettingKey) if a value's shape ever
// changes, exactly as the pre-store code did.
const LOCAL_SETTING_KEYS: Record<SettingKey, string> = {
  'reserve-copies': 'wfminv:reserve-copies-v1',
  'filters-open': 'wfminv:filters-open-v1',
  view: 'wfminv:view-v1',
  'score-explainer-dismissed': 'wfminv:score-explainer-dismissed-v1',
};

// The SQLite `setting.key` the desktop store parks the reload-restore snapshot
// under. Distinct from the `snapshot`/`snapshot_item` history tables.
const DESKTOP_SNAPSHOT_KEY = 'last-owned';

export interface StateStore {
  readonly mode: 'local' | 'tauri';

  /**
   * Load every scalar setting into an in-memory cache so `getSetting` can be
   * read synchronously at component-init time — no default-value flash for a
   * returning user. Called once at boot BEFORE the Svelte app mounts. Must never
   * reject: a backing-store failure leaves the cache empty and every
   * `getSetting` falls back to its caller default.
   */
  hydrate(): Promise<void>;

  /** Synchronous after `hydrate()`. `null` = unset (the caller applies its default). */
  getSetting(key: SettingKey): string | null;
  setSetting(key: SettingKey, value: string): Promise<void>;

  loadSnapshot(): Promise<Snapshot | null>;
  saveSnapshot(input: SaveSnapshotInput): Promise<void>;
  clearSnapshot(): Promise<void>;

  /**
   * Append a `source='import'` history snapshot from a dropped inventory.json.
   * Desktop-only substrate: a no-op in the browser. Best-effort by contract —
   * losing a history row must never break the user's file-drop, so callers
   * swallow a rejection.
   */
  recordImportSnapshot(inventoryJson: string): Promise<void>;
}

/**
 * Hosted / `serve` browser build. Behaviour is verbatim the pre-store code:
 * scalars read/written under their historical localStorage keys, the snapshot
 * round-tripped through storage.ts unchanged. localStorage reads are already
 * synchronous, so `hydrate` is a no-op and `getSetting` reads live.
 */
export class LocalStorageStateStore implements StateStore {
  readonly mode = 'local' as const;

  async hydrate(): Promise<void> {
    /* localStorage is synchronous — nothing to prime. */
  }

  getSetting(key: SettingKey): string | null {
    try {
      return localStorage.getItem(LOCAL_SETTING_KEYS[key]);
    } catch {
      return null;
    }
  }

  async setSetting(key: SettingKey, value: string): Promise<void> {
    try {
      localStorage.setItem(LOCAL_SETTING_KEYS[key], value);
    } catch {
      /* quota / disabled storage — match the pre-store best-effort writes. */
    }
  }

  async loadSnapshot(): Promise<Snapshot | null> {
    return loadLocalSnapshot();
  }

  async saveSnapshot(input: SaveSnapshotInput): Promise<void> {
    saveLocalSnapshot(input);
  }

  async clearSnapshot(): Promise<void> {
    clearLocalSnapshot();
  }

  async recordImportSnapshot(): Promise<void> {
    /* No history substrate in the browser — snapshot history is desktop-only. */
  }
}

/**
 * Desktop build. Every method is a wfm-core-backed command over Tauri IPC. The
 * scalar settings ride the `setting` KV table (get_setting/set_setting); the
 * reload-restore snapshot rides the same table under DESKTOP_SNAPSHOT_KEY,
 * serialized with the SAME bytes the browser writes. `recordImportSnapshot`
 * calls `import_snapshot`, which appends to the `snapshot` history tables.
 */
export class TauriStateStore implements StateStore {
  readonly mode = 'tauri' as const;
  #cache = new Map<SettingKey, string | null>();

  async hydrate(): Promise<void> {
    const invoke = resolveInvoke();
    const keys: SettingKey[] = [
      'reserve-copies',
      'filters-open',
      'view',
      'score-explainer-dismissed',
    ];
    await Promise.all(
      keys.map(async (key) => {
        try {
          const value = await invoke<string | null>('get_setting', { key });
          this.#cache.set(key, value ?? null);
        } catch {
          // A failed read leaves the key absent → getSetting returns null → the
          // caller's default applies. Booting with defaults beats not booting.
          this.#cache.set(key, null);
        }
      }),
    );
  }

  getSetting(key: SettingKey): string | null {
    return this.#cache.get(key) ?? null;
  }

  async setSetting(key: SettingKey, value: string): Promise<void> {
    // Update the cache first so an immediately-following synchronous read is
    // consistent even if the IPC write is still in flight.
    this.#cache.set(key, value);
    await resolveInvoke()<void>('set_setting', { key, value });
  }

  async loadSnapshot(): Promise<Snapshot | null> {
    const raw = await resolveInvoke()<string | null>('get_setting', {
      key: DESKTOP_SNAPSHOT_KEY,
    });
    return deserializeSnapshot(raw ?? null);
  }

  async saveSnapshot(input: SaveSnapshotInput): Promise<void> {
    await resolveInvoke()<void>('set_setting', {
      key: DESKTOP_SNAPSHOT_KEY,
      value: serializeSnapshot(input),
    });
  }

  async clearSnapshot(): Promise<void> {
    // There is no delete_setting command; an empty value deserializes back to
    // null (deserializeSnapshot treats '' as absent), which is the clear.
    await resolveInvoke()<void>('set_setting', {
      key: DESKTOP_SNAPSHOT_KEY,
      value: '',
    });
  }

  async recordImportSnapshot(inventoryJson: string): Promise<void> {
    // Tauri v2 maps the camelCase JS key to the snake_case `inventory_json`
    // command parameter.
    await resolveInvoke()<number>('import_snapshot', { inventoryJson });
  }
}

/**
 * Boot-time store selection — the desktop store inside the Tauri webview, the
 * localStorage store everywhere else. Keyed off the same `__TAURI_INTERNALS__`
 * sniff as the transport so the two seams always agree on which build we're in.
 */
export function createStateStore(): StateStore {
  return isDesktopRuntime() ? new TauriStateStore() : new LocalStorageStateStore();
}
