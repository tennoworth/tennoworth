// Riven appraisal and the comps sample read. Both are native
// (market_domain::rivens::appraise, which documents why the appraisal reports
// landmarks rather than a fitted distribution) and run from the view module
// (native.ts); this file is their call shape. The price always comes from the
// user - an offer they received, or their own estimate.

import type { RivenAuction } from '../contracts/desktop';
import type { RivenStatTier } from '../contracts/data';
import type { Appraisal, AppraisalCaveat, CompsSampleRead } from '../contracts/generated/web';
import { call, native } from './native';

export type { Appraisal, AppraisalCaveat, CompsSampleRead };

const num = (v: unknown) => Number(v) || 0;

/** Appraise an offer against a weapon's DE band. */
export function appraise(
  price: number | null,
  tier: RivenStatTier | null | undefined,
  rerolls: number,
  locked = false,
): Appraisal {
  return call({ op: 'appraise_riven', input: {
    price,
    tier: tier
      ? { avg: num(tier.avg), median: num(tier.median), min: num(tier.min), max: num(tier.max), stddev: num(tier.stddev), pop: num(tier.pop) }
      : null,
    rerolls,
    locked,
  } });
}

/** How much less a rerolled riven fetches than an unrolled one on this weapon;
 *  null when either band has no price. */
export function rerolledDiscount(
  unrolled: RivenStatTier | null | undefined,
  rolled: RivenStatTier | null | undefined,
): number | null {
  const d = native().web_riven_rerolled_discount(num(unrolled?.median), num(rolled?.median));
  return Number.isNaN(d) ? null : d;
}

/** What the comps sample on screen is, stated as facts. WFM's instants are
 *  read with Date.parse, as the listing-age column beside it reads them. */
export function describeComps(comps: readonly RivenAuction[], nowMs: number): CompsSampleRead {
  const instant = (ms: number) => (Number.isFinite(ms) ? ms : null);
  return call({ op: 'describe_riven_comps', input: {
    comps: comps.map((c) => ({ owner_status: c.owner_status, created_ms: instant(Date.parse(c.created ?? '')) })),
    now_ms: instant(nowMs),
  } });
}
