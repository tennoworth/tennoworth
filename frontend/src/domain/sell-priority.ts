// The sell-priority rules the views apply synchronously. Each rule is native
// (market_domain::math::sell_priority, documented there) and runs from the
// view module (native.ts); this file is the call shape plus the thresholds the
// views print in tooltips. The score comes from score_inventory on the desktop
// and from the pipeline's handoff_sample on the homepage.

import type { MarketItemEntry } from '../contracts/data';
import { call, native } from './native';

type PricedEntry = Pick<
  MarketItemEntry,
  'vol' | 'low_sell' | 'avg' | 'median_now' | 'median_90d'
> | null | undefined;

const num = (v: unknown) => Number(v) || 0;

/** Below this many closed trades / 48 h the book is too thin to trust its
 *  ask; the thin-book tags print it. liquidity-boundary.json pins it. */
export const LIQUID_VOL = 5;

/** An ask this many times under the median is a troll or stale undercut, not
 *  a price. The live-ask read in price-rule.ts applies it too. */
export const TROLL_UNDERCUT_RATIO = 3;

/** The Top-picks floor, printed in the picks tooltip; pick-floor.json pins it. */
export const MIN_PICK_SCORE = 20;

/** Copies listable after the "Keep copies" reserve and leveled copies. */
export function sellableQty(count: number, reserve: number, leveled = 0): number {
  return native().web_sellable_qty(count, reserve, leveled);
}

/** Copies the Spares preset counts: the ones you would dissolve for endo. */
export function spareQty(count: number, keptLvl: number | null, leveled = 0): number {
  return native().web_spare_qty(count, keptLvl ?? Number.NaN, leveled);
}

/** What a listing would realistically clear at: the ask, sanity-clamped
 *  against the closed-trade median. */
export function clearingPrice(m: PricedEntry): number {
  return native().web_clearing_price(num(m?.vol), num(m?.low_sell), num(m?.avg), num(m?.median_now), num(m?.median_90d));
}

// Where today's median sits in its 90-day channel: native scoring::timing
// decides it (tests/fixtures/sell-priority/timing.json). "hold" is a dip near
// the 90-day low, "drop" the same on heavy selling, "peak" a corroborated high.
export type TimingState = 'hold' | 'drop' | 'peak' | 'neutral';

// Only the fields selectPicks reads. Generic so the caller's full row passes
// through untouched: the picks UI and the listing modal need most of it.
export interface PickCandidate {
  sellable: number;
  patience: boolean;
  sell_score: number;
  /** A "hold" row sells near its 90-day low; a pick is "sell this now". */
  timing?: TimingState;
}

/** The Top picks of rows already sorted best-first (computeResults sorts the
 *  whole table once). The evening digest applies the same rule. */
export function selectPicks<T extends PickCandidate>(rows: T[], { cap }: { cap?: number } = {}): T[] {
  if (rows.length === 0) return [];
  const picked = call({ op: 'select_picks', input: {
    rows: rows.map(r => ({ sellable: num(r.sellable), patience: r.patience, hold: r.timing === 'hold', score: num(r.sell_score) })),
    ...(cap === undefined ? {} : { cap }),
  } });
  return picked.map(i => rows[i]!);
}
