// Demand, from DE's own telemetry.
//
// The app's answer to "what's worth selling" has always been circular: spread
// and volume from warframe.market tell you what is being TRADED, not what is
// being WANTED. Two parts at 12p with a 5p spread and single-digit volume look
// identical - and one of them is a piece of a weapon 5% of players run while
// the other belongs to something nobody has equipped since 2019. The first
// sells tonight; the second sits in your listings for a month.
//
// DE publishes per-Mastery-Rank usage share for 694 frames and weapons and has
// done for six years. This module turns that into a liquidity signal.
//
// TWO HONESTIES the UI must carry:
//   - Usage is measured on the PARENT (the weapon or frame), so a part
//     inherits its parent's figure. That is an approximation, not per-part
//     telemetry.
//   - The data is annual and published in arrears. The year is part of the
//     record so a reader knows how fresh it is.

import type { Market } from './types';
import { DEAD_SHARE, usageWeight } from './sell-priority';

export { DEAD_SHARE, POPULAR_SHARE } from './sell-priority';

export interface UsageEntry {
  name: string;
  category: string;
  year: number;
  /** Percentage of that category's total usage. */
  share: number;
  /** Mastery Rank at which this item's usage peaks. */
  peak_mr: number;
  /** Usage share at MR 0 … 33. */
  by_mr: number[];
}

export type UsageSurface = Record<string, UsageEntry>;

export type UsageParentIndex = Map<string, string | null>;

function validUsage(value: unknown): value is UsageEntry {
  if (!value || typeof value !== 'object') return false;
  const row = value as Partial<UsageEntry>;
  return typeof row.name === 'string' && row.name.length > 0
    && typeof row.category === 'string' && row.category.length > 0
    && Number.isInteger(row.year) && (row.year ?? 0) > 0
    && typeof row.share === 'number' && Number.isFinite(row.share) && row.share >= 0
    && typeof row.peak_mr === 'number' && Number.isFinite(row.peak_mr) && row.peak_mr >= 0
    && Array.isArray(row.by_mr) && row.by_mr.length > 0
    && row.by_mr.every((value) => typeof value === 'number' && Number.isFinite(value) && value >= 0);
}

export function buildUsageParentIndex(market: Market | null | undefined): UsageParentIndex {
  const index: UsageParentIndex = new Map();
  for (const [parent, set] of Object.entries(market?.set_to_parts ?? {})) {
    for (const part of set?.parts ?? []) {
      if (!part?.slug) continue;
      if (!index.has(part.slug)) index.set(part.slug, parent);
      else if (index.get(part.slug) !== parent) index.set(part.slug, null);
    }
  }
  return index;
}

const parentIndexCache = new WeakMap<Market, UsageParentIndex>();

function parentIndexFor(market: Market): UsageParentIndex {
  let index = parentIndexCache.get(market);
  if (!index) {
    index = buildUsageParentIndex(market);
    parentIndexCache.set(market, index);
  }
  return index;
}

/**
 * Usage for a slug, following a part up to its parent set when needed.
 *
 * `inherited` is not decoration - a part showing its parent's number without
 * saying so would imply per-part telemetry that does not exist.
 */
export function usageFor(
  slug: string,
  market: Market | null | undefined,
): { entry: UsageEntry; inherited: boolean } | null {
  if (!market) return null;
  const usage = market.usage as UsageSurface | undefined | null;
  if (!usage) return null;

  if (Object.hasOwn(usage, slug)) {
    const direct = usage[slug];
    return validUsage(direct) ? { entry: direct, inherited: false } : null;
  }

  const parentSlug = parentIndexFor(market).get(slug);
  if (parentSlug) {
    const parent = usage[parentSlug];
    if (validUsage(parent)) return { entry: parent, inherited: true };
  }
  return null;
}

/** Share at or above which an item counts as genuinely popular within its
 *  category. Categories hold 113–233 items, so an even split would be well
 *  under 1% - a whole percent is a real signal. */
/** 48h trades below which the market side of the signal is too thin to lean
 *  on. Matches the threshold the relic planner uses. */
export const THIN_VOLUME = 5;

export type Liquidity =
  | 'sells-today' //   played and trading - list at market and it moves
  | 'underpriced' //   played and trading, but priced below its own baseline
  | 'slow' //          barely played; list high and be patient
  | 'thin' //          barely traded, whatever the usage says
  | 'unknown'; //      no usage data for this item or its parent

export interface DemandRead {
  usage: UsageEntry | null;
  inherited: boolean;
  liquidity: Liquidity;
  /** Mastery-Rank band that accounts for most of this item's usage - who is
   *  actually buying it. Null when there is no usage curve. */
  band: { from: number; to: number } | null;
}

/**
 * The Mastery-Rank band holding the middle `coverage` of an item's usage.
 *
 * A single peak MR is a spike on a noisy curve; the band is what a seller can
 * act on ("price this for MR 5–14, not against a whole-population median").
 * Walks outward from the peak until enough mass is covered, which handles the
 * bimodal curves a few weapons genuinely have.
 */
export function masteryBand(byMr: number[], coverage = 0.6): { from: number; to: number } | null {
  const total = byMr.reduce((a, b) => a + b, 0);
  if (!(total > 0)) return null;

  let peak = 0;
  for (let i = 1; i < byMr.length; i += 1) if (byMr[i] > byMr[peak]) peak = i;

  let from = peak;
  let to = peak;
  let acc = byMr[peak];
  while (acc / total < coverage && (from > 0 || to < byMr.length - 1)) {
    const left = from > 0 ? byMr[from - 1] : -1;
    const right = to < byMr.length - 1 ? byMr[to + 1] : -1;
    if (right > left) {
      to += 1;
      acc += byMr[to];
    } else {
      from -= 1;
      acc += byMr[from];
    }
  }
  return { from, to };
}

/**
 * Fuse usage with the market signal.
 *
 * Volume gates everything: an item nobody has traded in 48h is thin no matter
 * how popular its parent weapon is, because popularity does not make a buyer
 * appear tonight. Only past that gate does usage separate "sells today" from
 * "list high and wait".
 */
export function readDemand(
  slug: string,
  market: Market | null | undefined,
  opts: { vol: number; price: number | null; baseline: number | null },
): DemandRead {
  const hit = usageFor(slug, market);
  const usage = hit?.entry ?? null;
  const band = usage ? masteryBand(usage.by_mr) : null;

  let liquidity: Liquidity;
  if (opts.vol < THIN_VOLUME) {
    liquidity = 'thin';
  } else if (!usage) {
    liquidity = 'unknown';
  } else if (usage.share < DEAD_SHARE) {
    liquidity = 'slow';
  } else if (
    opts.price != null &&
    opts.baseline != null &&
    opts.baseline > 0 &&
    opts.price < opts.baseline * 0.9
  ) {
    liquidity = 'underpriced';
  } else {
    liquidity = 'sells-today';
  }

  return { usage, inherited: hit?.inherited ?? false, liquidity, band };
}

/**
 * A multiplier for the sell ranking, from usage.
 *
 * Deliberately gentle - 0.75× to 1.25×. Usage is a real signal but it is
 * annual, measured on the parent, and about equipping rather than buying;
 * letting it dominate a ranking built from live prices would be trading one
 * kind of overconfidence for another. Items with no usage data score 1.0 and
 * are neither rewarded nor punished.
 */
export function liquidityWeight(usage: UsageEntry | null): number {
  return usageWeight(usage?.share);
}
