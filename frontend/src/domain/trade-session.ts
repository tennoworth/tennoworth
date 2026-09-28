import type { MarketItemEntry } from '../contracts/data';

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
    && lot >= 1 && lot <= 6 && quantity % lot === 0 && (bulk || lot === 1);
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
