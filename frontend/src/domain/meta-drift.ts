// Meta Drift's model is native (market_domain::meta_drift, documented there);
// this file joins its rows to the market's ask and volume and words the delta.
import type { Market } from '../contracts/data';
import type { DriftShare, OnlyShare } from '../contracts/generated/web';
import { call } from './native';

interface MarketFields {
  lowSell: number | null;
  volume48h: number | null;
}

export type DriftRow = DriftShare & MarketFields;
export type OnlyRow = OnlyShare & MarketFields;

export interface MetaDriftModel {
  priorYear: number;
  currentYear: number;
  /** Years between the two snapshots that DE did not publish. */
  missingYears: number[];
  gains: DriftRow[];
  losses: DriftRow[];
  onlyPrior: OnlyRow[];
  onlyCurrent: OnlyRow[];
  categoryChanges: number;
  categories: string[];
}

const known = (v: unknown): number | null => typeof v === 'number' && Number.isFinite(v) && v >= 0 ? v : null;

// A failed join stays unknown, never a plausible zero.
function withMarket<T extends { slug: string }>(market: Market, row: T): T & MarketFields {
  const item = market.items?.[row.slug];
  return { ...row, lowSell: known(item?.low_sell), volume48h: known(item?.vol) };
}

export function formatDeltaPp(value: number): string {
  const decimals = Math.abs(value) < 0.01 ? 4 : 2;
  return `${value > 0 ? '+' : ''}${value.toFixed(decimals)} pp`;
}

export function buildMetaDrift(market: Market | null | undefined): MetaDriftModel | null {
  if (!market) return null;
  const model = call({ op: 'meta_drift', input: { usage_history: market.usage_history ?? null } });
  if (!model) return null;
  const join = <T extends { slug: string }>(rows: T[]) => rows.map((row) => withMarket(market, row));
  return {
    ...model,
    gains: join(model.gains),
    losses: join(model.losses),
    onlyPrior: join(model.onlyPrior),
    onlyCurrent: join(model.onlyCurrent),
  };
}
