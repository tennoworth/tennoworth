// Ledger arithmetic over EE.log-confirmed trades. The arithmetic is native
// (market_domain::ledger); local calendar weeks, a locale's name order and
// the item text stay here, with the page that knows the time zone and locale.

import type { TradeRow } from '../contracts/desktop';
import type { LedgerTotals, SoldItemTotal } from '../contracts/generated/web';
import { call } from './native';

// The trade store's rows go as they are: they carry every field the rules
// read, as plain numbers.

export function totals(trades: TradeRow[]): LedgerTotals {
  return call({ op: 'ledger_totals', input: trades });
}

/** Trades at or after `nowSecs - days*86400`. */
export function since(trades: TradeRow[], days: number, nowSecs: number): TradeRow[] {
  if (trades.length === 0) return [];
  return call({ op: 'ledger_since', input: { at: trades.map((t) => t.at), days, now_secs: nowSecs } }).map((i) => trades[i]!);
}

/** Realised plat per item across SALES, largest first. */
export function soldByItem(trades: TradeRow[]): SoldItemTotal[] {
  return call({ op: 'sold_by_item', input: trades })
    .sort((a, b) => b.plat - a.plat || a.name.localeCompare(b.name));
}

/** The first `count` sellers, and what the rest add up to as one "Other" row,
 *  so a long tail neither crowds the ranking nor drops out of the total. */
export function topSellers(rows: SoldItemTotal[], count: number): { top: SoldItemTotal[]; other: { items: number; plat: number } | null } {
  const { top, other } = call({ op: 'top_sellers', input: { plats: rows.map((r) => r.plat), count } });
  return { top: rows.slice(0, top), other };
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
  return call({ op: 'median_week', input: weeks.map(({ plat, partial }) => ({ plat, partial })) });
}

/** "Primed Flow, Lith C5 Relic ×3" for the side of the trade that matters. */
export function describeItems(t: Pick<TradeRow, 'kind' | 'items'>): string {
  const side = t.kind === 'purchase' ? 'received' : 'given';
  const picked = t.items.filter((i) => i.direction === side);
  const list = (picked.length ? picked : t.items).map((i) => (i.qty > 1 ? `${i.name} ×${i.qty}` : i.name));
  return list.join(', ') || '-';
}
