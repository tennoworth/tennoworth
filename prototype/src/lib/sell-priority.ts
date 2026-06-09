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
