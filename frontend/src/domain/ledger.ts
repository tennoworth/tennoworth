// Ledger arithmetic over EE.log-confirmed trades. Pure; the panel renders it.

import type { TradeRow } from '../contracts/desktop';

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

/** The first `count` sellers, and what the rest add up to as one "Other" row,
 *  so a long tail neither crowds the ranking nor drops out of the total. */
export function topSellers(rows: SoldItemTotal[], count: number): { top: SoldItemTotal[]; other: { items: number; plat: number } | null } {
  const rest = rows.slice(count);
  return { top: rows.slice(0, count), other: rest.length ? { items: rest.length, plat: rest.reduce((n, r) => n + r.plat, 0) } : null };
}

export interface WeekTotal {
  /** Local midnight on the Monday that starts the week, in ms. */
  start: number;
  plat: number;
  sales: number;
  /** The current week, still running: a short bar is not a drop. */
  partial: boolean;
}

function weekStart(ms: number): number {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  d.setDate(d.getDate() - ((d.getDay() + 6) % 7));
  return d.getTime();
}

/** Plat realised from sales in each of the last `weeks` local weeks, oldest
 *  first, ending with the current week. Weeks before the first recorded trade
 *  are left out: the ledger was not running then, so they are unknown, not
 *  weeks with no sales. Weeks step by calendar date, so a daylight-saving
 *  change does not shift a sale into the wrong week. */
export function weeklySales(trades: TradeRow[], weeks: number, nowMs: number): WeekTotal[] {
  const current = weekStart(nowMs);
  const first = trades.length ? weekStart(Math.min(...trades.map((t) => t.at)) * 1000) : current;
  const out: WeekTotal[] = [];
  for (let back = weeks - 1; back >= 0; back--) {
    const d = new Date(current);
    d.setDate(d.getDate() - 7 * back);
    if (d.getTime() < first) continue;
    out.push({ start: d.getTime(), plat: 0, sales: 0, partial: back === 0 });
  }
  const byStart = new Map(out.map((w) => [w.start, w]));
  for (const t of trades) {
    if (t.kind !== 'sale') continue;
    const week = byStart.get(weekStart(t.at * 1000));
    if (week) { week.plat += t.plat; week.sales += 1; }
  }
  return out;
}

/** Median plat of the finished weeks; null when there are none. */
export function medianWeek(weeks: WeekTotal[]): number | null {
  const done = weeks.filter((w) => !w.partial).map((w) => w.plat).sort((a, b) => a - b);
  if (!done.length) return null;
  const mid = Math.floor(done.length / 2);
  return done.length % 2 ? done[mid]! : (done[mid - 1]! + done[mid]!) / 2;
}

/** "Primed Flow, Lith C5 Relic ×3" for the side of the trade that matters. */
export function describeItems(t: Pick<TradeRow, 'kind' | 'items'>): string {
  const side = t.kind === 'purchase' ? 'received' : 'given';
  const picked = t.items.filter((i) => i.direction === side);
  const list = (picked.length ? picked : t.items).map((i) => (i.qty > 1 ? `${i.name} ×${i.qty}` : i.name));
  return list.join(', ') || '-';
}
