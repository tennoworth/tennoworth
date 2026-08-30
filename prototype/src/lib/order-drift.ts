// Order drift - "the market moved under your live listings".
//
// A listing is priced once and then forgotten. The book keeps moving, so an
// ask that was competitive last week is now either stranded above the market
// (it will never sell) or sitting under it (it sells, and you leave plat on
// the table). This module answers which of your orders have drifted far enough
// to be worth a PATCH.
//
// It compares your ask against `clearingPrice(m)` - the SAME clamp the sell
// score uses, deliberately. The raw lowest ask is one number any account can
// set for free, and chasing it is how you get talked into undercutting a troll
// 1p listing. clearingPrice already refuses to trust a lone aspirational ask on
// a thin book and refuses to treat a 1p undercut as the real price, so drift
// inherits both protections instead of re-deriving them.
//
// KNOWN LIMIT, stated because it bounds what this can honestly claim: the
// market snapshot does not say whose order is whose, so `low_sell` INCLUDES
// your own listing. If you are the lowest ask, your price and the reference
// price are the same number and no drift is reported - which is the right
// answer (you are already competitive), but it is a consequence of the data,
// not a judgement. It also means drift can only ever say "relative to the last
// snapshot", which lags by up to the scrape interval.

import type { MarketItemEntry } from './types';
import { clearingPrice, LIQUID_VOL } from './sell-priority';

/** How far off the reference price an order must be before it is worth a
 *  PATCH. Below this, repricing costs a WFM write and a trip through the
 *  rate limiter to chase noise. */
export const DRIFT_PCT = 0.2;

/** …and it must also move by at least this much plat. 20% of a 5p item is 1p;
 *  surfacing that is churn, not advice. Both gates must pass. */
export const MIN_DRIFT_PLAT = 3;

export type DriftKind = 'overpriced' | 'underpriced';

export interface DriftInput {
  id: string;
  slug: string;
  name: string;
  platinum: number;
  /** Only 'sell' orders are analysed - see `selectDrifted`. */
  type?: 'sell' | 'buy';
  m: Pick<MarketItemEntry, 'vol' | 'low_sell' | 'avg' | 'median_now' | 'median_90d'> | null | undefined;
}

export interface DriftRow {
  id: string;
  slug: string;
  name: string;
  /** What the order is listed at now. */
  listed: number;
  /** What it would be repriced to. */
  suggested: number;
  /** Signed difference, suggested − listed. */
  delta: number;
  /** Magnitude of the move as a fraction of the listed price. */
  delta_pct: number;
  kind: DriftKind;
  /** Below the liquidity floor the book is thin enough that the suggestion is
   *  weak evidence - the UI de-emphasises rather than hides, matching how the
   *  sell picks treat the same threshold. */
  thin: boolean;
}

/**
 * Orders whose price has drifted far enough from the market to be worth
 * repricing, biggest move first.
 *
 * Buy orders are skipped rather than inverted: every reference price here
 * describes what a SELLER clears at, so reusing it for a buy order would give
 * confidently wrong advice. Better to say nothing.
 */
export function selectDrifted(orders: readonly DriftInput[], limit = 25): DriftRow[] {
  const rows: DriftRow[] = [];

  for (const o of orders) {
    if (o.type === 'buy') continue;
    const listed = Number(o.platinum) || 0;
    if (listed <= 0) continue;
    if (!o.m) continue;

    const suggested = Math.round(clearingPrice(o.m));
    if (suggested <= 0) continue;

    const delta = suggested - listed;
    if (Math.abs(delta) < MIN_DRIFT_PLAT) continue;
    const delta_pct = Math.abs(delta) / listed;
    if (delta_pct < DRIFT_PCT) continue;

    rows.push({
      id: o.id,
      slug: o.slug,
      name: o.name,
      listed,
      suggested,
      delta,
      delta_pct,
      kind: delta < 0 ? 'overpriced' : 'underpriced',
      thin: (Number(o.m.vol) || 0) < LIQUID_VOL,
    });
  }

  // Biggest absolute plat swing first - that is the order in which repricing
  // actually pays, and it keeps a 200p stranded listing above a 4p nudge.
  rows.sort((a, b) => Math.abs(b.delta) - Math.abs(a.delta));
  return rows.slice(0, limit);
}
