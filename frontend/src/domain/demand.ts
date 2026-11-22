// Demand, from DE's own telemetry.
//
// The app's answer to "what's worth selling" has always been circular: spread
// and volume from warframe.market tell you what is being TRADED, not what is
// being WANTED. Two parts at 12p with a 5p spread and single-digit volume look
// identical - and one of them is a piece of a weapon 5% of players run while
// the other belongs to something nobody has equipped since 2019. The first
// sells tonight; the second sits in your listings for a month.
//
// DE publishes per-Mastery-Rank usage share for 694 frames and weapons and has
// done for six years. Native scoring (market_domain::scoring) turns that into
// the liquidity read on every Sell row; this module holds its shapes and the
// Mastery-Rank band the demand cell draws.
//
// TWO HONESTIES the UI must carry:
//   - Usage is measured on the PARENT (the weapon or frame), so a part
//     inherits its parent's figure. That is an approximation, not per-part
//     telemetry.
//   - The data is annual and published in arrears. The year is part of the
//     record so a reader knows how fresh it is.

export interface UsageEntry {
  name: string;
  category: string;
  year: number;
  /** Percentage of that category's total usage. */
  share: number;
  /** Mastery Rank at which this item's usage peaks. */
  peak_mr: number;
  /** Usage share at MR 0 … 33. */
  by_mr: number[];
}

export type Liquidity =
  | 'sells-today' //   played and trading - list at market and it moves
  | 'underpriced' //   played and trading, but priced below its own baseline
  | 'slow' //          barely played; list high and be patient
  | 'thin' //          barely traded, whatever the usage says
  | 'unknown'; //      no usage data for this item or its parent

export interface DemandRead {
  usage: UsageEntry | null;
  inherited: boolean;
  liquidity: Liquidity;
  /** Mastery-Rank band that accounts for most of this item's usage - who is
   *  actually buying it. Null when there is no usage curve. */
  band: { from: number; to: number } | null;
}

/**
 * The Mastery-Rank band holding the middle `coverage` of an item's usage.
 *
 * A single peak MR is a spike on a noisy curve; the band is what a seller can
 * act on ("price this for MR 5–14, not against a whole-population median").
 * Walks outward from the peak until enough mass is covered, which handles the
 * bimodal curves a few weapons genuinely have.
 */
export function masteryBand(byMr: number[], coverage = 0.6): { from: number; to: number } | null {
  const total = byMr.reduce((a, b) => a + b, 0);
  if (!(total > 0)) return null;

  let peak = 0;
  for (let i = 1; i < byMr.length; i += 1) if (byMr[i] > byMr[peak]) peak = i;

  let from = peak;
  let to = peak;
  let acc = byMr[peak];
  while (acc / total < coverage && (from > 0 || to < byMr.length - 1)) {
    const left = from > 0 ? byMr[from - 1] : -1;
    const right = to < byMr.length - 1 ? byMr[to + 1] : -1;
    if (right > left) {
      to += 1;
      acc += byMr[to];
    } else {
      from -= 1;
      acc += byMr[from];
    }
  }
  return { from, to };
}
