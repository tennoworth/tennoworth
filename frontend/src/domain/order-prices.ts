import type { MarketItemEntry } from '../contracts/data';

export function orderUnitPrice(platinum: number, perTrade?: number | null): number | null {
  const lot = perTrade ?? 1;
  return Number.isFinite(platinum) && platinum > 0 && Number.isSafeInteger(lot) && lot >= 1 && lot <= 6
    ? platinum / lot : null;
}

export function orderLotPrice(unit: number, perTrade?: number | null): number | null {
  const lot = perTrade ?? 1;
  return Number.isFinite(unit) && unit > 0 && Number.isSafeInteger(lot) && lot >= 1 && lot <= 6
    ? Math.ceil(unit * lot) : null;
}

/** Older snapshots mixed bulk totals into asks/bids. Their historical prices
 * remain usable, but a bulk item needs a normalized snapshot or a live check
 * before those book prices can be called per-unit evidence. */
export function cachedUnitMarket(m: MarketItemEntry, bulk = false): MarketItemEntry {
  if (m.price_basis === 'unit' || (!bulk && !m.tags?.some(t => t === 'arcane_enhancement' || t === 'relic'))) return m;
  return { ...m, low_sell: 0, top_buy: 0, low5_avg: 0 };
}
