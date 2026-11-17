import type { Market } from '../contracts/data';
import type { LiveTop } from '../contracts/desktop';
import { buildBrowseIndex } from './market-browse';

// Two shapes of mispriced book in the snapshot, sized on 2026-11-17 (16 rows):
// - flip: a live bid at least BARGAIN_MIN_MARGIN over the cheapest ask, so the
//   item can be bought and sold straight to that buyer;
// - cheap: one ask at most CHEAP_AT of the 90-day median while the five
//   cheapest still average CHEAP_BOOK of it, so the rest of the book agrees
//   on the usual price and one seller is far under it.
// Thin items carry the fantasy bids and wrong-rank asks, so both need trades
// behind them, and a flip's bid must be believable against the median.
// A snapshot is hours old and these close fast: a row is a lead to check live,
// never a promise.
export const BARGAIN_MIN_VOL = 10;
export const BARGAIN_MIN_MARGIN = 3;
export const FLIP_MAX_BID = 1.5;
export const CHEAP_AT = 0.6;
export const CHEAP_BOOK = 0.85;
export const CHEAP_MIN_MEDIAN = 10;

export type BargainKind = 'flip' | 'cheap';

export interface Bargain {
  slug: string;
  name: string;
  kind: BargainKind;
  ask: number;
  /** flip: the bid that would buy it back; cheap: the 90-day median. */
  resale: number;
  margin: number;
  vol: number;
}

export function findBargains(market: Pick<Market, 'items' | 'catalog'> | null | undefined, limit = 12): Bargain[] {
  const items = market?.items;
  if (!items) return [];
  const index = buildBrowseIndex(market);
  const out: Bargain[] = [];
  for (const [slug, e] of Object.entries(items)) {
    const ask = Number(e?.low_sell) || 0;
    const bid = Number(e?.top_buy) || 0;
    const median = Number(e?.median_90d) || 0;
    const vol = Number(e?.vol) || 0;
    const low5 = Number(e?.low5_avg) || 0;
    if (ask <= 0 || median <= 0 || vol < BARGAIN_MIN_VOL) continue;
    const row = { slug, name: index.nameOf(slug), ask, vol };
    if (bid - ask >= BARGAIN_MIN_MARGIN && bid <= median * FLIP_MAX_BID) {
      out.push({ ...row, kind: 'flip', resale: bid, margin: bid - ask });
    } else if (median >= CHEAP_MIN_MEDIAN && ask <= median * CHEAP_AT && low5 >= median * CHEAP_BOOK && median - ask >= BARGAIN_MIN_MARGIN) {
      out.push({ ...row, kind: 'cheap', resale: median, margin: median - ask });
    }
  }
  return out.sort((a, b) => b.margin - a.margin || b.vol - a.vol).slice(0, limit);
}

export type LiveBargain =
  | { state: 'holds'; ask: number; seller: string | null }
  | { state: 'gone'; ask: number | null }
  | { state: 'failed' };

/** Whether the live book still offers what the snapshot showed. */
export function liveBargain(b: Bargain, quote: LiveTop | undefined): LiveBargain | null {
  if (!quote) return null;
  if (quote.error) return { state: 'failed' };
  const ask = quote.low_sell;
  if (ask == null) return { state: 'gone', ask: null };
  const holds = b.kind === 'flip'
    ? quote.top_buy != null && quote.top_buy - ask >= BARGAIN_MIN_MARGIN
    : ask <= b.ask;
  return holds ? { state: 'holds', ask, seller: quote.low_sell_by ?? null } : { state: 'gone', ask };
}
