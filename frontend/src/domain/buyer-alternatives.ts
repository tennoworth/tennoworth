import type { BuyerOrder, LiveTop } from '../contracts/desktop';
import { MAX_PER_TRADE } from './limits';

/** Why buyers cannot be compared; the view words each one. */
export type BuyerCompareError = 'invalid' | 'unavailable' | 'locked' | 'stale';

export function compareBuyers(quote: LiveTop | null, quantity: number, ask: number, now: number) {
  const book = quote?.buyer_book;
  const observed = Date.parse(book?.observed_at ?? '');
  if (!Number.isSafeInteger(quantity) || quantity <= 0 || !Number.isFinite(ask) || ask <= 0) {
    return { error: 'invalid' as BuyerCompareError } as const;
  }
  if (!quote || quote.error || !book || !Array.isArray(book.orders) || !Number.isFinite(observed)) {
    return { error: 'unavailable' as BuyerCompareError } as const;
  }
  if (!book.own_orders_excluded) return { error: 'locked' as BuyerCompareError } as const;
  if (!Number.isFinite(now) || now < observed || now - observed >= 60_000) {
    return { error: 'stale' as BuyerCompareError } as const;
  }
  const seen = new Set<string>();
  const eligible = book.orders.filter(order => {
    if (!order.id || !order.user_id || seen.has(order.user_id) || !['online', 'ingame'].includes(order.status)
      || !Number.isSafeInteger(order.quantity) || order.quantity <= 0
      || !Number.isSafeInteger(order.per_trade) || order.per_trade < 1 || order.per_trade > MAX_PER_TRADE
      || order.quantity % order.per_trade !== 0 || !Number.isSafeInteger(order.platinum) || order.platinum <= 0) return false;
    seen.add(order.user_id);
    return true;
  }).sort((a, b) => b.platinum / b.per_trade - a.platinum / a.per_trade || a.id.localeCompare(b.id));
  let remaining = quantity;
  const rows: Array<{ order: BuyerOrder; quantity: number; value: number }> = [];
  for (const order of eligible) {
    const units = Math.floor(Math.min(remaining, order.quantity) / order.per_trade) * order.per_trade;
    if (!units) continue;
    rows.push({ order, quantity: units, value: units / order.per_trade * order.platinum });
    remaining -= units;
  }
  const covered = quantity - remaining;
  const value = rows.reduce((sum, row) => sum + row.value, 0);
  return { error: null, rows, covered, uncovered: remaining, value, askValue: covered * ask,
    gap: covered * ask - value, observed: book.observed_at };
}
