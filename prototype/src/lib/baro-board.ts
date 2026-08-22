// Pricing Baro's manifest.
//
// worldState publishes his stock from the moment the visit is announced —
// days before he lands — with a ducat AND a credit price per line. That turns
// the Baro view from "here is a list" into "here is what your ducats are worth
// this rotation", which is the only question a trader has.
//
// Pure functions on purpose: the ranking is the opinionated part of the
// feature, so it is testable without a DOM.

import type { BaroStock, Market, MarketItemEntry } from './types';

/** What the board says to do about one line of stock. */
export type BaroVerdict =
  | 'flip' //      priced above its own baseline and traded enough to sell into
  | 'hold' //      worth buying, but his arrival depresses it — wait for recovery
  | 'thin' //      a "price" set by one or two optimistic listings
  | 'skip' //      the plat does not justify the ducats
  | 'unpriced'; //  cosmetic or bundle — no market listing exists at all

export interface BaroRow {
  item: string;
  slug?: string;
  /** DE's `/Lotus/...` path. The stable row identity — two lines can share a
   *  display name, and it is also how the row finds its category glyph. */
  unique?: string;
  ducats?: number;
  credits?: number;
  /** Current market price — the depth-aware ask where we have one. */
  price: number | null;
  /** 90-day baseline, for "is today's price actually good". */
  baseline: number | null;
  /** 48h trade volume. */
  vol: number | null;
  /** Plat returned per ducat spent. The board's default sort. */
  platPerDucat: number | null;
  verdict: BaroVerdict;
}

/** Below this many trades in the 48h window, one optimistic listing sets the
 *  "price". Anything thinner is reported as thin rather than ranked on a
 *  number that does not mean anything. */
export const THIN_VOLUME = 5;

/** Plat-per-ducat below which a line is not worth the ducats, given ducats
 *  themselves cost time to farm. Deliberately conservative: the board should
 *  under-promise, and a user who wants the item anyway can still see the row. */
export const SKIP_PLAT_PER_DUCAT = 0.16;

/** How far above its 90-day baseline a price has to sit before "sell into it"
 *  is honest rather than noise. */
const FLIP_PREMIUM = 1.05;

/** Depth-aware current price where the snapshot has one, else the lowest ask.
 *  `low5_avg` averages the ~5 cheapest live asks, so a single troll listing
 *  cannot define the price. */
export function currentPrice(entry: MarketItemEntry | undefined): number | null {
  if (!entry) return null;
  const depth = entry.low5_avg ?? 0;
  if (depth > 0) return depth;
  return entry.low_sell > 0 ? entry.low_sell : null;
}

function baselineOf(entry: MarketItemEntry | undefined): number | null {
  if (!entry) return null;
  const m = entry.median_90d ?? 0;
  return m > 0 ? m : null;
}

/**
 * Decide what to do with one line.
 *
 * The ordering matters: unpriceable beats everything (a cosmetic must never be
 * ranked as if it were free plat), thin beats value judgements (we will not
 * call a flip off two trades), and only then does the plat-per-ducat test run.
 */
export function verdictFor(row: Omit<BaroRow, 'verdict'>): BaroVerdict {
  if (!row.slug || row.price == null) return 'unpriced';
  if (row.vol != null && row.vol < THIN_VOLUME) return 'thin';
  if (row.platPerDucat != null && row.platPerDucat < SKIP_PLAT_PER_DUCAT) return 'skip';
  if (row.baseline != null && row.price >= row.baseline * FLIP_PREMIUM) return 'flip';
  // Priced at or below baseline while he is selling it: his arrival is the
  // reason. The profit is in holding for the recovery, not in reselling today.
  return 'hold';
}

/**
 * Price Baro's manifest against the snapshot.
 *
 * Rows keep their manifest order until sorted, and every unpriceable row is
 * kept — dropping a cosmetic would read as "he isn't selling it".
 */
export function priceManifest(stock: BaroStock[], market: Market | null): BaroRow[] {
  return stock.map((s) => {
    const entry = s.slug ? market?.items?.[s.slug] : undefined;
    const price = currentPrice(entry);
    const baseline = baselineOf(entry);
    const vol = entry ? entry.vol : null;
    const platPerDucat = price != null && s.ducats ? price / s.ducats : null;
    const partial = {
      item: s.item,
      slug: s.slug,
      unique: s.unique,
      ducats: s.ducats,
      credits: s.credits,
      price,
      baseline,
      vol,
      platPerDucat,
    };
    return { ...partial, verdict: verdictFor(partial) };
  });
}

/** Sort by plat-per-ducat, best first. Unpriceable rows sink to the bottom
 *  rather than sorting as zero, so they never displace a real offer. */
export function byPlatPerDucat(rows: BaroRow[]): BaroRow[] {
  return [...rows].sort((a, b) => {
    const av = a.platPerDucat ?? -1;
    const bv = b.platPerDucat ?? -1;
    if (av !== bv) return bv - av;
    return a.item.localeCompare(b.item);
  });
}

export interface DucatGap {
  /** Ducats needed for everything worth buying. */
  needed: number;
  /** How many rows the held ducats already cover, best-value first. */
  affordable: number;
  /** Shortfall against the whole worthwhile basket, 0 when it is covered. */
  short: number;
  /** What the worthwhile basket resells for at its 90-day baseline. */
  resale: number;
}

/**
 * What the current ducat balance actually buys this rotation.
 *
 * "Worthwhile" is flip-or-hold: skip and thin rows are excluded from the
 * basket because recommending them would inflate the shortfall with things the
 * board just told the user not to buy.
 */
export function ducatGap(rows: BaroRow[], ducatsHeld: number): DucatGap {
  const basket = byPlatPerDucat(rows).filter(
    (r) => (r.verdict === 'flip' || r.verdict === 'hold') && (r.ducats ?? 0) > 0,
  );
  const needed = basket.reduce((sum, r) => sum + (r.ducats ?? 0), 0);

  let spent = 0;
  let affordable = 0;
  for (const r of basket) {
    const cost = r.ducats ?? 0;
    if (spent + cost > ducatsHeld) break;
    spent += cost;
    affordable += 1;
  }
  const resale = basket.reduce((sum, r) => sum + (r.baseline ?? r.price ?? 0), 0);
  return {
    needed,
    affordable,
    short: Math.max(0, needed - ducatsHeld),
    resale: Math.round(resale),
  };
}

/** Where a visit sits relative to now. `unknown` only when no schedule
 *  exists at all — with worldState there always is one. */
export type BaroPhase = 'here' | 'incoming' | 'gone' | 'unknown';

export function baroPhase(
  activation: string | undefined,
  expiry: string | undefined,
  now: number,
): { phase: BaroPhase; windowMs: number | null } {
  const start = activation ? Date.parse(activation) : NaN;
  const end = expiry ? Date.parse(expiry) : NaN;
  if (!Number.isFinite(start) || !Number.isFinite(end)) {
    return { phase: 'unknown', windowMs: null };
  }
  if (now < start) return { phase: 'incoming', windowMs: start - now };
  if (now < end) return { phase: 'here', windowMs: end - now };
  return { phase: 'gone', windowMs: null };
}

/**
 * Whether the stock on screen belongs to the visit on screen.
 *
 * The surface can legitimately carry a PAST visit's stock: the old upstream
 * only published inventory during the 48h window, so the pipeline carried it
 * forward. worldState makes that rare, but a snapshot built before the switch
 * — or one carried through a DE outage — can still hit it, and showing last
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
