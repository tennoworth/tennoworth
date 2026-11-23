// Mispriced books in the snapshot (market_domain::browse::find_bargains, which
// documents the two shapes and their floors). A row is a lead to check live.
import type { Market } from '../contracts/data';
import type { LiveTop } from '../contracts/desktop';
import type { BargainKind } from '../contracts/generated/web';
import { buildBrowseIndex } from './market-browse';
import { call, native } from './native';

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

const num = (v: unknown) => Number(v) || 0;

export function findBargains(market: Pick<Market, 'items' | 'catalog'> | null | undefined, limit = 12): Bargain[] {
  const items = market?.items;
  if (!items) return [];
  const entries = Object.entries(items);
  const leads = call({ op: 'find_bargains', input: {
    items: entries.map(([, e]) => ({
      ask: num(e?.low_sell), bid: num(e?.top_buy), median: num(e?.median_90d), vol: num(e?.vol), low5: num(e?.low5_avg),
    })),
    limit,
  } });
  if (leads.length === 0) return [];
  const index = buildBrowseIndex(market);
  return leads.map(({ at, kind, ask, resale, margin, vol }) => {
    const slug = entries[at]![0];
    return { slug, name: index.nameOf(slug), kind, ask, resale, margin, vol };
  });
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
  const holds = native().web_bargain_holds(b.kind === 'flip', b.ask, ask, quote.top_buy ?? Number.NaN);
  return holds ? { state: 'holds', ask, seller: quote.low_sell_by ?? null } : { state: 'gone', ask };
}
