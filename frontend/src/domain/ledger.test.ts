// @ts-nocheck
import { describe, it, expect } from 'vitest';
import { totals, since, soldByItem, describeItems, outcomeRows, outcomeHeadline } from './ledger.js';

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
});
