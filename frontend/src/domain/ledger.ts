// Ledger arithmetic over EE.log-confirmed trades. Pure; the panel renders it.

import type { ListingOutcomes, OutcomeGroup, TradeRow } from '../contracts/desktop';
import type { WithinDeadline } from '../contracts/generated/desktop';

export interface LedgerTotals {
  sales: number;
  purchases: number;
  /** Plat received on sales. */
  platIn: number;
  /** Plat spent on purchases. */
  platOut: number;
  net: number;
}

export function totals(trades: TradeRow[]): LedgerTotals {
  const t: LedgerTotals = { sales: 0, purchases: 0, platIn: 0, platOut: 0, net: 0 };
  for (const tr of trades) {
    if (tr.kind === 'sale') { t.sales += 1; t.platIn += tr.plat; }
    else if (tr.kind === 'purchase') { t.purchases += 1; t.platOut += tr.plat; }
  }
  t.net = t.platIn - t.platOut;
  return t;
}

/** Trades at or after `nowSecs - days*86400`. */
export function since(trades: TradeRow[], days: number, nowSecs: number): TradeRow[] {
  const cutoff = nowSecs - days * 86400;
  return trades.filter((t) => t.at >= cutoff);
}

export interface SoldItemTotal {
  name: string;
  qty: number;
  /** Plat attributed to this item: whole-trade plat when it was the only item
   *  given, else split evenly by quantity across the given items - an honest
   *  approximation, labelled as such in the UI. */
  plat: number;
  trades: number;
}

/** Realised plat per item across SALES, largest first. */
export function soldByItem(trades: TradeRow[]): SoldItemTotal[] {
  const acc = new Map<string, SoldItemTotal>();
  for (const tr of trades) {
    if (tr.kind !== 'sale') continue;
    const given = tr.items.filter((i) => i.direction === 'given');
    const units = given.reduce((n, i) => n + Math.max(1, i.qty), 0);
    for (const i of given) {
      const share = units > 0 ? (tr.plat * Math.max(1, i.qty)) / units : 0;
      const row = acc.get(i.name) ?? { name: i.name, qty: 0, plat: 0, trades: 0 };
      row.qty += i.qty;
      row.plat += share;
      row.trades += 1;
      acc.set(i.name, row);
    }
  }
  return [...acc.values()]
    .map((r) => ({ ...r, plat: Math.round(r.plat) }))
    .sort((a, b) => b.plat - a.plat || a.name.localeCompare(b.name));
}

/** "Primed Flow, Lith C5 Relic ×3" for the side of the trade that matters. */
export function describeItems(t: Pick<TradeRow, 'kind' | 'items'>): string {
  const side = t.kind === 'purchase' ? 'received' : 'given';
  const picked = t.items.filter((i) => i.direction === side);
  const list = (picked.length ? picked : t.items).map((i) => (i.qty > 1 ? `${i.name} ×${i.qty}` : i.name));
  return list.join(', ') || '-';
}

export interface OutcomeRow {
  label: string;
  listings: string;
  sold: string;
  day: string;
  week: string;
  wait: string;
  price: string;
  priceTitle: string;
}

/** A deadline share is only shown once some listing is old enough to judge. */
function within(d: WithinDeadline): string {
  return d.eligible ? `${d.sold} of ${d.eligible}` : '-';
}

function wait(hours: number | null): string {
  if (hours === null) return '-';
  if (hours < 1) return 'under 1 h';
  if (hours < 48) return `${Math.round(hours)} h`;
  return `${(hours / 24).toFixed(1)} d`;
}

/** Rows for the groups that have listings; an empty group is not a 0% row. */
export function outcomeRows(o: ListingOutcomes): OutcomeRow[] {
  const groups: [string, OutcomeGroup][] = [['Trade Session', o.session], ['Other listings', o.manual]];
  return groups
    .filter(([, g]) => g.listings > 0)
    .map(([label, g]) => ({
      label,
      listings: String(g.listings),
      sold: `${g.quantity_sold} of ${g.quantity_listed}`,
      day: within(g.within_day),
      week: within(g.within_week),
      wait: wait(g.median_hours_to_first_sale),
      price: g.price_ratio === null ? '-' : `${Math.round(g.price_ratio * 100)}%`,
      priceTitle: g.price_ratio === null
        ? 'No sold copy had a single unit price'
        : `Realised over listed price on ${g.priced_quantity} cop${g.priced_quantity === 1 ? 'y' : 'ies'} sold for plat alone`,
    }));
}

export function outcomeHeadline(o: ListingOutcomes): string {
  if (!o.enabled) return 'off';
  const listings = o.session.listings + o.manual.listings;
  if (!listings) return 'none in the last ' + o.window_days + ' days';
  const sold = [o.session, o.manual].reduce((n, g) => n + g.within_week.sold, 0);
  const judged = [o.session, o.manual].reduce((n, g) => n + g.within_week.eligible, 0);
  const noun = `listing${listings === 1 ? '' : 's'}`;
  return judged
    ? `${listings} ${noun} in ${o.window_days} days · ${sold} of ${judged} sold within a week`
    : `${listings} ${noun} in ${o.window_days} days · too recent to judge`;
}
