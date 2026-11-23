// The board's basket and visit phase are native (market_domain::baro), like
// the board itself; this file is their call shape.
import type { BaroRow } from '../contracts/generated/domain';
import type { DucatBasket } from '../contracts/generated/web';
import { call, jsNumber } from './native';

/**
 * What his stock costs, and how far your spare parts would go toward it -
 * coverage, never affordability: a scan cannot see a ducat balance
 * (market_domain::baro::ducat_basket says why). Rows arrive in rank order
 * from the native Baro calculation.
 */
export function ducatBasket(rows: BaroRow[], scrapPotential: number): DucatBasket {
  return call({ op: 'ducat_basket', input: {
    rows: rows.map((r) => ({ verdict: r.verdict, ducats: r.ducats ?? null, baseline: r.baseline ?? null, price: r.price ?? null })),
    scrap_potential: jsNumber(scrapPotential),
  } });
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
  return call({ op: 'baro_phase', input: {
    activation_ms: jsNumber(activation ? Date.parse(activation) : NaN),
    expiry_ms: jsNumber(expiry ? Date.parse(expiry) : NaN),
    now_ms: jsNumber(now),
  } });
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
