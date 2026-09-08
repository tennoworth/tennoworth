import type { BuyerOrder, LiveTop } from '../contracts/desktop';

export function compareBuyers(quote: LiveTop | null, quantity: number, ask: number, now: number) {
  const book = quote?.buyer_book;
  const observed = Date.parse(book?.observed_at ?? '');
  if (!Number.isSafeInteger(quantity) || quantity <= 0 || !Number.isFinite(ask) || ask <= 0) {
    return { error: 'Choose a positive whole quantity and a valid listing reference.' } as const;
  }
  if (!quote || quote.error || !book || !Array.isArray(book.orders) || !Number.isFinite(observed)) {
    return { error: 'Buyer coverage is unavailable. Check buyers to try again.' } as const;
  }
  if (!book.own_orders_excluded) return { error: 'Unlock your WFM account, then check again to exclude your own orders.' } as const;
  if (!Number.isFinite(now) || now < observed || now - observed >= 60_000) {
    return { error: 'This buyer check is stale. Refresh before comparing quantities.' } as const;
  }
  const seen = new Set<string>();
  const eligible = book.orders.filter(order => {
    if (!order.id || !order.user_id || seen.has(order.user_id) || !['online', 'ingame'].includes(order.status)
      || !Number.isSafeInteger(order.quantity) || order.quantity <= 0
      || !Number.isSafeInteger(order.per_trade) || order.per_trade < 1 || order.per_trade > 6
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
