// A user's pricing rule: which market figure a suggested ask starts from, and
// how many platinum to add to it. My orders' health check flags and reprices
// sell listings by it, and the listing review prefills new listings with it, so a player who prices at "90-day median +1p" sees that
// number instead of the app's built-in "match the lowest ask". Set in
// Settings → Selling.
//
// The rule and the live-ask read are native (market_domain::listing); this
// file reads the stored setting and picks a snapshot row's evidence.

import type { MarketItemEntry } from '../contracts/data';
import type { LiveAskRead, PriceBasis, PriceEvidence, PriceRule } from '../contracts/generated/web';
import { call } from './native';
import { cachedUnitMarket, marketAtRank } from './order-prices';

export type { LiveAskRead, PriceBasis, PriceEvidence, PriceRule };

/** Reproduces the behaviour before rules existed: match the lowest other ask. */
export const DEFAULT_PRICE_RULE: PriceRule = { basis: 'lowest-ask', offset: 0 };

export const MIN_PRICE_OFFSET = -50;
export const MAX_PRICE_OFFSET = 50;

const BASES: readonly PriceBasis[] = ['lowest-ask', 'low5-avg', 'low20-avg', 'avg-48h', 'median-90d'];

/** Snapshots write `0` for a statistic they do not carry. */
function known(value: number | null | undefined): number | null {
  return typeof value === 'number' && Number.isFinite(value) && value > 0 ? value : null;
}

/** The suggested per-unit ask, unrounded, or `null` when the rule's basis is
 *  unknown for this item. */
export function priceFromRule(rule: PriceRule, evidence: PriceEvidence): number | null {
  return call({ op: 'price_from_rule', input: { rule, evidence } });
}

/** The rule bases a snapshot entry carries, plus the median the live-ask
 *  read judges outliers against (today's, else the 90-day baseline). Pass an
 *  entry through `cachedUnitMarket` first: lot-priced books are not unit prices. */
export function snapshotEvidence(m: MarketItemEntry | null | undefined): PriceEvidence & { median: number | null } {
  if (!m) return { median: null };
  return {
    low5Avg: m.low5_avg, low20Avg: m.low20_avg, avg48h: m.avg, median90d: m.median_90d,
    median: known(m.median_now) ?? known(m.median_90d),
  };
}

/** The rule evidence for one rank of an item. A rank the snapshot does not
 *  describe keeps only the rank-0 median, which still judges troll asks on its
 *  tier - an ask under a third of the unranked price is no ranked price
 *  either - so its rule falls back to that rank's live asks. */
export function rankEvidence(m: MarketItemEntry | null | undefined, rank: number | null | undefined, bulk: boolean): PriceEvidence & { median: number | null } {
  if (!m) return { median: null };
  const tier = marketAtRank(m, rank);
  return tier ? snapshotEvidence(cachedUnitMarket(tier, bulk)) : { median: snapshotEvidence(m).median };
}

/** The cheapest live ask "within reason": a troll undercut under the median
 *  is skipped. `asks` is the live check's list, cheapest first. */
export function reasonableLowestAsk(asks: readonly number[], median: number | null | undefined): LiveAskRead {
  return call({ op: 'reasonable_lowest_ask', input: { asks: [...asks], median } });
}

export function parsePriceRule(raw: string | null): PriceRule {
  if (!raw) return DEFAULT_PRICE_RULE;
  try {
    const value = JSON.parse(raw) as Partial<PriceRule>;
    const basis = BASES.find(b => b === value.basis);
    const offset = value.offset;
    if (!basis || typeof offset !== 'number' || !Number.isInteger(offset)) return DEFAULT_PRICE_RULE;
    return { basis, offset: Math.min(MAX_PRICE_OFFSET, Math.max(MIN_PRICE_OFFSET, offset)) };
  } catch {
    return DEFAULT_PRICE_RULE;
  }
}
