// A user's pricing rule: which market figure a suggested ask starts from, and
// how many platinum to add to it. My orders' health check flags and reprices
// sell listings by it, so a player who prices at "90-day median +1p" sees that
// number instead of the app's built-in "match the lowest ask". Set in
// Settings → Selling.
//
// A rule only changes what is suggested. Nothing reprices on its own.

export type PriceBasis = 'lowest-ask' | 'low5-avg' | 'low20-avg' | 'avg-48h' | 'median-90d';

export interface PriceRule {
  basis: PriceBasis;
  offset: number;
}

/** Reproduces the behaviour before rules existed: match the lowest other ask. */
export const DEFAULT_PRICE_RULE: PriceRule = { basis: 'lowest-ask', offset: 0 };

export const MIN_PRICE_OFFSET = -50;
export const MAX_PRICE_OFFSET = 50;

const BASES: readonly PriceBasis[] = ['lowest-ask', 'low5-avg', 'low20-avg', 'avg-48h', 'median-90d'];

/** The figures a basis can read. Snapshots write `0` for a statistic they do
 *  not carry, so `0`, `null` and absence all mean "unknown". */
export interface PriceEvidence {
  /** Lowest other online ask (own order excluded), or the troll-clamped
   *  clearing price when no live check has run. */
  lowestAsk?: number | null;
  low5Avg?: number | null;
  /** Snapshot only: a live check returns at most five asks. */
  low20Avg?: number | null;
  avg48h?: number | null;
  median90d?: number | null;
}

function known(value: number | null | undefined): number | null {
  return typeof value === 'number' && Number.isFinite(value) && value > 0 ? value : null;
}

function basisValue(basis: PriceBasis, evidence: PriceEvidence): number | null {
  switch (basis) {
    case 'lowest-ask': return known(evidence.lowestAsk);
    case 'low5-avg': return known(evidence.low5Avg);
    case 'low20-avg': return known(evidence.low20Avg);
    case 'avg-48h': return known(evidence.avg48h);
    case 'median-90d': return known(evidence.median90d);
  }
}

/** The suggested per-unit ask, or `null` when the rule's basis is unknown for
 *  this item - an absent statistic must not turn into a plausible-looking price.
 *
 *  Not rounded: bulk listings compare fractional unit prices, and rounding here
 *  hid a real overpricing (6.66p against a 6.5p ask became "fine" against 7p).
 *  Callers round when they build the lot total. A negative offset cannot take
 *  the price below 1p, or below the basis itself when that is already under 1p. */
export function priceFromRule(rule: PriceRule, evidence: PriceEvidence): number | null {
  const base = basisValue(rule.basis, evidence);
  if (base == null) return null;
  return Math.max(Math.min(base, 1), base + rule.offset);
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
