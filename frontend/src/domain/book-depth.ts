import type { MarketItemEntry } from '../contracts/data';
import { native } from './native';

type BookFields = Pick<MarketItemEntry, 'low5_avg' | 'low20_avg' | 'sells'>;

const num = (v: unknown) => Number(v) || 0;

/** A queue at one price: the 20 cheapest asks within 10% of the 5 cheapest
 *  (market_domain::math::book_depth). */
export function crowdedBook(m: BookFields | null | undefined): boolean {
  return native().web_crowded_book(num(m?.low5_avg), num(m?.low20_avg), num(m?.sells)) !== 0;
}
