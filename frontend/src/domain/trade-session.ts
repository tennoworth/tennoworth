import type { MarketItemEntry } from '../contracts/data';
import { MAX_PER_TRADE } from './limits';

export type SessionMode = 'fast' | 'per-trade' | 'clear' | 'max';
export const SESSION_MODES: Array<{ id: SessionMode; name: string; description: string }> = [
  { id: 'fast', name: 'Fast Cash', description: 'Liquid singles at credible asking prices.' },
  { id: 'per-trade', name: 'Plat per Trade', description: 'Valuable exchanges, spread across items.' },
  { id: 'clear', name: 'Clear Inventory', description: 'Clear safe spare stacks that have demand.' },
  { id: 'max', name: 'Max Value', description: 'Larger valuable positions; patience may be needed.' },
];

export interface SessionCandidate {
  key: string;
  slug: string;
  name: string;
  owned: number;
  sellable: number;
  leveled: number;
  subtype?: string | null;
  type: string;
  hold: boolean;
  bulk: boolean;
  supported?: boolean;
  market: MarketItemEntry;
  components?: Record<string, number>;
}

export interface SessionRow extends SessionCandidate {
  component_limits: Record<string, number>;
  quantity: number;
  per_trade: number;
  platinum: number;
  trades: number;
  reason: string;
  bid: number | null;
}

export function validSessionLot(quantity: number, lot: number, bulk: boolean): boolean {
  return Number.isSafeInteger(quantity) && quantity > 0 && Number.isSafeInteger(lot)
    && lot >= 1 && lot <= MAX_PER_TRADE && quantity % lot === 0 && (bulk || lot === 1);
}

/** What the native Trade Session selection returns (market_domain::trade_session), with each row's market restored from its candidate. */
export interface SessionPlan {
  rows: SessionRow[];
  trades: number;
  total: number;
  excluded: Array<{ name: string; reason: string }>;
  target: number | null;
  shortfall: number | null;
}

/** The fewest in-game trades that sell `quantity` copies under the lot rule
 *  `validSessionLot` enforces: a bulk-tradable item moves up to MAX_PER_TRADE
 *  copies per trade, anything else one per trade. Not a whole number of copies → 0. */
export function tradesToSell(quantity: number, bulk: boolean): number {
  if (!Number.isSafeInteger(quantity) || quantity <= 0) return 0;
  return bulk ? Math.ceil(quantity / MAX_PER_TRADE) : quantity;
}

export interface TradeSplit<T> { today: T[]; later: T[]; used: number; laterTrades: number }

/** Splits a best-first list at today's trades left. Rows stay in rank order: the
 *  first row that no longer fits starts the after-the-reset part, even when a
 *  smaller row further down would still fit, so the list never reorders. An
 *  unknown allowance (`null`) is not treated as zero or as unlimited: nothing splits. */
export function splitByTrades<T>(rows: T[], trades: (row: T) => number, remaining: number | null): TradeSplit<T> {
  const total = rows.reduce((sum, row) => sum + trades(row), 0);
  if (remaining == null || !Number.isFinite(remaining)) return { today: rows, later: [], used: total, laterTrades: 0 };
  let used = 0, cut = rows.length;
  for (let i = 0; i < rows.length; i++) {
    const need = trades(rows[i]);
    if (used + need > remaining) { cut = i; break; }
    used += need;
  }
  return { today: rows.slice(0, cut), later: rows.slice(cut), used, laterTrades: total - used };
}
