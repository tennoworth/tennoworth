import { callDomain } from './domain-commands';
import type { Inventory, Market, OwnedRecord } from '../contracts/data';
import type { Catalogs } from '../domain/resolver';
import type { ScoringMarketEntry, OwnedItem } from '../contracts/generated/domain';

export async function normalizeInventoryNative(data: Inventory, catalogs: Catalogs, market: Market) {
  const result = await callDomain('normalize_inventory', {
    inventory: data,
    catalog: [...catalogs.uniqueToInfo].map(([path, info]) => [path, { name: info.name, category: info.category ?? null }]),
    market: {
      catalog: market.catalog ?? {},
      path_to_info: Object.fromEntries(Object.entries(market.path_to_info ?? {}).map(([path, info]) => [path, { ...info, category: info.category ?? null }])),
    },
  });
  return { owned: new Map(result.owned), unresolved: result.unresolved, flatCount: result.flat_count };
}

export async function scoreInventoryNative(owned: Map<string, OwnedRecord>, market: Market, reserveCopies: number, sparesOnly: boolean, availability?: ReadonlyMap<string, number>) {
  const items: Record<string, ScoringMarketEntry> = {};
  for (const rec of owned.values()) {
    const m = market.items[rec.slug];
    if (!m) continue;
    items[rec.slug] = {
      vol: m.vol ?? null, low_sell: m.low_sell ?? null, avg: m.avg ?? null,
      median_now: m.median_now ?? null, median_90d: m.median_90d ?? null,
      low5_avg: m.low5_avg ?? null, ducats: m.ducats ?? null,
      donch_top_90d: m.donch_top_90d ?? null, donch_bot_90d: m.donch_bot_90d ?? null,
      top_buy: m.top_buy ?? null, medians_7d: Array.isArray(m.medians_7d) ? m.medians_7d : [],
    };
  }
  const rows = await callDomain('score_inventory', {
    owned: [...owned].map(([key, rec]): [string, OwnedItem] => [key, { ...rec, subtype: rec.subtype ?? null, kept_lvl: rec.kept_lvl ?? null, leveled: rec.leveled ?? 0 }]),
    market: { items, usage: market.usage ?? {}, set_to_parts: Object.fromEntries(Object.entries(market.set_to_parts ?? {}).map(([slug, entry]) => [slug, { parts: Array.isArray(entry?.parts) ? entry.parts : [] }])) },
    reserve_copies: reserveCopies,
    spares_only: sparesOnly,
    availability: availability ? Object.fromEntries(availability) : undefined,
  });
  return new Map(rows.map(row => [row.key, row]));
}
