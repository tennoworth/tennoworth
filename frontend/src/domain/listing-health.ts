// Listing health - the state of each of YOUR live WFM listings against two
// things the panel can know right now: the live top-of-book for that exact
// tier (with your own order already excluded - see wfm-trading `market::live_top`), and
// what your latest inventory scan says you still own.
//
// This is deliberately not the snapshot-based drift check in `order-drift.ts`
// (which stays as the always-available fallback). Live data removes that
// module's two caveats - the snapshot lags by up to 2 h and cannot tell whose
// order is whose - so the verdicts here can be blunter: "someone is asking
// less than you", "someone is bidding more than you ask", "you don't own that
// many any more". Everything is a suggestion; the user clicks.

import type { LiveTop } from '../contracts/desktop';
import { DEFAULT_PRICE_RULE, priceFromRule, type PriceRule } from './price-rule';

export type HealthKind =
  /** Your ask is above your price rule's target, by default the lowest OTHER
   *  online ask - you won't sell first. */
  | 'overpriced'
  /** A live buyer bids MORE than you're asking - you'd leave plat on the table. */
  | 'underbid'
  /** The listing's quantity exceeds what the last scan says you own. */
  | 'excess-qty'
  /** The last scan says you own none - the listing is a ghost. */
  | 'not-owned';

export interface HealthInput {
  id: string;
  slug: string;
  name: string;
  platinum: number;
  quantity: number;
  type: 'sell' | 'buy';
  /** Live top-of-book for this order's tier, if fetched; `null` = not checked. */
  live?: LiveTop | null;
  /** Tradeable copies owned per the latest scan; `null` = unknown (no scan). */
  owned?: number | null;
  /** Snapshot statistics for the rule bases a live check does not report. */
  stats?: { low5Avg?: number | null; avg48h?: number | null; median90d?: number | null };
}

/** What the player asked the health check to flag. The defaults reproduce the
 *  verdicts from before preferences existed. */
export interface HealthPrefs {
  rule: PriceRule;
  /** Platinum an ask may sit above the rule's target before it is flagged. */
  tolerance: number;
  muted: HealthKind[];
}

export const MAX_HEALTH_TOLERANCE = 100;
const KINDS: readonly HealthKind[] = ['overpriced', 'underbid', 'excess-qty', 'not-owned'];

const DEFAULT_HEALTH_PREFS: HealthPrefs = { rule: DEFAULT_PRICE_RULE, tolerance: 0, muted: [] };

/** Reads the `listing-health` setting. The rule is passed in because it has
 *  its own `price-rule` setting, shared with the Sell view. */
export function parseHealthPrefs(raw: string | null, rule: PriceRule): HealthPrefs {
  const fallback = { ...DEFAULT_HEALTH_PREFS, rule };
  if (!raw) return fallback;
  try {
    const value = JSON.parse(raw) as { tolerance?: unknown; muted?: unknown };
    const tolerance = typeof value.tolerance === 'number' && Number.isInteger(value.tolerance)
      ? Math.min(MAX_HEALTH_TOLERANCE, Math.max(0, value.tolerance)) : 0;
    const muted = Array.isArray(value.muted) ? KINDS.filter(kind => (value.muted as unknown[]).includes(kind)) : [];
    return { rule, tolerance, muted };
  } catch {
    return fallback;
  }
}

export interface HealthIssue {
  id: string;
  slug: string;
  name: string;
  kind: HealthKind;
  /** What the listing says now (price for price kinds, quantity for qty kinds). */
  current: number;
  /** What to change it to. `0` on `not-owned` means "delete". */
  suggested: number;
}

/** Only sell listings are assessed - buy orders are the user's own bids and
 *  "someone asks less than your bid" is not a problem, it's a purchase. */
export function assessListing(o: HealthInput, prefs: HealthPrefs = DEFAULT_HEALTH_PREFS): HealthIssue[] {
  const out: HealthIssue[] = [];
  if (o.type !== 'sell') return out;

  const t = o.live;
  if (t && !t.error) {
    const target = priceFromRule(prefs.rule, { lowestAsk: t.low_sell, ...o.stats });
    if (target != null && o.platinum > target + prefs.tolerance) {
      out.push({
        id: o.id, slug: o.slug, name: o.name, kind: 'overpriced',
        current: o.platinum, suggested: target,
      });
    } else if (t.top_buy != null && o.platinum < t.top_buy) {
      out.push({
        id: o.id, slug: o.slug, name: o.name, kind: 'underbid',
        current: o.platinum, suggested: t.top_buy,
      });
    }
  }

  if (o.owned != null) {
    if (o.owned <= 0) {
      out.push({
        id: o.id, slug: o.slug, name: o.name, kind: 'not-owned',
        current: o.quantity, suggested: 0,
      });
    } else if (o.quantity > o.owned) {
      out.push({
        id: o.id, slug: o.slug, name: o.name, kind: 'excess-qty',
        current: o.quantity, suggested: o.owned,
      });
    }
  }
  return prefs.muted.length ? out.filter(issue => !prefs.muted.includes(issue.kind)) : out;
}

export function assessListings(orders: HealthInput[], prefs: HealthPrefs = DEFAULT_HEALTH_PREFS): HealthIssue[] {
  return orders.flatMap(o => assessListing(o, prefs));
}

export interface HealthSummary {
  overpriced: number;
  underbid: number;
  excessQty: number;
  notOwned: number;
  total: number;
}

export function summarize(issues: HealthIssue[]): HealthSummary {
  const s: HealthSummary = { overpriced: 0, underbid: 0, excessQty: 0, notOwned: 0, total: issues.length };
  for (const i of issues) {
    if (i.kind === 'overpriced') s.overpriced += 1;
    else if (i.kind === 'underbid') s.underbid += 1;
    else if (i.kind === 'excess-qty') s.excessQty += 1;
    else if (i.kind === 'not-owned') s.notOwned += 1;
  }
  return s;
}

/** Key for the owned-quantity map: slug plus relic refinement, matching the
 *  Sell view's `OwnedRecord` keying. Rank isn't part of it - the scan counts
 *  a mod's copies, not per-rank. */
export function ownedKey(slug: string, subtype: string | null | undefined): string {
  return `${slug}|${subtype ?? ''}`;
}

/** Ownership evidence for one listing, from the last scan's counts.
 *
 *  `null` means "not assessable" and suppresses every ownership verdict.
 *
 *  A composed item - a prime set, assembled from parts - is never reported by a
 *  scan that reads individual items, so its absence from the map is absence of
 *  evidence rather than evidence of absence. Reading that absence as `0`
 *  produced a false "you don't own this" and offered a one-click delete of a
 *  live listing the user could in fact build.
 *
 *  Evidence that IS present is always used, so a set the scan really carries a
 *  count for is still assessed. A count of `0` is evidence, not absence.
 *
 *  `composed` comes from the market snapshot. Until it is known, no absent item
 *  can be told apart from a set, so every absence is unassessable. */
export function ownedEvidence(
  slug: string,
  subtype: string | null | undefined,
  ownedQty: ReadonlyMap<string, number> | null | undefined,
  composed: ReadonlySet<string> | null,
): number | null {
  if (!ownedQty) return null;
  const known = ownedQty.get(ownedKey(slug, subtype));
  if (known != null) return known;
  return !composed || composed.has(slug) ? null : 0;
}
