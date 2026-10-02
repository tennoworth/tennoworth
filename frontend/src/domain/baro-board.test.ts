import { describe, expect, it } from 'vitest';
import { baroPhase, ducatBasket, stockIsCurrent } from './baro-board';
import type { BaroRow } from '../contracts/generated/domain';

describe('ducatBasket', () => {
  const rows = [
    { item: 'A', ducats: 350, platPerDucat: 0.41, baseline: 128, verdict: 'flip' },
    { item: 'B', ducats: 500, platPerDucat: 0.34, baseline: 151, verdict: 'flip' },
    { item: 'C', ducats: 375, platPerDucat: 0.26, baseline: 104, verdict: 'hold' },
    { item: 'D', ducats: 400, platPerDucat: 0.18, baseline: 75, verdict: 'thin' },
  ] as BaroRow[];

  it('counts only what the board actually recommends buying', () => {
    // D is thin and must not inflate the basket.
    expect(ducatBasket(rows, 0).needed).toBe(1225);
    expect(ducatBasket(rows, 0).count).toBe(3);
  });

  it('reports coverage from scrapping, best value first', () => {
    const b = ducatBasket(rows, 900);
    expect(b.coveredByScrapping).toBe(2);
  });

  it('reports full coverage when the spares would pay for everything', () => {
    const b = ducatBasket(rows, 2000);
    expect(b.coveredByScrapping).toBe(3);
    expect(b.resale).toBe(383);
  });

  it('exposes no notion of a balance we cannot observe', () => {
    // Ducats are account state, never visible to an inventory scan. If an
    // "affordable" or "short" field reappears, something is pretending to
    // know a balance again - and the scrap planner will double-count.
    expect(Object.keys(ducatBasket(rows, 500)).sort()).toEqual([
      'count',
      'coveredByScrapping',
      'needed',
      'resale',
    ]);
  });
});

describe('baroPhase', () => {
  const activation = '2026-08-21T13:00:00Z';
  const expiry = '2026-08-23T13:00:00Z';
  const start = Date.parse(activation);
  const end = Date.parse(expiry);

  it('counts down to an announced arrival and follows the clock it is given', () => {
    expect(baroPhase(activation, expiry, start - 3_600_000)).toEqual({ phase: 'incoming', windowMs: 3_600_000 });
    expect(baroPhase(activation, expiry, start - 60_000)).toEqual({ phase: 'incoming', windowMs: 60_000 });
  });

  it('turns to here at arrival and counts down to departure', () => {
    expect(baroPhase(activation, expiry, start)).toEqual({ phase: 'here', windowMs: end - start });
    expect(baroPhase(activation, expiry, end - 7_200_000)).toEqual({ phase: 'here', windowMs: 7_200_000 });
  });

  it('has no countdown once he has left or without a schedule', () => {
    expect(baroPhase(activation, expiry, end)).toEqual({ phase: 'unknown', windowMs: null });
    expect(baroPhase(undefined, undefined, start)).toEqual({ phase: 'unknown', windowMs: null });
  });
});

describe('stockIsCurrent', () => {
  it('rejects stock carried over from a previous visit', () => {
    expect(stockIsCurrent('2026-08-07T13:00:00Z', '2026-08-21T13:00:00Z')).toBe(false);
  });

  it('accepts stock captured during the visit on screen', () => {
    expect(stockIsCurrent('2026-08-21T13:00:00Z', '2026-08-21T13:00:00Z')).toBe(true);
  });

  it('treats a missing stamp as not current', () => {
    expect(stockIsCurrent(undefined, '2026-08-21T13:00:00Z')).toBe(false);
  });
});
