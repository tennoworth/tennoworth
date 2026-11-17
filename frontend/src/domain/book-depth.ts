import type { MarketItemEntry } from '../contracts/data';

// How the cheapest asks are spread says how a new listing will be picked.
// When the 20 cheapest sit within 10% of the 5 cheapest, buyers face a queue
// at one price and take the cheapest, so a listing above it waits. A slow
// queue was the first idea, but the live book rejected it: of 36 crowded
// books in the 2026-11-17 snapshot, one held more than half a day of trades.
export const CROWDED_MIN_ASKS = 20;
export const CROWDED_SPREAD = 0.1;

type BookFields = Pick<MarketItemEntry, 'low5_avg' | 'low20_avg' | 'sells'>;

export function crowdedBook(m: BookFields | null | undefined): boolean {
  const low5 = Number(m?.low5_avg) || 0;
  const low20 = Number(m?.low20_avg) || 0;
  // low20 is floored at low5 by the pipeline; a lower value is a snapshot
  // from before that fix, and says nothing about the queue.
  if (low5 <= 0 || low20 < low5 || (Number(m?.sells) || 0) < CROWDED_MIN_ASKS) return false;
  return low20 <= low5 * (1 + CROWDED_SPREAD);
}
