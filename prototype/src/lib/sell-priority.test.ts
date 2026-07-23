// @ts-nocheck — vitest runs these as JS-style fixtures; full TS shapes here would be busy-work without catching real bugs.
import { describe, it, expect } from 'vitest';
import { scoreRow, bandSignal, clearingPrice, sellableQty, selectPicks, MIN_PICK_SCORE, MAX_PICKS } from './sell-priority.js';

describe('scoreRow', () => {
  it('returns zero score with no market data', () => {
    expect(scoreRow({ owned: 5, m: null })).toEqual({ sell_score: 0, patience: false });
  });

  it('uses low_sell as the clearing price when available', () => {
    const r = scoreRow({ owned: 10, m: { low_sell: 30, avg: 45, vol: 20 } });
    // dailySales = 20/2 = 10, capped by owned=10 → unitsToday = 10
    // score = 10 × 30 = 300
    expect(r.sell_score).toBe(300);
    expect(r.patience).toBe(false);
  });

  it('falls back to avg when low_sell missing', () => {
    const r = scoreRow({ owned: 1, m: { low_sell: 0, avg: 50, vol: 10 } });
    // dailySales = 5, capped by owned=1 → 1 × 50 = 50
    expect(r.sell_score).toBe(50);
  });

  it('caps units at the market absorption rate, not at what you own', () => {
    // owned 100, vol 4 → dailySales = 2, score = 2 × 100p = 200
    const r = scoreRow({ owned: 100, m: { low_sell: 100, avg: 110, vol: 4 } });
    expect(r.sell_score).toBe(200);
  });

  it('caps at what you own when owned is the bottleneck', () => {
    // owned 1, vol 100 → dailySales = 50, capped by owned=1 → 1 × 20 = 20
    const r = scoreRow({ owned: 1, m: { low_sell: 20, avg: 25, vol: 100 } });
    expect(r.sell_score).toBe(20);
  });

  it('flags low-volume items as patience', () => {
    expect(scoreRow({ owned: 5, m: { low_sell: 200, avg: 220, vol: 1 } }).patience).toBe(true);
    expect(scoreRow({ owned: 5, m: { low_sell: 200, avg: 220, vol: 0 } }).patience).toBe(true);
    // vol 2 used to dodge the tag while still barely moving (winding_isles).
    expect(scoreRow({ owned: 5, m: { low_sell: 200, avg: 220, vol: 2 } }).patience).toBe(true);
    expect(scoreRow({ owned: 5, m: { low_sell: 200, avg: 220, vol: 3 } }).patience).toBe(false);
  });

  it('does not zero out completely dead items — they should still rank, just very low', () => {
    const r = scoreRow({ owned: 1, m: { low_sell: 100, avg: 100, vol: 0 } });
    // dailySales floor of 0.05 → 0.05 × 100 = 5
    expect(r.sell_score).toBeCloseTo(5);
    expect(r.patience).toBe(true);
  });

  it('handles missing fields defensively', () => {
    expect(scoreRow({ owned: 0, m: {} })).toEqual({ sell_score: 0, patience: true });
  });

  it('does not let a dead item with a fantasy ask top the sort', () => {
    // corpus_void_key: vol 1, lone 2,999p ask, real trades ~200p. Raw
    // low_sell scored it 1499.5 — rank #2 of 2,623 in the live snapshot.
    const r = scoreRow({ owned: 3, m: { low_sell: 2999, avg: 204, vol: 1, median_90d: 200 } });
    // dailySales = 0.5 → 0.5 × clamped(200 × 1.5) = 150, not 1499.5
    expect(r.sell_score).toBeCloseTo(150);
  });

  it('does not let a vol-2 item with a 10× ask dodge the clamp (winding isles)', () => {
    // winding_isles_scene: vol 2, one live 100p ask over a 10p median —
    // ranked #7 at "expected 100p/day" because the old gate was vol < 2.
    const r = scoreRow({ owned: 1, m: { low_sell: 100, avg: 10, vol: 2, median_now: 10 } });
    // clearing = 10 × 1.5 = 15, units = min(1, 1) → 15, not 100
    expect(r.sell_score).toBeCloseTo(15);
    expect(r.patience).toBe(true);
  });
});

