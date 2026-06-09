// @ts-nocheck — vitest runs these as JS-style fixtures; full TS shapes here would be busy-work without catching real bugs.
import { describe, it, expect } from 'vitest';
import { flattenInventory, extractKeptLvls, TRADEABLE_CATEGORIES } from './inventory.js';

describe('flattenInventory', () => {
  it('yields nothing for an empty inventory', () => {
    expect([...flattenInventory({})]).toEqual([]);
  });

  it('walks only the configured tradeable categories', () => {
    const inv = {
      MiscItems: [{ ItemType: '/Lotus/A', ItemCount: 2 }],
      Suits: [{ ItemType: '/Lotus/Frame', ItemCount: 1 }],
      Boosters: [{ ItemType: '/Lotus/Booster', ItemCount: 1 }],  // not tradeable
      Quests: [{ ItemType: '/Lotus/Quest', ItemCount: 1 }],       // not tradeable
    };
    const got = [...flattenInventory(inv)];
    const paths = got.map(r => r.path);
    expect(paths).toContain('/Lotus/A');
    expect(paths).toContain('/Lotus/Frame');
    expect(paths).not.toContain('/Lotus/Booster');
    expect(paths).not.toContain('/Lotus/Quest');
  });

  it('attaches the source category to each entry', () => {
    const inv = {
      MiscItems: [{ ItemType: '/X', ItemCount: 1 }],
      RawUpgrades: [{ ItemType: '/Y', ItemCount: 3 }],
    };
    const byPath = Object.fromEntries(
      [...flattenInventory(inv)].map(e => [e.path, e.category])
    );
    expect(byPath['/X']).toBe('MiscItems');
    expect(byPath['/Y']).toBe('RawUpgrades');
  });

  it('defaults ItemCount to 1 when missing', () => {
    const inv = { MiscItems: [{ ItemType: '/Lotus/A' }] };
    expect([...flattenInventory(inv)][0].count).toBe(1);
  });

  it('accepts both ItemType and Type as the path field', () => {
    const inv = { MiscItems: [{ Type: '/Lotus/Legacy', ItemCount: 5 }] };
    expect([...flattenInventory(inv)][0].path).toBe('/Lotus/Legacy');
  });

  it('skips entries with no path at all', () => {
    const inv = { MiscItems: [{ ItemCount: 1 }, { ItemType: '/Lotus/Real' }] };
    const paths = [...flattenInventory(inv)].map(r => r.path);
    expect(paths).toEqual(['/Lotus/Real']);
  });

  it('handles category present but not an array (defensive)', () => {
    const inv = { MiscItems: null, Recipes: undefined };
    expect([...flattenInventory(inv)]).toEqual([]);
  });

  it('exposes the category list so callers can reason about coverage', () => {
    expect(TRADEABLE_CATEGORIES).toContain('MiscItems');
    expect(TRADEABLE_CATEGORIES).toContain('Suits');
    expect(TRADEABLE_CATEGORIES).not.toContain('Boosters');
  });
});

describe('extractKeptLvls', () => {
  it('returns an empty map when Upgrades is missing or not an array', () => {
    expect(extractKeptLvls({}).size).toBe(0);
    expect(extractKeptLvls({ Upgrades: null }).size).toBe(0);
    expect(extractKeptLvls(null).size).toBe(0);
  });

  it('parses UpgradeFingerprint as a JSON string and reads lvl', () => {
    const inv = {
      Upgrades: [
        { ItemType: '/Lotus/Mods/Vitality', UpgradeFingerprint: '{"lvl":10}' },
        { ItemType: '/Lotus/Mods/Streamline', UpgradeFingerprint: '{"lvl":3}' },
      ],
    };
    const m = extractKeptLvls(inv);
    expect(m.get('/Lotus/Mods/Vitality')).toBe(10);
    expect(m.get('/Lotus/Mods/Streamline')).toBe(3);
  });

  it('keeps the max lvl across multiple instances of the same mod', () => {
    const inv = {
      Upgrades: [
        { ItemType: '/Lotus/Mods/Foo', UpgradeFingerprint: '{"lvl":3}' },
        { ItemType: '/Lotus/Mods/Foo', UpgradeFingerprint: '{"lvl":7}' },
        { ItemType: '/Lotus/Mods/Foo', UpgradeFingerprint: '{"lvl":1}' },
      ],
    };
    expect(extractKeptLvls(inv).get('/Lotus/Mods/Foo')).toBe(7);
  });

  it('records lvl=0 for an instance with no fingerprint (still a kept instance)', () => {
    const inv = {
      Upgrades: [
        { ItemType: '/Lotus/Mods/Foo' },
      ],
    };
    // The mere presence of an Upgrades entry signals the user holds an
    // instance — even at lvl=0 — so callers can choose whether to filter.
    expect(extractKeptLvls(inv).has('/Lotus/Mods/Foo')).toBe(true);
    expect(extractKeptLvls(inv).get('/Lotus/Mods/Foo')).toBe(0);
  });

  it('silently tolerates malformed fingerprint JSON', () => {
    const inv = {
      Upgrades: [
        { ItemType: '/Lotus/Mods/Broken', UpgradeFingerprint: '{not json' },
      ],
    };
    expect(extractKeptLvls(inv).get('/Lotus/Mods/Broken')).toBe(0);
  });
});
