// Hold-or-sell advisor - the "relic gremlins" play, computed instead of
// folklore. Joins the three inputs nobody else holds together: your scanned
// inventory (the rows this runs over), the calendar surface (prime release /
// vault dates + Resurgence rotations, from the wiki via the pipeline), and
// the year of daily prices (history.json, from relics.run).
//
// Advice only, never automation, and every verdict carries the numbers that
// produced it - a bare "sell" chip would be the single-number-riven-tool
// mistake in a different column. Rules are ordered; the first that fires
// wins. Anything without a calendar entry (non-primes, subtyped rows) gets
// null: no advice beats confident noise.

import { yearStats, type History } from './history';
import type { Market } from './types';

export type Advice = 'sell_now' | 'hold' | 'neutral';

export interface Verdict {
  advice: Advice;
  /** Short user-facing reasons, each carrying its numbers. */
  reasons: string[];
}

/** Days from `from` (ISO date or datetime) to `nowMs`; null when unparseable. */
function daysSince(from: string | undefined, nowMs: number): number | null {
  if (!from) return null;
  const t = Date.parse(from.length === 10 ? `${from}T00:00:00Z` : from);
  if (!Number.isFinite(t)) return null;
  return Math.floor((nowMs - t) / 86400000);
}

/** part slug → its set's slug, from the snapshot's set_to_parts (sets map to
 *  themselves). The calendar keys by SET slug, so this is how a part row
 *  finds its dates. */
export function buildPartToSet(market: Market | null | undefined): Map<string, string> {
  const out = new Map<string, string>();
  const s2p = market?.set_to_parts;
  if (!s2p) return out;
  for (const [setSlug, entry] of Object.entries(s2p)) {
    out.set(setSlug, setSlug);
    for (const p of entry.parts ?? []) out.set(p.slug, setSlug);
  }
  return out;
}

/** Median of a series slice's non-null values; null when under `minPts`. */
function sliceMedian(median: Array<number | null>, from: number, to: number, minPts = 5): number | null {
  const vals: number[] = [];
  for (let i = Math.max(0, from); i < Math.min(median.length, to); i++) {
    const v = median[i];
    if (v != null && Number.isFinite(v) && v > 0) vals.push(v);
  }
  if (vals.length < minPts) return null;
  vals.sort((a, b) => a - b);
  return vals[Math.floor(vals.length / 2)];
}

/** 30-day price movement as a fraction: (median of the last 15 d ÷ median of
 *  the 15 d before) − 1. Null when either half is too thin to trust. */
export function slope30(median: Array<number | null>): number | null {
  const n = median.length;
  const recent = sliceMedian(median, n - 15, n);
  const before = sliceMedian(median, n - 30, n - 15);
  if (recent == null || before == null || before <= 0) return null;
  return recent / before - 1;
}

/** Median over the 30 days BEFORE the vault date - the pre-vault baseline the
 *  post-vault ramp is measured against. */
export function preVaultMedian(
  h: Pick<History, 'start'>,
  median: Array<number | null>,
  vaultDate: string,
): number | null {
  const startMs = Date.parse(`${h.start}T00:00:00Z`);
  const vaultMs = Date.parse(vaultDate.length === 10 ? `${vaultDate}T00:00:00Z` : vaultDate);
  if (!Number.isFinite(startMs) || !Number.isFinite(vaultMs)) return null;
  const vi = Math.floor((vaultMs - startMs) / 86400000);
  if (vi <= 0) return null; // vaulted before the series starts
  return sliceMedian(median, vi - 30, vi);
}

/** How recently released still counts as the day-one decay window. */
export const FRESH_RELEASE_DAYS = 45;
/** How long after the vault the ramp is still considered "ahead". */
export const POST_VAULT_RAMP_DAYS = 270;
/** est_vault within this many days → the pre-vault hold call. */
export const VAULT_SOON_DAYS = 90;
/** median_now at ≥ this fraction of the 1-year high counts as "at the high". */
export const NEAR_HIGH = 0.9;
/** ...but only when the year actually moved: high ≥ this multiple of low. */
export const MEANINGFUL_RANGE = 1.3;
/** Resurgence + a 30-day fall steeper than this → sell into the flood. */
export const RESURGENCE_FALL = -0.05;
/** Below this multiple of the pre-vault price, the ramp is still ahead. */
export const RAMP_TARGET = 1.6;

export interface AdviseInput {
  slug: string;
  market: Market;
  /** Optional - rules that need it degrade to the calendar-only ones. */
  history?: History | null;
  partToSet: Map<string, string>;
  nowMs: number;
}

/** The verdict for one owned row's slug, or null when the item has no
 *  calendar entry (not a dated prime) - no advice is better than a guess. */