describe('clearingPrice', () => {
  it('uses the live ask when it agrees with the median', () => {
    expect(clearingPrice({ low_sell: 60, median_now: 65, avg: 62, vol: 20 })).toBe(60);
  });

  it('clamps a troll undercut up to the median (akbolto receiver case)', () => {
    // One 1p ask under a stable 38p median: one such listing absorbs one
    // sale; everything else clears near the median.
    expect(clearingPrice({ low_sell: 1, median_now: 38, avg: 30, vol: 54 })).toBe(38);
  });

  it('clamps a thin item’s aspirational ask down to 1.5× median', () => {
    expect(clearingPrice({ low_sell: 2999, median_now: 204, avg: 204, vol: 1 })).toBe(306);
    // vol 2–4 books get the same treatment — they used to dodge the vol<2 gate
    expect(clearingPrice({ low_sell: 100, median_now: 10, avg: 10, vol: 2 })).toBe(15);
    expect(clearingPrice({ low_sell: 100, median_now: 10, avg: 10, vol: 4 })).toBe(15);
  });

  it('keeps a thin item’s ask when it is within 1.5× of the median', () => {
    expect(clearingPrice({ low_sell: 14, median_now: 10, avg: 11, vol: 2 })).toBe(14);
  });

  it('keeps a liquid item’s high ask — there the book is real information', () => {
    expect(clearingPrice({ low_sell: 700, median_now: 200, avg: 250, vol: 30 })).toBe(700);
    // LIQUID_VOL boundary: vol 5 counts as liquid
    expect(clearingPrice({ low_sell: 100, median_now: 10, avg: 10, vol: 5 })).toBe(100);
  });

  it('falls back median → avg → 1 when there is no ask', () => {
    expect(clearingPrice({ low_sell: 0, median_now: 42, avg: 50, vol: 5 })).toBe(42);
    expect(clearingPrice({ low_sell: 0, median_now: 0, avg: 50, vol: 5 })).toBe(50);
    expect(clearingPrice({ low_sell: 0, median_now: 0, avg: 0, vol: 5 })).toBe(1);
  });

  it('uses median_90d when median_now is absent (pre-split snapshots)', () => {
    expect(clearingPrice({ low_sell: 1, median_90d: 38, avg: 30, vol: 54 })).toBe(38);
  });
});

describe('sellableQty', () => {
  it('passes owned count through untouched when reserve is 0', () => {
    expect(sellableQty(5, 0)).toBe(5);
    expect(sellableQty(0, 0)).toBe(0);
  });

  it('subtracts the reserve from owned count', () => {
    expect(sellableQty(5, 1)).toBe(4);
    expect(sellableQty(5, 5)).toBe(0);
  });

  it('clamps at 0 rather than going negative when reserve exceeds owned', () => {
    expect(sellableQty(2, 5)).toBe(0);
    expect(sellableQty(0, 3)).toBe(0);
  });

  it('is backward-equivalent to the old 2-arg signature when leveled is 0 (default)', () => {
    expect(sellableQty(5, 1)).toBe(sellableQty(5, 1, 0));
    expect(sellableQty(2, 5)).toBe(sellableQty(2, 5, 0));
  });

  it('leveled copies satisfy the reserve when leveled > reserve', () => {
    // 5 owned, 2 leveled (untradeable), reserve only asks for 1 kept back —
    // the leveled copies already cover that, so sellable = 5 - 2 = 3.
    expect(sellableQty(5, 1, 2)).toBe(3);
  });

  it('the reserve holds back more than the leveled count when reserve > leveled', () => {
    // 5 owned, 1 leveled, but the user wants 3 kept back — reserve wins.
    expect(sellableQty(5, 3, 1)).toBe(2);
  });

  it('holds back everything when all owned copies are leveled', () => {
    expect(sellableQty(4, 0, 4)).toBe(0);
  });

  it('clamps at 0 rather than going negative when leveled exceeds owned', () => {
    expect(sellableQty(3, 0, 5)).toBe(0);
  });
});

