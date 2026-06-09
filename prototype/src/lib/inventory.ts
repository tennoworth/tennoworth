// Inventory walker. Mirrors flatten_inventory() in wfm_inventory.py.

import type { Inventory, InventoryStackEntry, InventoryUpgrade } from './types';

export const TRADEABLE_CATEGORIES = [
  'MiscItems',
  'Recipes',
  'RawUpgrades',
  'Suits',
  'LongGuns',
  'Pistols',
  'Melee',
  'SpaceGuns',
  'SpaceMelee',
  'Sentinels',
  'SentinelWeapons',
] as const;

export interface FlatInventoryEntry {
  category: string;
  path: string;
  count: number;
}

export function* flattenInventory(inv: Inventory): Generator<FlatInventoryEntry> {
  for (const category of TRADEABLE_CATEGORIES) {
    const entries = inv[category] as InventoryStackEntry[] | undefined;
    if (!Array.isArray(entries)) continue;
    for (const e of entries) {
      const path = e.ItemType ?? e.Type;
      const count = e.ItemCount ?? 1;
      if (path) yield { category, path, count };
    }
  }
}

// Scans `inv.Upgrades` (per-instance leveled mods — the ones the user
// fused endo + credits into) and returns `path → max rank seen`. Used by
// the table to flag rows where a leveled copy exists, so the user can
// hide their working set and only see safe-to-sell duplicates.
//
// inv.Upgrades entries look like:
//   { ItemType: '/Lotus/Upgrades/Mods/...', UpgradeFingerprint: '{"lvl":5}', ... }
// The fingerprint is a JSON STRING (not an object). lvl=0 means "ranked
// up zero times"; some mods start at 0 in Upgrades because of cracked
// rivens or fused-then-unranked items. We surface the max lvl seen for
// each path so callers can decide their own threshold.
export function extractKeptLvls(inv: Inventory | null | undefined): Map<string, number> {
  const out = new Map<string, number>();
  const ups = inv?.Upgrades as InventoryUpgrade[] | undefined;
  if (!Array.isArray(ups)) return out;
  for (const e of ups) {
    const path = e?.ItemType;
    if (!path) continue;
    let lvl = 0;
    const fp = e.UpgradeFingerprint;
    if (typeof fp === 'string') {
      try {
        const parsed = JSON.parse(fp);
        if (typeof parsed?.lvl === 'number') lvl = parsed.lvl;
      } catch { /* silently ignore — malformed fingerprint, treat as lvl 0 */ }
    }
    const prev = out.get(path) ?? -1;
    if (lvl > prev) out.set(path, lvl);
  }
  return out;
}
