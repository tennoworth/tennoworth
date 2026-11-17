import { describe, expect, it } from 'vitest';
import { crowdedBook } from './book-depth';

describe('crowdedBook', () => {
  it('flags twenty asks within a tenth of the cheapest five', () => {
    expect(crowdedBook({ low5_avg: 69, low20_avg: 70, sells: 50 })).toBe(true);
    expect(crowdedBook({ low5_avg: 50, low20_avg: 55, sells: 20 })).toBe(true);
  });

  it('leaves a steep book alone', () => {
    expect(crowdedBook({ low5_avg: 50, low20_avg: 55.5, sells: 40 })).toBe(false);
  });

  it('needs twenty asks to call it a queue', () => {
    expect(crowdedBook({ low5_avg: 50, low20_avg: 50, sells: 19 })).toBe(false);
  });

  it('says nothing without the figures', () => {
    expect(crowdedBook({ low5_avg: 50, low20_avg: 0, sells: 40 })).toBe(false);
    expect(crowdedBook({ low5_avg: 0, low20_avg: 0, sells: 40 })).toBe(false);
    expect(crowdedBook({ low5_avg: 26.4, low20_avg: 21.5, sells: 22 })).toBe(false);
    expect(crowdedBook(undefined)).toBe(false);
  });
});