export function advise({ slug, market, history, partToSet, nowMs }: AdviseInput): Verdict | null {
  const setSlug = partToSet.get(slug);
  const cal = setSlug ? market.calendar?.primes?.[setSlug] : undefined;
  if (!cal) return null;

  const m = market.items?.[slug];
  const medianNow = (m?.median_now || m?.median_90d || 0) || null;
  const series = history?.items?.[slug];
  const stats = series ? yearStats(series) : null;
  const slope = series ? slope30(series.median) : null;
  const pct = (x: number) => `${x > 0 ? '+' : ''}${Math.round(x * 100)}%`;

  // 1. Fresh release: day-one prices decay to a floor; sell into the launch
  //    demand, don't hold through the slide.
  const sinceRelease = daysSince(cal.released, nowMs);
  if (sinceRelease != null && sinceRelease >= 0 && sinceRelease < FRESH_RELEASE_DAYS) {
    return {
      advice: 'sell_now',
      reasons: [
        `released ${sinceRelease} d ago - new-prime prices decay toward a floor over the first weeks`,
        ...(slope != null ? [`30 d move ${pct(slope)}`] : []),
      ],
    };
  }

  // 2. Resurgence: Varzia is reprinting the relics. Falling price → sell into
  //    the flood; stable price → say so, but don't call it.
  const rc = market.calendar?.resurgence_current;
  const resurgenceActive =
    !!rc && !!setSlug && (rc.frames ?? []).includes(setSlug) &&
    Date.parse(rc.from) <= nowMs && nowMs <= Date.parse(rc.to);
  if (resurgenceActive) {
    const until = new Date(rc.to).toISOString().slice(0, 10);
    if (slope != null && slope <= RESURGENCE_FALL) {
      return {
        advice: 'sell_now',
        reasons: [
          `Prime Resurgence is reprinting it until ${until}`,
          `price falling: ${pct(slope)} over 30 d`,
        ],
      };
    }
    return {
      advice: 'neutral',
      reasons: [
        `Prime Resurgence is reprinting it until ${until}`,
        slope != null ? `price holding so far (${pct(slope)} / 30 d)` : 'no price series yet',
      ],
    };
  }

  // 3. Recently vaulted, ramp still ahead: the hold that made the '25
  //    gremlins their plat. Must run before the near-high rule (see there).
  const vaultDate = cal.vault_date;
  const sinceVault = cal.vaulted ? daysSince(vaultDate, nowMs) : null;
  if (cal.vaulted && sinceVault != null && sinceVault >= 0 && sinceVault < POST_VAULT_RAMP_DAYS) {
    const pre = series && vaultDate && history ? preVaultMedian(history, series.median, vaultDate) : null;
    const rampNow = pre != null && pre > 0 && medianNow != null ? medianNow / pre : null;
    const belowRamp =
      rampNow != null ? rampNow < RAMP_TARGET
      : stats && medianNow != null ? medianNow < stats.high * 0.8
      : true; // no price data: the calendar call stands on its own
    if (belowRamp) {
      return {
        advice: 'hold',
        reasons: [
          `vaulted ${sinceVault} d ago - the post-vault ramp typically runs for months`,
          ...(rampNow != null ? [`now ×${rampNow.toFixed(1)} its pre-vault price`] : []),
          ...(slope != null ? [`30 d move ${pct(slope)}`] : []),
        ],
      };
    }
  }

  // 4. At the 1-year high - and the year actually moved. Evaluated AFTER
  //    the post-vault hold: a recently vaulted item mid-ramp prints a fresh
  //    1-year high every week, and that leading edge is exactly the wrong
  //    moment to call "sell" (the ramp typically runs for months). Once the
  //    ramp multiple clears RAMP_TARGET the hold rule stands aside and a
  //    high here means what it says.
  if (
    stats && medianNow != null &&
    stats.high >= stats.low * MEANINGFUL_RANGE &&
    medianNow >= stats.high * NEAR_HIGH
  ) {
    return {
      advice: 'sell_now',
      reasons: [
        `at ${Math.round((medianNow / stats.high) * 100)}% of its 1-year high (${Math.round(stats.high)}p)`,
        ...(slope != null ? [`30 d move ${pct(slope)}`] : []),
      ],
    };
  }

  // 5. Vault ahead: supply is about to dry up - the ramp comes after.
  const toVault = cal.vaulted ? null : daysSince(cal.est_vault_date, nowMs);
  if (toVault != null && toVault < 0 && -toVault <= VAULT_SOON_DAYS) {
    return {
      advice: 'hold',
      reasons: [
        `vault expected ~${cal.est_vault_date} (${-toVault} d) - prices typically climb once relics stop dropping`,
      ],
    };
  }

  return {
    advice: 'neutral',
    reasons: [
      ...(sinceVault != null ? [`vaulted ${sinceVault} d ago`] : ['not vaulted']),
      ...(stats && medianNow != null ? [`at ${Math.round((medianNow / stats.high) * 100)}% of its 1-year high`] : []),
      ...(slope != null ? [`30 d move ${pct(slope)}`] : []),
    ],
  };
}

/** Verdicts for every owned slug that has one. Keyed by slug (advice is
 *  per-item; a relic-refinement subtype never has a calendar entry anyway). */
export function adviseOwned(
  slugs: Iterable<string>,
  market: Market | null | undefined,
  history: History | null | undefined,
  nowMs: number,
): Map<string, Verdict> {
  const out = new Map<string, Verdict>();
  if (!market?.calendar?.primes) return out;
  const partToSet = buildPartToSet(market);
  for (const slug of slugs) {
    if (out.has(slug)) continue;
    const v = advise({ slug, market, history, partToSet, nowMs });
    if (v) out.set(slug, v);
  }
  return out;
}
