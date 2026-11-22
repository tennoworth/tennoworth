// What the views need synchronously from the sell-priority rules: the
// clamped clearing price, listable and spare quantities, the thin-book line,
// the timing states and the Top-picks bar. The priority score itself is native
// (market_domain::math::sell_priority): the desktop gets it from
// score_inventory and the homepage from the pipeline's handoff_sample.

import type { MarketItemEntry } from '../contracts/data';

type PricedEntry = Pick<
  MarketItemEntry,
  'vol' | 'low_sell' | 'avg' | 'median_now' | 'median_90d'
> | null | undefined;

// Below this many closed trades / 48 h the book is too thin to trust its ask
// as a forecast, and too thin to certify a trend. The one TypeScript home for
// the threshold: the ask clamp and the thin-book tags read it, and
// tests/fixtures/sell-priority/liquidity-boundary.json pins it against the
// native copy.
export const LIQUID_VOL = 5;

// "Keep copies" reserve - N copies of every owned item held back from ever
// being listed, so the user can't sell their only copy of something by
// accident. 0 (the default) is a no-op; clamped so a stale/edited
// localStorage value can never go negative.
//
// `leveled` = how many owned copies carry XP > 0. Warframe only allows
// trading UNRANKED gear - any XP makes that specific copy untradeable - so
// a leveled copy already satisfies the "don't sell my only copy" intent the
// reserve exists for. Leveled copies and the reserve don't stack: the hold-
// back is whichever is larger, not their sum.
export function sellableQty(count: number, reserve: number, leveled = 0): number {
  return Math.max(0, count - Math.max(reserve, leveled));
}

// The Spares preset's notion of "sellable": copies you would otherwise
// dissolve for endo. If you own a RANKED copy of the mod/arcane (kept_lvl > 0
// - the one you actually use), every unranked copy in the stack is a spare.
// If you don't, keep one to rank up later and the rest are spares. `leveled`
// copies (XP > 0, untradeable) never count. This is what the "you dissolved
// 30 Hammer Shot, oh well" threads are about - the tool's cheapest real win.
export function spareQty(count: number, keptLvl: number | null, leveled = 0): number {
  const tradeable = Math.max(0, count - leveled);
  const hasRankedCopy = keptLvl !== null && keptLvl > 0;
  return hasRankedCopy ? tradeable : Math.max(0, tradeable - 1);
}

// What a listing would realistically clear at. The lowest live ask is the
// honest answer MOST of the time, but it's a single number any account can
// set for free, so it gets sanity-clamped against the closed-trade median:
//  - ask < median/3  → a troll/stale 1p undercut (akbolto receiver: one 1p
//    ask under a stable 38p median cratered its score, killed its peak pill,
//    and fired a "feed it to Baro" deal badge). One such listing absorbs one
//    sale; the realistic price is still the median.
//  - thin item (vol < LIQUID_VOL) with ask > 1.5× median → an aspirational
//    ask nobody pays. The old gate was vol < 2 with a 3× tolerance, which
//    vol-2 items dodged entirely: winding_isles_scene (vol 2, one 100p ask
//    over a 10p median) ranked #7 at "expected 100p/day". On a thin book
//    the closed trades are the evidence; cap at 1.5× median. Liquid items
//    keep their ask - there the book is real information.
// No ask at all → median, then avg, floor 1.

/** An ask this many times under the median is a troll or stale undercut, not
 *  a price. Shared with the live-ask read in price-rule.ts; the native
 *  `clearing_price` carries the same 3 under the parity fixture. */
export const TROLL_UNDERCUT_RATIO = 3;

export function clearingPrice(m: PricedEntry): number {
  const lowSell = Number(m?.low_sell) || 0;
  const median = Number(m?.median_now) || Number(m?.median_90d) || 0;
  const avg = Number(m?.avg) || 0;
  const vol = Number(m?.vol) || 0;
  if (lowSell <= 0) return median > 0 ? median : Math.max(1, avg);
  if (median > 0) {
    if (lowSell * TROLL_UNDERCUT_RATIO < median) return median;
    if (vol < LIQUID_VOL && lowSell > median * 1.5) return median * 1.5;
  }
  return lowSell;
}

// Where today's median sits in its 90-day channel: native scoring::timing
// decides it (tests/fixtures/sell-priority/timing.json). "hold" is a dip near
// the 90-day low, "drop" the same on heavy selling, "peak" a corroborated high.
export type TimingState = 'hold' | 'drop' | 'peak' | 'neutral';

// "Top picks" strip - the best rows to act on right now, out of everything
// already scored above; the evening digest uses the same bar (pick-floor.json).
// `MIN_PICK_SCORE` is a first-pass guess, not a
// threshold: 20 score points filters out the long tail of items that
// technically clear sell_score > 0 but aren't worth a user's attention.
// Tune from observed ranking quality rather than treating it as currency.
export const MIN_PICK_SCORE = 20; // tunable prioritization floor
export const MAX_PICKS = 5;

// Only the fields selectPicks actually reads. Generic-constrained so the
// caller's full row shape (name, slug, timing, clearing_price, …) passes
// through untouched - the picks UI and the listing modal both need most of
// that shape, so returning a stripped-down copy would just force a second
// lookup back into `results` for every row a pick surfaces.
export interface PickCandidate {
  sellable: number;
  patience: boolean;
  sell_score: number;
  /** A "hold" row sells near its 90-day low; a pick is "sell this now". */
  timing?: TimingState;
}

export interface SelectPicksOptions {
  cap?: number;
  minScore?: number;
}

// Rows must already be sorted best-first (sell_score desc) - App.svelte's
// computeResults() does that once for the whole table. This function only
// filters and truncates: re-sorting here would cost nothing today but would
// silently paper over a caller that stopped guaranteeing the order, which is
// exactly the kind of bug that should be visible, not masked.
export function selectPicks<T extends PickCandidate>(
  rows: T[],
  { cap = MAX_PICKS, minScore = MIN_PICK_SCORE }: SelectPicksOptions = {}
): T[] {
  return rows
    .filter((r) => r.sellable > 0 && !r.patience && r.timing !== 'hold' && r.sell_score >= minScore)
    .slice(0, cap);
}
