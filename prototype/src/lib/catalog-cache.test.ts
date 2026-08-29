// IndexedDB-backed catalog cache: key versioning, TTL expiry, and the
// RETIRED_KEYS reclamation ("invalidation is not reclamation" - a bumped
// key never deletes its old row, only purgeRetiredCaches does).
import 'fake-indexeddb/auto';
import { describe, it, expect, beforeEach } from 'vitest';
import { readCached, writeCached, clearCached, purgeRetiredCaches } from './catalog-cache';

const TTL_MS = 24 * 60 * 60 * 1000;
const DB = 'wfminv';
const STORE = 'catalogs';

// Seed a row under a given key (raw IDB - the module's own API only ever
// touches its CURRENT key, which is exactly the point of the retired-keys
// tests).
async function seedRow(key: string, ts: number, data: unknown): Promise<void> {
  const db = await new Promise<IDBDatabase>((resolve, reject) => {
    const req = indexedDB.open(DB, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
  await new Promise<void>((resolve, reject) => {
    const tx = db.transaction(STORE, 'readwrite');
    tx.objectStore(STORE).put({ ts, data }, key);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
  db.close();
}

async function readKey(key: string): Promise<unknown> {
  const db = await new Promise<IDBDatabase>((resolve, reject) => {
    const req = indexedDB.open(DB, 1);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
  const value = await new Promise<unknown>((resolve, reject) => {
    const tx = db.transaction(STORE, 'readonly');
    const req = tx.objectStore(STORE).get(key);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
  db.close();
  return value;
}

beforeEach(async () => {
  await clearCached();
  await purgeRetiredCaches();
});

describe('catalog-cache', () => {
  it('round-trips a slim catalog through write + read', async () => {
    const data: [string, { name: string; category: string }][] = [
      ['/Lotus/Excalibur', { name: 'Excalibur', category: 'Suits' }],
    ];
    await writeCached(data);
    await expect(readCached()).resolves.toEqual(data);
  });

  it('returns null when nothing is cached', async () => {
    await expect(readCached()).resolves.toBeNull();
  });

  it('drops an expired row and reports null', async () => {
    await seedRow('wfstat-items-v3', Date.now() - TTL_MS - 60_000, [[
      '/Lotus/Stale', { name: 'Stale', category: 'MiscItems' },
    ]]);
    await expect(readCached()).resolves.toBeNull();
    await expect(readKey('wfstat-items-v3')).resolves.toBeUndefined();
  });

  it('purgeRetiredCaches reclaims old keys but leaves the live one', async () => {
    await seedRow('wfstat-items-v2', Date.now(), [['/Lotus/Dead', { name: 'Dead', category: 'MiscItems' }]]);
    await seedRow('wfstat-items-v1', Date.now(), [['/Lotus/Older', { name: 'Older', category: 'MiscItems' }]]);
    await writeCached([['/Lotus/Live', { name: 'Live', category: 'Suits' }]]);
    await purgeRetiredCaches();
    await expect(readKey('wfstat-items-v2')).resolves.toBeUndefined();
    await expect(readKey('wfstat-items-v1')).resolves.toBeUndefined();
    await expect(readCached()).resolves.toEqual([['/Lotus/Live', { name: 'Live', category: 'Suits' }]]);
  });
});
