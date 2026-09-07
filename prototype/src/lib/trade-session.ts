import { clearingPrice, hasRealPrice, LIQUID_VOL } from './sell-priority';
import { MAX_PLAN_ITEMS, MIN_PLATINUM, MAX_PLATINUM } from './limits';
import type { MarketItemEntry } from './types';

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
}

export interface SessionRow extends SessionCandidate {
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

export function selectSession(candidates: SessionCandidate[], mode: SessionMode, budget: number, target?: number) {
  const cap = Number.isSafeInteger(budget) ? Math.min(MAX_PLAN_ITEMS, Math.max(0, budget)) : 0;
  const goal = target != null && Number.isFinite(target) && target > 0 ? target : null;
  const excluded: Array<{ name: string; reason: string }> = [];
  const counts = new Map<string, number>();
  for (const row of candidates) counts.set(row.slug, (counts.get(row.slug) ?? 0) + 1);
  const eligible: Array<SessionRow & { volume: number; weight: number }> = [];
  for (const row of candidates) {
    const price = Math.ceil(clearingPrice(row.market));
    let reason: string | null = null;
    if (row.supported === false || row.subtype || row.slug.endsWith('_set') || /riven/i.test(row.type) || counts.get(row.slug) !== 1) {
      reason = 'This item identity is not supported in Trade Session yet.';
    } else if (!Number.isSafeInteger(row.sellable) || row.sellable <= 0 || !Number.isSafeInteger(row.owned) || row.sellable > row.owned) {
      reason = 'No confirmed sellable copies after protection and trade checks.';
    } else if (!hasRealPrice(row.market) || !Number.isFinite(price) || price < MIN_PLATINUM || price > MAX_PLATINUM) {
      reason = 'No credible price within the listing limits.';
    }
    const volume = Number.isFinite(row.market.vol) ? Math.max(0, row.market.vol) : 0;
    if (!reason && (mode === 'fast' || mode === 'clear') && volume < LIQUID_VOL) {
      reason = 'Not enough reported trading volume for this mode.';
    }
    if (reason) { excluded.push({ name: row.name, reason }); continue; }
    let lot = mode === 'fast' || !row.bulk ? 1 : Math.min(6, row.sellable, Math.floor(MAX_PLATINUM / price));
    if (mode === 'clear') {
      while (row.sellable % lot !== 0) lot--;
    }
    const bid = Number.isFinite(row.market.top_buy) && row.market.top_buy > 0 ? row.market.top_buy : null;
    eligible.push({ ...row, platinum: price, per_trade: lot, quantity: 0, trades: 0,
      reason: '', bid, volume, weight: row.hold ? 0.6 : 1 });
  }
  const score = (r: typeof eligible[number]): number[] => {
    const value = r.platinum * r.per_trade;
    if (mode === 'fast') {
      const spreadEvidence = r.bid != null && r.bid <= r.platinum ? r.bid / r.platinum : 0;
      return [r.volume * (1 + spreadEvidence) * r.weight, r.platinum];
    }
    if (mode === 'clear') return [Number(r.sellable / r.per_trade <= cap) * r.weight, r.per_trade * r.weight, r.volume];
    if (mode === 'max') return [value * Math.min(cap, Math.floor(r.sellable / r.per_trade)) * r.weight, r.volume];
    return [value * r.weight, r.volume];
  };
  eligible.sort((a, b) => {
    const aa = score(a), bb = score(b);
    for (let i = 0; i < aa.length; i++) if (aa[i] !== bb[i]) return bb[i] - aa[i];
    return a.key < b.key ? -1 : a.key > b.key ? 1 : 0;
  });
  let trades = 0, total = 0;
  const add = (row: typeof eligible[number]) => {
    if (trades >= cap || (goal != null && total >= goal) || row.quantity + row.per_trade > row.sellable) return false;
    row.quantity += row.per_trade;
    row.trades++;
    trades++;
    total += row.platinum * row.per_trade;
    return true;
  };
  if (mode === 'per-trade' || mode === 'fast') {
    // Diversify the first pass; a single large stack must not consume every
    // suggested exchange in the per-trade mode.
    let changed = true;
    while (changed) {
      changed = false;
      for (const row of eligible) if (add(row)) changed = true;
    }
  } else {
    for (const row of eligible) while (add(row)) { /* bounded by the trade cap */ }
  }
  const rows: SessionRow[] = eligible.filter(r => r.quantity > 0).map(r => ({ ...r,
    reason: [
      mode === 'fast' ? 'Liquid singles; match credible asks.' : mode === 'per-trade' ? `${r.platinum * r.per_trade}p per suggested exchange.`
        : mode === 'clear' && r.quantity === r.sellable ? 'Clears this safe spare stack.' : 'Prioritizes listing value within the budget.',
      r.volume < LIQUID_VOL ? 'Thin market: patience may be needed.' : '',
      r.hold ? 'Hold advice lowers priority; protected copies remain excluded.' : '',
    ].filter(Boolean).join(' '),
  }));
  return { rows, trades, total, excluded, target: goal, shortfall: goal == null ? null : Math.max(0, goal - total) };
}
