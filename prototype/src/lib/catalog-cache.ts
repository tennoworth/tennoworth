// IndexedDB cache for the warframestat.us items list. That payload is ~5 MB
// and well over localStorage's quota, but compresses cheaply and almost
// never changes day-to-day. Caching it cuts page-load latency from
// "5 MB network round-trip" to "a few hundred KB from IDB" after first use.

import type { SlimItemInfo } from './types';

const DB_NAME = 'wfminv';
const DB_VERSION = 1;
const STORE = 'catalogs';
// v3: sourced from the baked same-origin wfstat-catalog.json (forced
// English). v2 caches could hold Accept-Language-localized names that
// never matched the WFM catalog - invalidate them.
const KEY = 'wfstat-items-v3';
// Bumping KEY invalidates the old row but cannot delete it: nothing reads a
// key it no longer knows. Every key this cache has ever used therefore has to
// stay listed here, or its rows sit in the user's IndexedDB forever - v2 rows
// are on real installs right now, hundreds of KB each, unreachable by any code
// path. Add the outgoing key here whenever you bump KEY.
const RETIRED_KEYS = ['wfstat-items-v1', 'wfstat-items-v2'];
const TTL_MS = 24 * 60 * 60 * 1000; // 24 h

export type SlimCatalog = Array<[string, SlimItemInfo]>;

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, DB_VERSION);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function withStore<T>(
  mode: IDBTransactionMode,
  fn: (store: IDBObjectStore) => Promise<T> | T,
): Promise<T> {
  const db = await openDb();
  return new Promise<T>((resolve, reject) => {
    const tx = db.transaction(STORE, mode);
    const store = tx.objectStore(STORE);
    let result: T;
    Promise.resolve(fn(store))
      .then((r) => (result = r))
      .catch(() => tx.abort());
    tx.oncomplete = () => resolve(result);
    tx.onerror = () => reject(tx.error);
    tx.onabort = () => reject(tx.error);
  });
}

export async function readCached(): Promise<SlimCatalog | null> {
  try {
    const entry = await withStore<{ ts: number; data: SlimCatalog } | undefined>(
      'readonly',
      (store) =>
        new Promise((resolve, reject) => {
          const req = store.get(KEY);
          req.onsuccess = () => resolve(req.result as { ts: number; data: SlimCatalog } | undefined);
          req.onerror = () => reject(req.error);
        }),
    );
    if (!entry) return null;
    if (Date.now() - entry.ts > TTL_MS) {
      // Drop it rather than leaving a stale multi-hundred-KB row parked until
      // the next successful write happens to overwrite it - which never comes
      // if the user stops loading inventories.
      void clearCached();
      return null;
    }
    return entry.data;
  } catch (e) {
    console.warn('catalog cache read failed:', e);
    return null;
  }
}

export async function writeCached(data: SlimCatalog): Promise<void> {
  try {
    await withStore('readwrite', (store) =>
      new Promise<void>((resolve, reject) => {
        const req = store.put({ ts: Date.now(), data }, KEY);
        req.onsuccess = () => resolve();
        req.onerror = () => reject(req.error);
      }),
    );
  } catch (e) {
    console.warn('catalog cache write failed:', e);
  }
}

/** Delete the current cache row. Best-effort: a failure here is not worth
 *  surfacing, the cache is an optimization. */
export async function clearCached(): Promise<void> {
  await deleteKeys([KEY]);
}

/** Reclaim rows left behind by earlier cache-key versions. Called once at
 *  boot from the resolver - best-effort and non-blocking, so a browser with a
 *  broken IDB simply keeps the wasted space rather than failing a page load. */
export async function purgeRetiredCaches(): Promise<void> {
  await deleteKeys(RETIRED_KEYS);
}

async function deleteKeys(keys: string[]): Promise<void> {
  try {
    await withStore('readwrite', (store) =>
      Promise.all(
        keys.map(
          (k) =>
            new Promise<void>((resolve, reject) => {
              const req = store.delete(k);
              req.onsuccess = () => resolve();
              req.onerror = () => reject(req.error);
            }),
        ),
      ).then(() => undefined),
    );
  } catch {
    /* ignore */
  }
}
