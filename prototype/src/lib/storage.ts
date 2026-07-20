// Persists the last-processed inventory so a page refresh keeps the table.
// We only store the resolved owned-items map + metadata (~50 KB for a
// real inventory), not the raw 2 MB inventory.json — small enough for
// localStorage and avoids re-resolving on each page load.

// v4: also persist kept_lvl. v3 dropped it, so on every page reload the restored
// records had kept_lvl===undefined and the leveled-mod hide guard
// (rec.kept_lvl !== null && rec.kept_lvl >= hideAtLvl) matched nothing — a mod
// you've leveled into your build reappeared as "safe to sell". Old v2/v3
// snapshots are silently invalidated.
//
// v5: also persist `leveled` (count of owned instances with XP > 0 — copies
// Warframe has flagged untradeable). Without the bump, a restored v4 snapshot
// would read `leveled` as undefined on every row, and `sellableQty` would
// treat genuinely-leveled gear as fully sellable until the next inventory
// pull — the same silent-danger shape as the v3→v4 kept_lvl bug, but here it
// risks listing a copy that can't actually be traded. Old v4 snapshots are
// silently invalidated; reloading the inventory recomputes `leveled` fresh.
import type { OwnedRecord } from './types';

const KEY = 'wfminv:last-owned-v5';

export interface Snapshot {
  ts: number;
  invName: string;
  owned: Map<string, OwnedRecord>;
}

export interface SaveSnapshotInput {
  invName: string;
  owned: Map<string, OwnedRecord>;
}

// Serialize/deserialize are the single source of truth for the on-the-wire
// snapshot shape, shared by the localStorage store (below) and the desktop
// SQLite store (state-store.ts) so both persist byte-identical payloads. Only
// the backing store differs — the bytes never do.
export function serializeSnapshot({ invName, owned }: SaveSnapshotInput): string {
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
        kept_lvl: rec.kept_lvl ?? null,
        leveled: rec.leveled ?? 0,
      },
    ]),
  };
  return JSON.stringify(payload);
}

export function deserializeSnapshot(raw: string | null): Snapshot | null {
  if (!raw) return null;
  const p = JSON.parse(raw);
  return {
    ts: p.ts,
    invName: p.invName,
    owned: new Map<string, OwnedRecord>(p.owned),
  };
}

export function saveSnapshot(input: SaveSnapshotInput): void {
  try {
    localStorage.setItem(KEY, serializeSnapshot(input));
  } catch (e) {
    console.warn('Could not persist inventory snapshot:', e);
  }
}

export function loadSnapshot(): Snapshot | null {
  try {
    return deserializeSnapshot(localStorage.getItem(KEY));
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
