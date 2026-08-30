// Persists the last-processed inventory so a page refresh keeps the table.
// We only store the resolved owned-items map + metadata (~50 KB for a
// real inventory), not the raw 2 MB inventory.json - small enough for
// localStorage and avoids re-resolving on each page load.

// v4: also persist kept_lvl. v3 dropped it, so on every page reload the restored
// records had kept_lvl===undefined and the leveled-mod hide guard
// (rec.kept_lvl !== null && rec.kept_lvl >= hideAtLvl) matched nothing - a mod
// you've leveled into your build reappeared as "safe to sell". Old v2/v3
// snapshots are silently invalidated.
//
// v5: also persist `leveled` (count of owned instances with XP > 0 - copies
// Warframe has flagged untradeable). Without the bump, a restored v4 snapshot
// would read `leveled` as undefined on every row, and `sellableQty` would
// treat genuinely-leveled gear as fully sellable until the next inventory
// pull - the same silent-danger shape as the v3→v4 kept_lvl bug, but here it
// risks listing a copy that can't actually be traded. Old v4 snapshots are
// silently invalidated; reloading the inventory recomputes `leveled` fresh.
//
// v6: also persist `rivens` (the parsed riven list from Upgrades[]). The
// Rivens view needs the fingerprints - extracting them again on reload would
// require the raw 2 MB inventory, which the snapshot deliberately doesn't
// keep. Persisted unresolved (compat path, no slug) and resolved against the
// current market at render. Old v5 snapshots are silently invalidated;
// reloading the inventory recomputes `rivens` fresh.
import type { OwnedRiven } from './rivens';
import type { OwnedRecord } from './types';

const KEY = 'wfminv:last-owned-v6';

export interface Snapshot {
  ts: number;
  invName: string;
  owned: Map<string, OwnedRecord>;
  rivens: OwnedRiven[];
}

export interface SaveSnapshotInput {
  invName: string;
  owned: Map<string, OwnedRecord>;
  /** Parsed rivens from the scanned inventory; absent on older callers
   *  (and older snapshots) means no rivens saved yet. */
  rivens?: OwnedRiven[];
}

// The single source of truth for the on-the-wire snapshot shape: the
// localStorage store (below), the desktop SQLite store (state-store.ts), and
// the encrypted export (ExportImportDialogs.svelte) all build their payload
// here, so all three carry byte-identical records.
//
// The export used to re-list these seven fields itself. Nothing would have
// caught the drift: add a field to OwnedRecord, wire it into the stores,
// forget the export, and every export silently loses it - discovered only by
// a user importing on another machine and finding data missing.
export function buildSnapshotPayload(
  { invName, owned, rivens }: SaveSnapshotInput,
  ts: number,
): { ts: number; invName: string; owned: Array<[string, Record<string, unknown>]>; rivens: OwnedRiven[] } {
  return {
    ts,
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
    rivens: rivens ?? [],
  };
}

export function serializeSnapshot(input: SaveSnapshotInput): string {
  return JSON.stringify(buildSnapshotPayload(input, Date.now()));
}

export function deserializeSnapshot(raw: string | null): Snapshot | null {
  if (!raw) return null;
  const p = JSON.parse(raw);
  return {
    ts: p.ts,
    invName: p.invName,
    owned: new Map<string, OwnedRecord>(p.owned),
    rivens: Array.isArray(p.rivens) ? (p.rivens as OwnedRiven[]) : [],
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
