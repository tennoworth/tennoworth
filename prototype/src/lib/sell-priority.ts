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
//   price        = low_sell when > 0 else max(1, avg)     // realistic clearing price
//   daily_sales  = max(0.05, vol_48h / 2)                  // floor so dead items still rank, just low
//   units_today  = min(owned, daily_sales)                 // can't realise more than the market absorbs
//   sell_score   = units_today × price
//
// `patience` flag = true when vol_48h < 2 — these listings exist for the
// item but it barely moves. The UI uses the flag to draw a hint; sorting
// by sell_score already pushes them down naturally.

import type { MarketItemEntry } from './types';

export interface SellScoreInput {
  owned: number;
  m: Pick<MarketItemEntry, 'vol' | 'low_sell' | 'avg'> | null | undefined;
}

export interface SellScoreOutput {
  sell_score: number;
  patience: boolean;
}

export function scoreRow({ owned, m }: SellScoreInput): SellScoreOutput {
  if (!m) return { sell_score: 0, patience: false };
  const vol = Number(m.vol) || 0;
  const lowSell = Number(m.low_sell) || 0;
  const avg = Number(m.avg) || 0;
  const price = lowSell > 0 ? lowSell : Math.max(1, avg);
  const dailySales = Math.max(0.05, vol / 2);
  const unitsToday = Math.min(owned, dailySales);
  return {
    sell_score: unitsToday * price,
    patience: vol < 2,
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
}

const HOLD_BELOW = 0.2;
const PEAK_ABOVE = 0.8;
// A peak must be corroborated by the live book: real trades clear near the
// standing ask, so when the closed-trade median prints at more than ~2× the
// lowest live ask, the "peak" price isn't realizable — either wash trades
// (Goopolla: 36 "sales" at 12p over 209 live asks at 1p) or a price that
// already crashed since those trades closed. Either way "list now" is wrong.
const PEAK_MAX_ASK_GAP = 2;

export function bandSignal({ price, donchTop, donchBot, lowSell }: BandSignalInput): TimingState {
  const p = Number(price) || 0;
  const top = Number(donchTop) || 0;
  const bot = Number(donchBot) || 0;
  if (p <= 0 || top <= 0 || bot <= 0 || top <= bot) return 'neutral';
  const pos = (p - bot) / (top - bot); // 0 = at 90d low, 1 = at 90d high
  if (pos <= HOLD_BELOW) return 'hold';
  if (pos >= PEAK_ABOVE) {
    const ask = Number(lowSell) || 0;
    if (ask > 0 && p > ask * PEAK_MAX_ASK_GAP) return 'neutral';
    return 'peak';
  }
  return 'neutral';
}
