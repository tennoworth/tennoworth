// @ts-nocheck — vitest runs these as JS-style fixtures; full TS shapes here would be busy-work without catching real bugs.
import { describe, it, expect, beforeEach } from 'vitest';
import {
  saveSnapshot, loadSnapshot, clearSnapshot, diffOwned,
} from './storage.js';

beforeEach(() => {
  localStorage.clear();
});

describe('saveSnapshot / loadSnapshot', () => {
  it('round-trips an owned Map through localStorage', () => {
    const owned = new Map([
      ['axi_k2_relic|radiant', { count: 7, name: 'Axi K2 Relic (Radiant)', type: 'Relics', slug: 'axi_k2_relic', subtype: 'radiant' }],
      ['vitality|',            { count: 51, name: 'Vitality', type: 'Mods', slug: 'vitality', subtype: null }],
      ['steel_charge|',        { count: 1, name: 'Steel Charge', type: 'Mods', slug: 'steel_charge', subtype: null, kept_lvl: 5 }],
    ]);
    saveSnapshot({ invName: 'inventory.json', owned });
    const got = loadSnapshot();
    expect(got.invName).toBe('inventory.json');
    expect(got.owned).toBeInstanceOf(Map);
    expect(got.owned.size).toBe(3);
    expect(got.owned.get('vitality|')).toEqual({
      count: 51, name: 'Vitality', type: 'Mods', slug: 'vitality', subtype: null, kept_lvl: null, leveled: 0,
    });
    expect(got.owned.get('axi_k2_relic|radiant').subtype).toBe('radiant');
    // kept_lvl must survive the round-trip — losing it disabled the leveled-mod
    // hide filter on every reload (the storage v3 bug).
    expect(got.owned.get('steel_charge|').kept_lvl).toBe(5);
    expect(got.ts).toBeGreaterThan(0);
  });

  it('round-trips leveled (untradeable-copy count)', () => {
    const owned = new Map([
      ['broken_war|', { count: 3, name: 'Broken War', type: 'Melee', slug: 'broken_war', subtype: null, kept_lvl: null, leveled: 2 }],
    ]);
    saveSnapshot({ invName: 'inventory.json', owned });
    const got = loadSnapshot();
    expect(got.owned.get('broken_war|').leveled).toBe(2);
  });

  it('returns null when nothing was saved', () => {
    expect(loadSnapshot()).toBeNull();
  });

  it('returns null on corrupted storage rather than throwing', () => {
    localStorage.setItem('wfminv:last-owned-v5', '{garbage');
    expect(loadSnapshot()).toBeNull();
  });

  it('clearSnapshot wipes the key', () => {
    saveSnapshot({ invName: 'a', owned: new Map([['x', { count: 1, name: 'X', type: 'Misc' }]]) });
    clearSnapshot();
    expect(loadSnapshot()).toBeNull();
  });

  it('does not throw when localStorage write fails (quota etc.)', () => {
    const orig = Storage.prototype.setItem;
    Storage.prototype.setItem = () => { throw new DOMException('QuotaExceededError'); };
    try {
      expect(() =>
        saveSnapshot({ invName: 'a', owned: new Map([['x', { count: 1, name: 'X', type: 'Misc' }]]) })
      ).not.toThrow();
    } finally {
      Storage.prototype.setItem = orig;
    }
  });
});

describe('diffOwned', () => {
  it('returns empty map when there is no previous snapshot', () => {
    const current = new Map([['a', { count: 5 }]]);
    expect(diffOwned(null, current).size).toBe(0);
  });

  it('reports positive delta for newly farmed items', () => {
    const prev = new Map([['axi', { count: 3 }]]);
    const curr = new Map([['axi', { count: 10 }]]);
    expect(diffOwned(prev, curr).get('axi')).toBe(7);
  });

  it('reports negative delta for items that decreased', () => {
    const prev = new Map([['vitality', { count: 51 }]]);
    const curr = new Map([['vitality', { count: 48 }]]);
    expect(diffOwned(prev, curr).get('vitality')).toBe(-3);
  });

  it('treats items missing from previous as full positive delta', () => {
    const prev = new Map();
    const curr = new Map([['new_item', { count: 4 }]]);
    expect(diffOwned(prev, curr).get('new_item')).toBe(4);
  });

  it('skips items whose count is unchanged', () => {
    const prev = new Map([['x', { count: 5 }]]);
    const curr = new Map([['x', { count: 5 }]]);
    expect(diffOwned(prev, curr).has('x')).toBe(false);
  });

  it('ignores items present in previous but not current (we only show current)', () => {
    const prev = new Map([['gone', { count: 5 }]]);
    const curr = new Map([['kept', { count: 5 }]]);
    const d = diffOwned(prev, curr);
    expect(d.has('gone')).toBe(false);
    expect(d.has('kept')).toBe(true);  // new -> +5
  });
});
