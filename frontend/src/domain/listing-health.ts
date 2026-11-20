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
import { DEFAULT_PRICE_RULE, priceFromRule, reasonableLowestAsk, type PriceRule } from './price-rule';
import { DRIFT_PCT, MIN_DRIFT_PLAT } from './order-drift';
import { LIQUID_VOL } from './sell-priority';

export type HealthKind =
  /** Your ask is above your price rule's target, by default the lowest OTHER
   *  online ask - you won't sell first. */
  | 'overpriced'
  /** A live buyer bids MORE than you're asking - you'd leave plat on the table. */
  | 'underbid'
  /** Your ask sits well under your price rule's target. It still sells, for
   *  less than it could; the gap must clear the drift check's gates, because a
   *  small undercut is how most players sell. */
  | 'underpriced'
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
  /** Items per trade; `platinum` is the per-unit price of that lot. */
  perTrade?: number | null;
  /** Snapshot 48-hour volume of the listing's rank, for the thin-book mark;
   *  `null` when the snapshot does not describe that rank. */
  vol?: number | null;
  /** Live top-of-book for this order's tier, if fetched; `null` = not checked. */
  live?: LiveTop | null;
  /** Tradeable copies owned per the latest scan; `null` = unknown (no scan). */
  owned?: number | null;
  /** Snapshot statistics of the listing's rank (see `rankEvidence`) for the
   *  rule bases a live check does not report. */
  stats?: { low5Avg?: number | null; low20Avg?: number | null; avg48h?: number | null; median90d?: number | null; median?: number | null };
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
export const HEALTH_KINDS: readonly HealthKind[] = ['overpriced', 'underbid', 'underpriced', 'excess-qty', 'not-owned'];

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
    const muted = Array.isArray(value.muted) ? HEALTH_KINDS.filter(kind => (value.muted as unknown[]).includes(kind)) : [];
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
  /** What to change it to. `0` on `not-owned` means "delete". Price kinds
   *  give the per-unit price of `suggestedLot`. */
  suggested: number;
  /** Price kinds only: the whole-platinum lot price a reprice sends. */
  suggestedLot?: number;
  /** `underpriced` only: the book is too thin for the target to be firm. */
  thin?: boolean;
  /** `overpriced` only: the cheapest live ask, skipped as an outlier. */
  ignoredAsk?: number;
}

/** Only sell listings are assessed - buy orders are the user's own bids and
 *  "someone asks less than your bid" is not a problem, it's a purchase. */
export function assessListing(o: HealthInput, prefs: HealthPrefs = DEFAULT_HEALTH_PREFS): HealthIssue[] {
  const out: HealthIssue[] = [];
  if (o.type !== 'sell') return out;

  const t = o.live;
  if (t && !t.error) {
    const lot = lotSize(o.perTrade);
    const listedLot = Math.round(o.platinum * lot);
    const sells = t.sells ?? [];
    const read = sells.length ? reasonableLowestAsk(sells, o.stats?.median) : { ask: t.low_sell, ignored: null };
    const believable = read.ask == null ? [] : sells.slice(Math.max(0, sells.indexOf(read.ask)));
    const liveLow5 = believable.length ? believable.reduce((a, b) => a + b, 0) / believable.length : null;
    // The live asks stand in for a snapshot average only on a rank the
    // snapshot does not describe, whose stats carry just the median.
    const evidence = { low5Avg: liveLow5, ...o.stats, lowestAsk: read.ask };
    const target = priceFromRule(prefs.rule, evidence);
    // WFM takes whole platinum per lot. Comparing against the fractional
    // target flagged a 16p ask against 15.4p, and Reprice then sent 16p again.
    const targetLot = target == null ? null : wholeLot(target, lot);
    const priced = (kind: HealthKind, lotPrice: number, extra: Partial<HealthIssue> = {}): HealthIssue => ({
      id: o.id, slug: o.slug, name: o.name, kind, current: o.platinum, suggested: lotPrice / lot, suggestedLot: lotPrice, ...extra,
    });
    if (targetLot != null && listedLot > targetLot + prefs.tolerance * lot) {
      out.push(priced('overpriced', targetLot, read.ignored != null ? { ignoredAsk: read.ignored } : {}));
    } else if (t.top_buy != null && o.platinum < t.top_buy) {
      out.push(priced('underbid', wholeLot(t.top_buy, lot)));
    } else if (targetLot != null) {
      const gap = targetLot / lot - o.platinum;
      if (gap >= MIN_DRIFT_PLAT && gap / o.platinum >= DRIFT_PCT) {
        out.push(priced('underpriced', targetLot, o.vol != null && o.vol < LIQUID_VOL ? { thin: true } : {}));
      }
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

function lotSize(perTrade: number | null | undefined): number {
  return Number.isSafeInteger(perTrade) && (perTrade as number) >= 1 ? perTrade as number : 1;
}

// Rounded to six places first so float noise (6.5 × 6 = 39.000000000000004)
// cannot add a platinum.
function wholeLot(unit: number, lot: number): number {
  return Math.max(1, Math.ceil(Number((unit * lot).toFixed(6))));
}

export function assessListings(orders: HealthInput[], prefs: HealthPrefs = DEFAULT_HEALTH_PREFS): HealthIssue[] {
  return orders.flatMap(o => assessListing(o, prefs));
}

export interface HealthSummary {
  overpriced: number;
  underbid: number;
  underpriced: number;
  excessQty: number;
  notOwned: number;
  total: number;
}

export function summarize(issues: HealthIssue[]): HealthSummary {
  const s: HealthSummary = { overpriced: 0, underbid: 0, underpriced: 0, excessQty: 0, notOwned: 0, total: issues.length };
  for (const i of issues) {
    if (i.kind === 'overpriced') s.overpriced += 1;
    else if (i.kind === 'underbid') s.underbid += 1;
    else if (i.kind === 'underpriced') s.underpriced += 1;
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
