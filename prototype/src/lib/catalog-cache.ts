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
// never matched the WFM catalog — invalidate them.
const KEY = 'wfstat-items-v3';
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
    if (Date.now() - entry.ts > TTL_MS) return null;
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

export async function clearCached(): Promise<void> {
  try {
    await withStore('readwrite', (store) =>
      new Promise<void>((resolve, reject) => {
        const req = store.delete(KEY);
        req.onsuccess = () => resolve();
        req.onerror = () => reject(req.error);
      }),
    );
  } catch {
    /* ignore */
  }
}
