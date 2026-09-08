import { describe, it, expect } from 'vitest';
import { selectDrifted, DRIFT_PCT, MIN_DRIFT_PLAT } from './order-drift';
import type { DriftInput } from './order-drift';

/** A liquid book whose clearing price is `price`: vol clears LIQUID_VOL and
 *  the median agrees with the ask, so clearingPrice returns the ask unclamped. */
function liquid(price: number, vol = 20) {
  return { vol, low_sell: price, avg: price, median_now: price, median_90d: price };
}

function order(over: Partial<DriftInput> = {}): DriftInput {
  return {
    id: 'o1',
    slug: 'mag_prime_set',
    name: 'Mag Prime Set',
    platinum: 100,
    type: 'sell',
    m: liquid(100),
    ...over,
  };
}

describe('selectDrifted', () => {
  it('says nothing when the ask still matches the market', () => {
    expect(selectDrifted([order()])).toEqual([]);
  });

  it('flags an ask stranded above the market as overpriced', () => {
    const rows = selectDrifted([order({ platinum: 100, m: liquid(60) })]);
    expect(rows).toHaveLength(1);
    expect(rows[0].kind).toBe('overpriced');
    expect(rows[0].listed).toBe(100);
    expect(rows[0].suggested).toBe(60);
    expect(rows[0].delta).toBe(-40);
    expect(rows[0].delta_pct).toBeCloseTo(0.4);
  });

  it('flags an ask sitting under the market as underpriced', () => {
    const rows = selectDrifted([order({ platinum: 50, m: liquid(100) })]);
    expect(rows).toHaveLength(1);
    expect(rows[0].kind).toBe('underpriced');
    expect(rows[0].suggested).toBe(100);
    expect(rows[0].delta).toBe(50);
  });

  it('ignores moves under the percentage floor', () => {
    // 100 -> 110 is 10%, below DRIFT_PCT, even though it clears MIN_DRIFT_PLAT.
    expect(DRIFT_PCT).toBe(0.2);
    expect(selectDrifted([order({ platinum: 100, m: liquid(110) })])).toEqual([]);
  });

  it('ignores moves under the absolute plat floor even when the percentage is large', () => {
    // 5 -> 7 is 40% but only 2p: percentage alone would surface pure churn.
    expect(MIN_DRIFT_PLAT).toBe(3);
    expect(selectDrifted([order({ platinum: 5, m: liquid(7) })])).toEqual([]);
  });

  it('skips buy orders rather than inverting them', () => {
    // The reference price describes what a SELLER clears at; reusing it for a
    // buy order would be confidently wrong.
    const rows = selectDrifted([order({ type: 'buy', platinum: 100, m: liquid(60) })]);
    expect(rows).toEqual([]);
  });

  it('does not chase a lone 1p undercut', () => {
    // clearingPrice clamps a troll ask back to the median, so a 40p listing is
    // not told to drop to 1p. This is the whole reason drift reuses it.
    const rows = selectDrifted([
      order({ platinum: 40, m: { vol: 20, low_sell: 1, avg: 38, median_now: 38, median_90d: 38 } }),
    ]);
    expect(rows).toEqual([]);
  });

  it('marks a thin book without hiding it', () => {
    const rows = selectDrifted([
      order({ platinum: 100, m: { vol: 1, low_sell: 60, avg: 60, median_now: 60, median_90d: 60 } }),
    ]);
    expect(rows).toHaveLength(1);
    expect(rows[0].thin).toBe(true);
  });

  it('sorts by the size of the plat swing, not the percentage', () => {
    const rows = selectDrifted([
      order({ id: 'small', platinum: 10, m: liquid(20) }), // +10p, 100%
      order({ id: 'big', platinum: 400, m: liquid(200) }), // -200p, 50%
    ]);
    expect(rows.map((r) => r.id)).toEqual(['big', 'small']);
  });

  it('skips orders with no market entry or a nonsense price', () => {
    expect(selectDrifted([order({ m: null })])).toEqual([]);
    expect(selectDrifted([order({ platinum: 0 })])).toEqual([]);
  });

  it('honours the limit', () => {
    const many = Array.from({ length: 10 }, (_, i) =>
      order({ id: `o${i}`, platinum: 100, m: liquid(50) }),
    );
    expect(selectDrifted(many, 3)).toHaveLength(3);
  });

  it('is empty for an empty book', () => {
    expect(selectDrifted([])).toEqual([]);
  });
});
