// @ts-nocheck
import { describe, it, expect } from 'vitest';
import { totals, since, soldByItem, describeItems } from './ledger.js';

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
    expect(describeItems(t('sale', 0, 1, []))).toBe('—');
  });
});
