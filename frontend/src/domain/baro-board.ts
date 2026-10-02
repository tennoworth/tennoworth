import type { BaroRow } from '../contracts/generated/domain';

export interface DucatBasket {
  /** Ducats the worthwhile items cost, all together. */
  needed: number;
  /** How many items that is. */
  count: number;
  /** How many of them scrapping every spare part would cover, best value
   *  first. */
  coveredByScrapping: number;
  /** What the basket resells for at 90-day baselines. */
  resale: number;
}

/**
 * What his stock costs, and how far your spare parts would go toward it.
 *
 * **There is no ducat balance here, because we cannot see one.** Ducats are
 * account state, not an item, so an inventory scan never observes them - the
 * only ducat figure a scan can derive is what your spare prime parts would
 * yield IF you scrapped them. An earlier version passed that potential in as
 * `ducatsHeld` and treated it as money already banked, then went on to propose
 * scrapping those very same parts to cover the remainder: the same ducats
 * counted twice, and a plan wrong in the user's favour.
 *
 * Rows arrive in rank order from the native Baro calculation.
 *
 * So this takes the potential explicitly and reports COVERAGE, never
 * affordability.
 *
 * "Worthwhile" is flip-or-hold: skip and thin rows are excluded, because
 * sizing the basket with items the board just told the user not to buy would
 * inflate it.
 */
export function ducatBasket(rows: BaroRow[], scrapPotential: number): DucatBasket {
  const basket = rows.filter(
    (r) => (r.verdict === 'flip' || r.verdict === 'hold') && (r.ducats ?? 0) > 0,
  );
  const needed = basket.reduce((sum, r) => sum + (r.ducats ?? 0), 0);

  let spent = 0;
  let coveredByScrapping = 0;
  for (const r of basket) {
    const cost = r.ducats ?? 0;
    if (spent + cost > scrapPotential) break;
    spent += cost;
    coveredByScrapping += 1;
  }
  const resale = basket.reduce((sum, r) => sum + (r.baseline ?? r.price ?? 0), 0);
  return {
    needed,
    count: basket.length,
    coveredByScrapping,
    resale: Math.round(resale),
  };
}

/**
 * Where a visit sits relative to `now`, with the time left in that phase.
 * `now` is a parameter so a view can pass its ticking clock: read inside a
 * `$derived`, `Date.now()` is not tracked, and the countdown froze until the
 * snapshot next changed.
 */
export function baroPhase(
  activation: string | undefined,
  expiry: string | undefined,
  now: number,
): { phase: 'here' | 'incoming' | 'unknown'; windowMs: number | null } {
  const start = activation ? Date.parse(activation) : NaN;
  const end = expiry ? Date.parse(expiry) : NaN;
  if (Number.isFinite(end) && now < end && Number.isFinite(start) && now >= start) {
    return { phase: 'here', windowMs: end - now };
  }
  if (Number.isFinite(start) && now < start) return { phase: 'incoming', windowMs: start - now };
  return { phase: 'unknown', windowMs: null };
}

/**
 * Whether the stock on screen belongs to the visit on screen.
 *
 * The surface can legitimately carry a PAST visit's stock: the old upstream
 * only published inventory during the 48h window, so the pipeline carried it
 * forward. worldState makes that rare, but a snapshot built before the switch
 * - or one carried through a DE outage - can still hit it, and showing last
 * rotation's stock as if it were this one is exactly the sort of confidently
 * wrong output the board exists to avoid.
 */
export function stockIsCurrent(
  inventoryFor: string | undefined,
  activation: string | undefined,
): boolean {
  if (!inventoryFor || !activation) return false;
  return inventoryFor === activation;
}
