import { describe, expect, it } from 'vitest';
import type { MarketItemEntry } from '../contracts/data';
import type { LiveTop } from '../contracts/desktop';
import { findBargains, liveBargain, type Bargain } from './market-bargains';

const entry = (e: Partial<MarketItemEntry>): MarketItemEntry =>
  ({ avg: 0, low_sell: 0, top_buy: 0, vol: 0, median_90d: 0, low5_avg: 0, ...e }) as MarketItemEntry;

const market = (items: Record<string, Partial<MarketItemEntry>>) => ({
  catalog: { 'wolf sledge set': 'wolf_sledge_set' },
  items: Object.fromEntries(Object.entries(items).map(([k, v]) => [k, entry(v)])),
});

describe('findBargains', () => {
  it('finds a bid over the cheapest ask', () => {
    const [b] = findBargains(market({ wolf_sledge_set: { low_sell: 30, top_buy: 40, median_90d: 49, vol: 40 } }));
    expect(b).toEqual({ slug: 'wolf_sledge_set', name: 'Wolf Sledge Set', kind: 'flip', ask: 30, resale: 40, margin: 10, vol: 40 });
  });

  it('ignores a fantasy bid far over the usual price', () => {
    expect(findBargains(market({ core: { low_sell: 999, top_buy: 2000, median_90d: 1000, vol: 40 } }))).toEqual([]);
  });

  it('finds one ask far under a book that agrees on the usual price', () => {
    const [b] = findBargains(market({ strun_prime_blueprint: { low_sell: 14, low5_avg: 23.6, median_90d: 24, vol: 222 } }));
    expect(b).toMatchObject({ kind: 'cheap', ask: 14, resale: 24, margin: 10, name: 'Strun Prime Blueprint' });
  });

  it('leaves a book that has moved down as a whole', () => {
    expect(findBargains(market({ slid: { low_sell: 14, low5_avg: 15, median_90d: 24, vol: 222 } }))).toEqual([]);
  });

  it('needs trades behind a lead', () => {
    expect(findBargains(market({ thin: { low_sell: 30, top_buy: 40, median_90d: 49, vol: 9 } }))).toEqual([]);
  });

  it('ranks by margin and caps the list', () => {
    const rows = findBargains(market({
      a: { low_sell: 30, top_buy: 34, median_90d: 40, vol: 20 },
      b: { low_sell: 30, top_buy: 40, median_90d: 40, vol: 20 },
      c: { low_sell: 30, top_buy: 36, median_90d: 40, vol: 20 },
    }), 2);
    expect(rows.map((r) => r.slug)).toEqual(['b', 'c']);
  });

  it('is empty without a snapshot', () => {
    expect(findBargains(null)).toEqual([]);
  });
});

describe('liveBargain', () => {
  const flip: Bargain = { slug: 's', name: 'S', kind: 'flip', ask: 30, resale: 40, margin: 10, vol: 40 };
  const cheap: Bargain = { ...flip, kind: 'cheap', ask: 14, resale: 24 };
  const quote = (q: Partial<LiveTop>): LiveTop => ({ slug: 's', sells: [], buys: [], low_sell: null, top_buy: null, ...q });

  it('waits for a check', () => {
    expect(liveBargain(flip, undefined)).toBeNull();
  });

  it('confirms a flip while the gap is still there', () => {
    expect(liveBargain(flip, quote({ low_sell: 31, top_buy: 38, low_sell_by: 'Seller' }))).toEqual({ state: 'holds', ask: 31, seller: 'Seller' });
    expect(liveBargain(flip, quote({ low_sell: 36, top_buy: 38 }))).toEqual({ state: 'gone', ask: 36 });
  });

  it('confirms a cheap ask only at the price seen', () => {
    expect(liveBargain(cheap, quote({ low_sell: 14 }))).toMatchObject({ state: 'holds' });
    expect(liveBargain(cheap, quote({ low_sell: 22 }))).toEqual({ state: 'gone', ask: 22 });
    expect(liveBargain(cheap, quote({}))).toEqual({ state: 'gone', ask: null });
  });

  it('says when the lookup failed', () => {
    expect(liveBargain(cheap, quote({ error: 'timeout' }))).toEqual({ state: 'failed' });
  });
});
