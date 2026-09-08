import { readCached, writeCached, purgeRetiredCaches, type SlimCatalog } from './catalog-cache';
import type { Catalogs } from '../domain/resolver';

const WFSTAT_CATALOG_URL = '/wfstat-catalog.json';

export async function loadCatalogs(): Promise<Catalogs> {
  // Reclaim rows from retired cache keys. Fire-and-forget: bumping the key
  // invalidates the old row but leaves it on disk, and this is the only code
  // path that still knows the old names.
  void purgeRetiredCaches();

  // Cheap path: IndexedDB cache, 24 h TTL. The slim form holds only the
  // (uniqueName, name, category) triples (~17k entries) we actually need -
  // keeps the stored payload to a fraction of warframestat.us's ~5 MB raw.
  const cached = await readCached();
  if (cached && Array.isArray(cached)) {
    return { uniqueToInfo: new Map(cached) };
  }

  const r = await fetch(WFSTAT_CATALOG_URL);
  if (!r.ok) throw new Error(`wfstat-catalog.json responded ${r.status} - rebuild the snapshot (wfm-scrape build)`);
  // Already in slim [uniqueName, {name, category}] form - baked that way.
  const slim = (await r.json()) as SlimCatalog;
  if (!Array.isArray(slim)) throw new Error('wfstat-catalog.json is not an array');

  // Fire-and-forget; we don't want to block first paint on the write.
  void writeCached(slim);
  return { uniqueToInfo: new Map(slim) };
}
