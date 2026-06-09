// Persists the last-processed inventory so a page refresh keeps the table.
// We only store the resolved owned-items map + metadata (~50 KB for a
// real inventory), not the raw 2 MB inventory.json — small enough for
// localStorage and avoids re-resolving on each page load.

// v3: composite key per (slug, subtype) so each relic refinement is its own
// row. rec now carries `slug` and `subtype` separately; the Map key is
// `${slug}|${subtype ?? ''}`. Old v2 snapshots are silently invalidated.
import type { OwnedRecord } from './types';

const KEY = 'wfminv:last-owned-v3';

export interface Snapshot {
  ts: number;
  invName: string;
  owned: Map<string, OwnedRecord>;
}

export interface SaveSnapshotInput {
  invName: string;
  owned: Map<string, OwnedRecord>;
}

export function saveSnapshot({ invName, owned }: SaveSnapshotInput): void {
  try {
    const payload = {
      ts: Date.now(),
      invName,
      owned: [...owned.entries()].map(([key, rec]) => [
        key,
        {
          count: rec.count,
          name: rec.name,
          type: rec.type,
          slug: rec.slug,
          subtype: rec.subtype ?? null,
        },
      ]),
    };
    localStorage.setItem(KEY, JSON.stringify(payload));
  } catch (e) {
    console.warn('Could not persist inventory snapshot:', e);
  }
}

export function loadSnapshot(): Snapshot | null {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return null;
    const p = JSON.parse(raw);
    return {
      ts: p.ts,
      invName: p.invName,
      owned: new Map<string, OwnedRecord>(p.owned),
    };
  } catch (e) {
    console.warn('Could not load inventory snapshot:', e);
    return null;
  }
}

export function clearSnapshot(): void {
  try {
    localStorage.removeItem(KEY);
  } catch {
    /* ignore */
  }
}

// owned is Map<key, {count, slug, subtype, ...}> where key encodes both
// the slug and the subtype (so each relic refinement is its own entry).
// Returns Map<key, delta> for keys present in `current` whose count
// differs from `previous`. Negative delta = sold/consumed; positive = farmed.
export function diffOwned(
  previous: Map<string, OwnedRecord> | null | undefined,
  current: Map<string, OwnedRecord>,
): Map<string, number> {
  const out = new Map<string, number>();
  if (!previous) return out;
  for (const [key, rec] of current) {
    const prev = previous.get(key);
    const before = prev ? prev.count : 0;
    if (rec.count !== before) out.set(key, rec.count - before);
  }
  return out;
}
