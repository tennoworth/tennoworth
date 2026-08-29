// Listing health - the state of each of YOUR live WFM listings against two
// things the panel can know right now: the live top-of-book for that exact
// tier (with your own order already excluded - see wfm-core `live_top`), and
// what your latest inventory scan says you still own.
//
// This is deliberately not the snapshot-based drift check in `order-drift.ts`
// (which stays as the always-available fallback). Live data removes that
// module's two caveats - the snapshot lags by up to 2 h and cannot tell whose
// order is whose - so the verdicts here can be blunter: "someone is asking
// less than you", "someone is bidding more than you ask", "you don't own that
// many any more". Everything is a suggestion; the user clicks.

import type { LiveTop } from './transport';

export type HealthKind =
  /** Your ask is above the lowest OTHER online ask - you won't sell first. */
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
  /** One-line, user-facing explanation with the numbers in it. */
  why: string;
}

/** Only sell listings are assessed - buy orders are the user's own bids and
 *  "someone asks less than your bid" is not a problem, it's a purchase. */
export function assessListing(o: HealthInput): HealthIssue[] {
  const out: HealthIssue[] = [];
  if (o.type !== 'sell') return out;

  const t = o.live;
  if (t && !t.error) {
    if (t.low_sell != null && o.platinum > t.low_sell) {
      out.push({
        id: o.id, slug: o.slug, name: o.name, kind: 'overpriced',
        current: o.platinum, suggested: t.low_sell,
        why: `Lowest other online ask is ${t.low_sell}p (you: ${o.platinum}p). Match it to sell first.`,
      });
    } else if (t.top_buy != null && o.platinum < t.top_buy) {
      out.push({
        id: o.id, slug: o.slug, name: o.name, kind: 'underbid',
        current: o.platinum, suggested: t.top_buy,
        why: `An online buyer bids ${t.top_buy}p - more than your ${o.platinum}p ask.`,
      });
    }
  }

  if (o.owned != null) {
    if (o.owned <= 0) {
      out.push({
        id: o.id, slug: o.slug, name: o.name, kind: 'not-owned',
        current: o.quantity, suggested: 0,
        why: 'Your last scan found no tradeable copy - the listing can only disappoint a buyer.',
      });
    } else if (o.quantity > o.owned) {
      out.push({
        id: o.id, slug: o.slug, name: o.name, kind: 'excess-qty',
        current: o.quantity, suggested: o.owned,
        why: `Listed ×${o.quantity} but your last scan found ${o.owned} tradeable.`,
      });
    }
  }
  return out;
}

export function assessListings(orders: HealthInput[]): HealthIssue[] {
  return orders.flatMap(assessListing);
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
