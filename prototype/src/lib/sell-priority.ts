// Sell-priority scoring.
//
// The naive "potential plat" column (owned × avg) overstates: it assumes
// every copy you own clears at the average price, instantly. In reality a
// 200p item that sells once a week pays out far less today than a 30p item
// that turns over ten times a day. This module estimates **what you'd
// actually receive per day if you listed everything**, which is the question
// the product positioning ("what to sell right now") wants answered.
//
// Formula:
//   price        = clearingPrice(m)                        // clamped, see below
//   daily_sales  = max(0.05, vol_48h / 2)                  // floor so dead items still rank, just low
//   units_today  = min(owned, daily_sales)                 // can't realise more than the market absorbs
//   sell_score   = units_today × price
//
// `patience` flag = true when vol_48h < 2 — these listings exist for the
// item but it barely moves. The flag is the ONLY mitigation for dead
// items: the volume floor (0.05) deliberately keeps them ranked, so a
// dead item with a sane clearing price can still appear high when you
// own many copies. That's why clearingPrice() must never trust a lone
// aspirational ask (see below) — before the clamp, a vol-1 item with a
// 2,999p fantasy ask ranked #2 of 2,623.

import type { MarketItemEntry } from './types';

type PricedEntry = Pick<
  MarketItemEntry,
  'vol' | 'low_sell' | 'avg' | 'median_now' | 'median_90d'
> | null | undefined;

export interface SellScoreInput {
  owned: number;
  m: PricedEntry;
}

export interface SellScoreOutput {
  sell_score: number;
  patience: boolean;
}

// Below this many closed trades / 48 h the book is too thin to trust its ask
// as a forecast, and too thin to certify a trend. Shared by the ask clamp,
// the patience tag, and the UI's trend badges (via LIQUID_VOL export).
export const LIQUID_VOL = 5;
const PATIENCE_VOL = 3;

// What a listing would realistically clear at. The lowest live ask is the
// honest answer MOST of the time, but it's a single number any account can
// set for free, so it gets sanity-clamped against the closed-trade median:
//  - ask < median/3  → a troll/stale 1p undercut (akbolto receiver: one 1p
//    ask under a stable 38p median cratered its score, killed its peak pill,
//    and fired a "feed it to Baro" deal badge). One such listing absorbs one
//    sale; the realistic price is still the median.
//  - thin item (vol < LIQUID_VOL) with ask > 1.5× median → an aspirational
//    ask nobody pays. The old gate was vol < 2 with a 3× tolerance, which
//    vol-2 items dodged entirely: winding_isles_scene (vol 2, one 100p ask
//    over a 10p median) ranked #7 at "expected 100p/day". On a thin book
//    the closed trades are the evidence; cap at 1.5× median. Liquid items
//    keep their ask — there the book is real information.
// No ask at all → median, then avg, floor 1.
export function clearingPrice(m: PricedEntry): number {
  const lowSell = Number(m?.low_sell) || 0;
  const median = Number(m?.median_now) || Number(m?.median_90d) || 0;
  const avg = Number(m?.avg) || 0;
  const vol = Number(m?.vol) || 0;
  if (lowSell <= 0) return median > 0 ? median : Math.max(1, avg);
  if (median > 0) {
    if (lowSell * 3 < median) return median;
    if (vol < LIQUID_VOL && lowSell > median * 1.5) return median * 1.5;
  }
  return lowSell;
}

export function scoreRow({ owned, m }: SellScoreInput): SellScoreOutput {
  if (!m) return { sell_score: 0, patience: false };
  const vol = Number(m.vol) || 0;
  const price = clearingPrice(m);
  const dailySales = Math.max(0.05, vol / 2);
  const unitsToday = Math.min(owned, dailySales);
  return {
    sell_score: unitsToday * price,
    // Was vol < 2, which vol-2 items dodged while still barely moving.
    patience: vol < PATIENCE_VOL,
  };
}

// Where the current price sits inside its 90-day Donchian band answers the
// timing question the raw sell-score ignores: are you about to sell into a
// trough or a peak? A price pinned near its 90-day low — e.g. a mod Baro just
// flooded, which craters ~50% on arrival and recovers over weeks — is a "hold";
// near its 90-day high is "peak", the moment to list. Neutral in between, or
// whenever the band is missing/degenerate (CSV-only rebuilds inherit zeros).
export type TimingState = 'hold' | 'peak' | 'neutral';

export interface BandSignalInput {
  price: number;
  donchTop?: number | null;
  donchBot?: number | null;
  lowSell?: number | null;
  topBuy?: number | null;
}

const HOLD_BELOW = 0.2;
const PEAK_ABOVE = 0.8;
// A peak must be corroborated by the live book — "peak" means "list NOW",
// and you list into the standing market, not into a price history. Two
// independent corroborations, either suffices:
//  - ask-side: a live ask within 2× of the price (real peaks have asks
//    tracking the trades up; wash-traded "peaks" sit over 1p ask walls —
//    Goopolla printed 36 "sales" at 12p over 209 live asks at 1p);
//  - demand-side: a live top buy offer within 2× of the price. This is the
//    signal a solo seller can't fake cheaply, and it rescues legit peaks
//    whose ask side is polluted by one troll undercut.
// NO live book at all → neutral, fail closed: an uncorroborated peak on a
// no-ask item (neo_t7_relic: median 42, zero asks, top buy 15) was exactly
// the cheapest state to manipulate into existence.
const PEAK_MAX_ASK_GAP = 2;

export function bandSignal({ price, donchTop, donchBot, lowSell, topBuy }: BandSignalInput): TimingState {
  const p = Number(price) || 0;
  const top = Number(donchTop) || 0;
  const bot = Number(donchBot) || 0;
  if (p <= 0 || top <= 0 || bot <= 0 || top <= bot) return 'neutral';
  const pos = (p - bot) / (top - bot); // 0 = at 90d low, 1 = at 90d high
  if (pos <= HOLD_BELOW) return 'hold';
  if (pos >= PEAK_ABOVE) {
    const ask = Number(lowSell) || 0;
    const buy = Number(topBuy) || 0;
    const askCorroborates = ask > 0 && p <= ask * PEAK_MAX_ASK_GAP;
    const buyCorroborates = buy > 0 && p <= buy * PEAK_MAX_ASK_GAP;
    return askCorroborates || buyCorroborates ? 'peak' : 'neutral';
  }
  return 'neutral';
}
