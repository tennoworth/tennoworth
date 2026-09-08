import { flattenInventory, extractKeptLvls } from './inventory';
import { resolvePath, type Catalogs } from './resolver';
import type { Inventory, Market, OwnedRecord } from '../contracts/data';

export function normalizeInventoryPreview(data: Inventory, catalogs: Catalogs, market: Market) {
  const keptLvls = extractKeptLvls(data);
  const owned = new Map<string, OwnedRecord>();
  const unresolved: Record<string, number> = {};
  let flatCount = 0;
  for (const { category: invCat, path, count, xp } of flattenInventory(data)) {
    flatCount++;
    const { name: itemName, slug, category: itemType, subtype } =
      resolvePath(path, catalogs, market);
    if (!slug) {
      unresolved[invCat] = (unresolved[invCat] || 0) + 1;
      continue;
    }
    const type = itemType || invCat;
    const key = `${slug}|${subtype ?? ''}`;
    const keptLvl = keptLvls.get(path);
    const rec = owned.get(key) || {
      count: 0, name: itemName ?? slug, type, slug, subtype: subtype ?? null,
      kept_lvl: null, leveled: 0,
    };
    rec.count += count;
    // XP > 0 on an instance category entry means DE has flagged that
    // specific copy untradeable in-game (only unranked gear can trade).
    // Stack categories never carry XP, so this stays 0 for them.
    if (xp > 0) rec.leveled += count;
    // Carry forward the highest kept lvl across any inventory path
    // that resolved to the same slug+subtype (rare for mods - one path
    // per slug - but harmless for the relic refinement case).
    if (typeof keptLvl === 'number' && (rec.kept_lvl === null || keptLvl > rec.kept_lvl)) {
      rec.kept_lvl = keptLvl;
    }
    owned.set(key, rec);
  }
  return { owned, unresolved, flatCount };
}
