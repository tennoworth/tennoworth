// Ledger arithmetic over EE.log-confirmed trades. Pure; the panel renders it.

import type { TradeRow } from './transport';

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