describe('bandSignal', () => {
  it('flags a price near the 90d low as hold (e.g. a Baro-flooded mod)', () => {
    // Primed Reach post-Baro: trough ~28 in a 25–66 band → bottom of range
    expect(bandSignal({ price: 28, donchBot: 25, donchTop: 66 })).toBe('hold');
  });

  it('flags a corroborated price near the 90d high as peak', () => {
    expect(bandSignal({ price: 64, donchBot: 25, donchTop: 66, lowSell: 60 })).toBe('peak');
  });

  it('returns neutral in the middle of the band', () => {
    expect(bandSignal({ price: 45, donchBot: 25, donchTop: 66 })).toBe('neutral');
  });

  it('is neutral when the band is degenerate (top === bot)', () => {
    expect(bandSignal({ price: 50, donchBot: 50, donchTop: 50 })).toBe('neutral');
  });

  it('is neutral when band data is missing or zero (CSV-only rebuilds)', () => {
    expect(bandSignal({ price: 50, donchBot: 0, donchTop: 0 })).toBe('neutral');
    expect(bandSignal({ price: 50 })).toBe('neutral');
    expect(bandSignal({ price: 0, donchBot: 25, donchTop: 66 })).toBe('neutral');
  });

  it('clamps prices outside the band rather than throwing', () => {
    expect(bandSignal({ price: 10, donchBot: 25, donchTop: 66 })).toBe('hold');
    expect(bandSignal({ price: 90, donchBot: 25, donchTop: 66, lowSell: 85 })).toBe('peak');
  });

  it('suppresses a peak the live book contradicts (wash-traded median)', () => {
    // Goopolla: 36 "trades" at 12p while 209 live asks sit at 1p — the peak
    // price is unrealizable, so "list now" would be wrong twice over.
    expect(bandSignal({ price: 12, donchBot: 1, donchTop: 12, lowSell: 1 })).toBe('neutral');
  });

  it('keeps a peak when the live ask corroborates it', () => {
    expect(bandSignal({ price: 64, donchBot: 25, donchTop: 66, lowSell: 60 })).toBe('peak');
    // a normal undercut spread is nowhere near the 2× gap threshold
    expect(bandSignal({ price: 64, donchBot: 25, donchTop: 66, lowSell: 40 })).toBe('peak');
  });

  it('fails CLOSED when there is no live book at all', () => {
    // neo_t7_relic: median 42 at the band top, zero live asks, top buy 15.
    // An uncorroborated peak on a no-ask item is the cheapest state to
    // manipulate into existence — never render "list now" from history alone.
    expect(bandSignal({ price: 64, donchBot: 25, donchTop: 66, lowSell: 0 })).toBe('neutral');
    expect(bandSignal({ price: 64, donchBot: 25, donchTop: 66 })).toBe('neutral');
    expect(bandSignal({ price: 42, donchBot: 3, donchTop: 42, lowSell: 0, topBuy: 15 })).toBe('neutral');
  });

  it('lets real demand corroborate a peak when the ask side is polluted', () => {
    // akbolto receiver: stable 38p median at its high, one 1p troll ask —
    // but a live buy offer near the price proves the peak is real.
    expect(bandSignal({ price: 38, donchBot: 20, donchTop: 40, lowSell: 1, topBuy: 30 })).toBe('peak');
    // ...while a wash-traded fish has no demand side to fake (Goopolla:
    // fake 12p median over 1p asks, zero buyers) → stays suppressed.
    expect(bandSignal({ price: 12, donchBot: 1, donchTop: 12, lowSell: 1, topBuy: 0 })).toBe('neutral');
  });

  it('does not let the ask gap interfere with hold', () => {
    expect(bandSignal({ price: 28, donchBot: 25, donchTop: 66, lowSell: 1 })).toBe('hold');
  });
});

describe('selectPicks', () => {
  // Minimal row shape — real callers pass the full `results` row (name,
  // slug, timing, clearing_price, …); selectPicks only reads these three.
  function row(overrides = {}) {
    return { sellable: 5, patience: false, sell_score: 50, ...overrides };
  }

  it('returns nothing for empty input', () => {
    expect(selectPicks([])).toEqual([]);
  });

  it('excludes rows below the score floor', () => {
    const rows = [row({ sell_score: 5 }), row({ sell_score: 19.9 })];
    expect(selectPicks(rows, { minScore: 20 })).toEqual([]);
    // default floor (MIN_PICK_SCORE) behaves the same without an explicit option
    expect(selectPicks(rows)).toEqual([]);
  });

  it('excludes patience rows even when the score is high', () => {
    const rows = [row({ sell_score: 500, patience: true })];
    expect(selectPicks(rows)).toEqual([]);
  });

  it('excludes rows with nothing sellable', () => {
    const rows = [row({ sell_score: 500, sellable: 0 })];
    expect(selectPicks(rows)).toEqual([]);
  });

  it('respects an explicit cap', () => {
    const rows = Array.from({ length: 10 }, (_, i) => row({ sell_score: 100 - i, key: i }));
    const picks = selectPicks(rows, { cap: 3 });
    expect(picks).toHaveLength(3);
    expect(picks.map((r) => r.key)).toEqual([0, 1, 2]);
  });

  it('defaults the cap to MAX_PICKS', () => {
    const rows = Array.from({ length: MAX_PICKS + 3 }, () => row());
    expect(selectPicks(rows)).toHaveLength(MAX_PICKS);
  });

  it('defaults the score floor to MIN_PICK_SCORE', () => {
    const rows = [row({ sell_score: MIN_PICK_SCORE - 0.01 }), row({ sell_score: MIN_PICK_SCORE })];
    expect(selectPicks(rows)).toEqual([row({ sell_score: MIN_PICK_SCORE })]);
  });

  it('slices in the given order instead of re-sorting — trusts the caller pre-sorted', () => {
    // Deliberately out of score order. If selectPicks ever re-sorted, this
    // would come back ['high', 'low'] and mask a caller regression instead
    // of surfacing it.
    const rows = [row({ sell_score: 10, key: 'low' }), row({ sell_score: 90, key: 'high' })];
    const picks = selectPicks(rows, { minScore: 1 });
    expect(picks.map((r) => r.key)).toEqual(['low', 'high']);
  });
});
