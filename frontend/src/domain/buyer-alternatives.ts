import type { BuyerOrder, LiveTop } from '../contracts/desktop';
import type { BuyerCompareError } from '../contracts/generated/web';
import { call } from './native';

export type { BuyerCompareError };

export type BuyerComparison =
  | { readonly error: BuyerCompareError }
  | { error: null; rows: Array<{ order: BuyerOrder; quantity: number; value: number }>; covered: number;
    uncovered: number; value: number; askValue: number; gap: number; observed: string | null };

/** Which live buyers would take `quantity` at what value, against the ask
 *  (market_domain::buyers::compare_buyers). The page reads the clocks: `now`,
 *  and the quote's observation time as Date.parse reads it. */
export function compareBuyers(quote: LiveTop | null, quantity: number, ask: number, now: number): BuyerComparison {
  const book = quote?.buyer_book;
  const observed = Date.parse(book?.observed_at ?? '');
  const result = call({ op: 'compare_buyers', input: { quote, quantity, ask, now, observed } });
  if ('error' in result) return { error: result.error };
  const orders = book?.orders ?? [];
  return {
    error: null, rows: result.rows.map(row => ({ order: orders[row.order]!, quantity: row.quantity, value: row.value })),
    covered: result.covered, uncovered: result.uncovered, value: result.value, askValue: result.ask_value, gap: result.gap,
    observed: book?.observed_at ?? null,
  };
}
