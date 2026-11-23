// Listing health - the state of each of YOUR live WFM listings against two
// things the panel can know right now: the live top-of-book for that exact
// tier (with your own order already excluded - see wfm-trading `market::live_top`), and
// what your latest inventory scan says you still own.
//
// This is deliberately not the snapshot-based drift check in `order-drift.ts`
// (which stays as the always-available fallback). Live data removes that
// module's two caveats - the snapshot lags by up to 2 h and cannot tell whose
// order is whose - so the verdicts can be blunter. Everything is a
// suggestion; the user clicks. The verdicts are native
// (market_domain::listing::assess_listings); this file reads the stored
// preferences and the scan's counts.

import type { HealthInput, HealthIssue, HealthKind, HealthPrefs, HealthReport } from '../contracts/generated/web';
import { call } from './native';
import { DEFAULT_PRICE_RULE, type PriceRule } from './price-rule';

export type { HealthInput, HealthIssue, HealthKind, HealthPrefs, HealthReport };

export const MAX_HEALTH_TOLERANCE = 100;
export const HEALTH_KINDS: readonly HealthKind[] = ['overpriced', 'underbid', 'underpriced', 'excess-qty', 'not-owned'];

/** The verdicts from before preferences existed. */
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

/** Every sell listing's verdicts, in listing order, and their tally. */
export function assessListings(orders: readonly HealthInput[], prefs: HealthPrefs = DEFAULT_HEALTH_PREFS): HealthReport {
  return call({ op: 'assess_listings', input: { orders: orders.map(keepPresentStats), prefs } });
}

// JSON drops a key whose value is undefined, but a snapshot average that is
// present and unknown still keeps the live asks from standing in for it, so
// it travels as null.
function keepPresentStats(o: HealthInput): HealthInput {
  return o.stats && 'low5Avg' in o.stats ? { ...o, stats: { ...o.stats, low5Avg: o.stats.low5Avg ?? null } } : o;
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
