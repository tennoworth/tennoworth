// @ts-nocheck
import { describe, it, expect } from 'vitest';
import { totals, since, soldByItem, describeItems, outcomeRows, outcomeHeadline, topSellers, weeklySales, medianWeek } from './ledger.js';

const t = (kind, plat, at, items) => ({ id: at, at, partner: 'P', kind, plat, items, log_stamp: null, wfm_closed: false });
const g = (name, qty = 1) => ({ name, qty, direction: 'given' });
const r = (name, qty = 1) => ({ name, qty, direction: 'received' });

describe('ledger', () => {
  const trades = [
    t('sale', 40, 1000, [g('Primed Flow')]),
    t('sale', 30, 2000, [g('Lith C5 Relic', 3)]),
    t('purchase', 25, 3000, [r('Ash Prime Blueprint')]),
    t('trade', 0, 4000, [g('A'), r('B')]),
    t('sale', 90, 5000, [g('Primed Flow'), g('Lith C5 Relic', 2)]),
  ];

  it('totals sales/purchases plat and net', () => {
    expect(totals(trades)).toEqual({ sales: 3, purchases: 1, platIn: 160, platOut: 25, net: 135 });
    expect(totals([])).toEqual({ sales: 0, purchases: 0, platIn: 0, platOut: 0, net: 0 });
  });

  it('since() slices by age', () => {
    expect(since(trades, 1, 4000 + 86400).map((x) => x.at)).toEqual([4000, 5000]);
  });

  it('soldByItem attributes plat per item, splitting multi-item sales by quantity', () => {
    // sale 3: 90p over 1 Primed Flow + 2 relics = 30p/unit → PF 30, relics 60
    expect(soldByItem(trades)).toEqual([
      { name: 'Lith C5 Relic', qty: 5, plat: 90, trades: 2 },
      { name: 'Primed Flow', qty: 2, plat: 70, trades: 2 },
    ]);
  });

  it('describeItems shows the side that matters', () => {
    expect(describeItems(t('sale', 1, 1, [g('Primed Flow'), g('Lith C5 Relic', 3)]))).toBe('Primed Flow, Lith C5 Relic ×3');
    expect(describeItems(t('purchase', 1, 1, [r('Ash Prime Blueprint', 2)]))).toBe('Ash Prime Blueprint ×2');
    expect(describeItems(t('trade', 0, 1, [g('A'), r('B')]))).toBe('A');
    expect(describeItems(t('sale', 0, 1, []))).toBe('-');
  });
});

describe('listing outcomes', () => {
  const none = { listings: 0, quantity_listed: 0, quantity_sold: 0, within_day: { eligible: 0, sold: 0 }, within_week: { eligible: 0, sold: 0 }, median_hours_to_first_sale: null, price_ratio: null, priced_quantity: 0 };
  const o = (session, manual = none, enabled = true) => ({ enabled, window_days: 30, session, manual });

  it('shows unknowns as a dash, never as zero', () => {
    const [row] = outcomeRows(o({ ...none, listings: 2, quantity_listed: 2 }));
    expect(row).toMatchObject({ label: 'Trade Session', sold: '0 of 2', day: '-', week: '-', wait: '-', price: '-' });
  });

  it('formats waits in hours, then days', () => {
    const wait = (h) => outcomeRows(o({ ...none, listings: 1, median_hours_to_first_sale: h }))[0].wait;
    expect([wait(0.4), wait(5.2), wait(47.6), wait(61)]).toEqual(['under 1 h', '5 h', '48 h', '2.5 d']);
  });

  it('says when nothing is old enough to judge, or when it is off', () => {
    expect(outcomeHeadline(o({ ...none, listings: 1 }))).toBe('1 listing in 30 days · too recent to judge');
    expect(outcomeHeadline(o(none))).toBe('none in the last 30 days');
    expect(outcomeHeadline(o(none, none, false))).toBe('off');
  });

  it('meters a share only once something is old enough to judge', () => {
    const [row] = outcomeRows(o({ ...none, listings: 3, quantity_listed: 5, quantity_sold: 2, within_day: { eligible: 0, sold: 0 }, within_week: { eligible: 2, sold: 1 } }));
    expect(row.meters).toEqual([
      { label: 'Copies sold', value: 2, max: 5, text: '2 of 5' },
      { label: 'Sold in a day', value: 0, max: 0, text: 'too recent' },
      { label: 'Sold in a week', value: 1, max: 2, text: '1 of 2' },
    ]);
  });
});

describe('ledger charts', () => {
  // Built from local dates so the expectations hold in any time zone.
  const at = (y, m, d, h = 12) => new Date(y, m - 1, d, h).getTime() / 1000;
  const sale = (plat, secs) => t('sale', plat, secs, [g('X')]);

  it('buckets sales into local Monday weeks, oldest first, ending with the running week', () => {
    // Wednesday 2026-10-28; its week starts Monday 2026-10-26.
    const now = at(2026, 10, 28) * 1000;
    const weeks = weeklySales([
      sale(50, at(2026, 10, 26, 0)),
      sale(20, at(2026, 10, 25, 23)),
      sale(7, at(2026, 10, 19)),
      t('purchase', 99, at(2026, 10, 27), [r('Y')]),
      sale(1000, at(2026, 9, 1)),
    ], 3, now);
    expect(weeks.map((w) => [new Date(w.start).toDateString(), w.plat, w.sales, w.partial])).toEqual([
      [new Date(2026, 9, 12).toDateString(), 0, 0, false],
      [new Date(2026, 9, 19).toDateString(), 27, 2, false],
      [new Date(2026, 9, 26).toDateString(), 50, 1, true],
    ]);
  });

  it('starts at the first recorded trade instead of inventing empty weeks', () => {
    const now = at(2026, 10, 28) * 1000;
    // A purchase opens the record; that week counts even with no sale in it.
    const weeks = weeklySales([t('purchase', 5, at(2026, 10, 13), [r('Y')]), sale(9, at(2026, 10, 27))], 12, now);
    expect(weeks.map((w) => [new Date(w.start).toDateString(), w.plat])).toEqual([
      [new Date(2026, 9, 12).toDateString(), 0],
      [new Date(2026, 9, 19).toDateString(), 0],
      [new Date(2026, 9, 26).toDateString(), 9],
    ]);
    expect(weeklySales([], 12, now)).toHaveLength(1);
  });

  // Only a zone with daylight saving can fail this (TZ=America/New_York does
  // when weeks step by fixed milliseconds); under UTC it always passes.
  it('steps weeks by calendar date across a daylight-saving change', () => {
    const weeks = weeklySales([], 6, at(2026, 11, 4) * 1000);
    for (const w of weeks) {
      expect(new Date(w.start).getDay()).toBe(1);
      expect(new Date(w.start).getHours()).toBe(0);
    }
  });

  it('takes the median of finished weeks only', () => {
    const w = (plat, partial = false) => ({ start: 0, plat, sales: 0, partial });
    expect(medianWeek([w(10), w(30), w(20), w(500, true)])).toBe(20);
    expect(medianWeek([w(10), w(30)])).toBe(20);
    expect(medianWeek([w(9, true)])).toBeNull();
  });

  it('folds the tail of top sellers into one Other row', () => {
    const rows = [1, 2, 3, 4].map((n) => ({ name: `I${n}`, qty: 1, plat: 10 * n, trades: 1 }));
    expect(topSellers(rows, 2)).toEqual({ top: rows.slice(0, 2), other: { items: 2, plat: 70 } });
    expect(topSellers(rows, 8).other).toBeNull();
  });
});
