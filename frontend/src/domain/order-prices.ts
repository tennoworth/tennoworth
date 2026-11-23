import type { MarketItemEntry } from '../contracts/data';
import { native } from './native';

// A wasm f64 parameter would coerce a string or null to a number; the rule
// reads anything but a number as invalid, so it travels as NaN.
const wasmNumber = (value: unknown) => typeof value === 'number' ? value : Number.NaN;
const orNull = (value: number) => Number.isNaN(value) ? null : value;

/** The per-unit price of a listing (market_domain::listing::unit_price). */
export function orderUnitPrice(platinum: number, perTrade?: number | null): number | null {
  return orNull(native().web_order_unit_price(wasmNumber(platinum), wasmNumber(perTrade ?? 1)));
}

/** The whole-platinum lot total for a per-unit price. */
export function orderLotPrice(unit: number, perTrade?: number | null): number | null {
  return orNull(native().web_order_lot_price(wasmNumber(unit), wasmNumber(perTrade ?? 1)));
}

/** The snapshot figures for one rank of an item, or `null` when the snapshot
 *  has none. An entry describes rank 0, plus its max rank under `ranked`;
 *  a maxed arcane sells for several times an unranked one, so neither tier
 *  stands in for the other. */
export function marketAtRank(m: MarketItemEntry, rank?: number | null): MarketItemEntry | null {
  const wanted = rank ?? 0;
  if (wanted === 0) return m;
  const tier = m.ranked;
  if (!tier || tier.rank !== wanted) return null;
  return {
    price_basis: m.price_basis, tags: m.tags, ducats: m.ducats,
    avg: tier.avg, low_sell: tier.low_sell, low5_avg: tier.low5_avg, low20_avg: tier.low20_avg, top_buy: tier.top_buy,
    vol: tier.vol, ratio: 0, buys: tier.buys, sells: tier.sells, median_now: tier.median_now, median_90d: tier.median_90d,
  };
}

/** Older snapshots mixed bulk totals into asks/bids. Their historical prices
 * remain usable, but a bulk item needs a normalized snapshot or a live check
 * before those book prices can be called per-unit evidence. */
export function cachedUnitMarket(m: MarketItemEntry, bulk = false): MarketItemEntry {
  if (m.price_basis === 'unit' || (!bulk && !m.tags?.some(t => t === 'arcane_enhancement' || t === 'relic'))) return m;
  return { ...m, low_sell: 0, top_buy: 0, low5_avg: 0, low20_avg: 0 };
}
