import { describe, it, expect } from 'vitest';

import {
  wfmItemUrl, baroLocation, humanWindow, plat,
  ownedBreakdown, keptNoteTitle, freshnessLabel,
} from './format.js';

describe('wfmItemUrl', () => {
  it('builds the canonical item page', () => {
    expect(wfmItemUrl('axi_k2_relic')).toBe('https://warframe.market/items/axi_k2_relic');
  });
});

describe('baroLocation', () => {
  it('cleans up internal node names', () => {
    expect(baroLocation('TennoConHUB2')).toBe('TennoCon Relay');
    expect(baroLocation('SolarisUnitedHub1')).toBe('Fortuna backroom');
  });

  it('passes display names through untouched', () => {
    expect(baroLocation('Strata Relay (Earth)')).toBe('Strata Relay (Earth)');
  });
});

describe('humanWindow', () => {
  it('drops to the two largest units', () => {
    expect(humanWindow((3 * 24 * 60 + 4 * 60 + 30) * 60000)).toBe('3d 4h'); // minutes hidden
    expect(humanWindow((4 * 60 + 12) * 60000)).toBe('4h 12m');
    expect(humanWindow(12 * 60000)).toBe('12m');
  });

  it('renders exact boundaries without carrying into a bogus unit', () => {
    expect(humanWindow(24 * 60 * 60000)).toBe('1d 0h');
    expect(humanWindow(60 * 60000)).toBe('1h 0m');
    expect(humanWindow(0)).toBe('0m');
  });

  it('floors rather than rounds up', () => {
    expect(humanWindow(59_999)).toBe('0m');
  });

  // The Baro feed goes absent during warframestat outages and market.json can
  // bake an empty surface, so these arrive in practice. "NaNd NaNh" on the
  // countdown is worse than admitting we don't know.
  it('refuses to render an unknown window', () => {
    expect(humanWindow(null)).toBe('—');
    expect(humanWindow(undefined)).toBe('—');
    expect(humanWindow(NaN)).toBe('—');
    expect(humanWindow(Infinity)).toBe('—');
    expect(humanWindow(-1)).toBe('—');
  });
});

describe('plat', () => {
  it('rounds and thousands-separates', () => {
    // en-US separators; the suite runs with a fixed locale.
    expect(plat(2500)).toBe((2500).toLocaleString());
    expect(plat(2499.6)).toBe((2500).toLocaleString());
    expect(plat(12)).toBe('12');
  });

  // The reason this helper exists: the review modal used avg.toFixed(0) while
  // the table used Math.round().toLocaleString(), so a maxed arcane read
  // "2500" in one place and "2,500" in the other.
  it('agrees with the table rendering it used to diverge from', () => {
    expect(plat(2500)).toBe(Math.round(2500).toLocaleString());
  });

  it('renders a missing value rather than NaN', () => {
    expect(plat(null)).toBe('—');
    expect(plat(undefined)).toBe('—');
    expect(plat(NaN)).toBe('—');
  });
});

describe('ownedBreakdown', () => {
  it('splits held-back copies into leveled and reserve-kept', () => {
    // 10 owned, 4 sellable, 2 of the 6 held back are leveled.
    expect(ownedBreakdown(10, 4, 2)).toEqual({ heldBack: 6, leveledPart: 2, keptPart: 4 });
  });

  it('clamps leveled to what is actually held back', () => {
    // 5 leveled but only 2 held back (reserve is 0) — an unclamped subtraction
    // would render "-3 kept".
    expect(ownedBreakdown(10, 8, 5)).toEqual({ heldBack: 2, leveledPart: 2, keptPart: 0 });
  });

  it('treats a missing leveled count as zero', () => {
    expect(ownedBreakdown(3, 1, null)).toEqual({ heldBack: 2, leveledPart: 0, keptPart: 2 });
    expect(ownedBreakdown(3, 1, undefined)).toEqual({ heldBack: 2, leveledPart: 0, keptPart: 2 });
  });

  it('holds nothing back when everything is sellable', () => {
    expect(ownedBreakdown(4, 4, 0)).toEqual({ heldBack: 0, leveledPart: 0, keptPart: 0 });
  });
});

describe('keptNoteTitle', () => {
  it('pluralises copy/copies', () => {
    expect(keptNoteTitle(1)).toContain('1 copy ');
    expect(keptNoteTitle(2)).toContain('2 copies ');
  });
});

describe('freshnessLabel', () => {
  it('maps each bucket to its human label', () => {
    expect(freshnessLabel('fresh')).toBe('under 3 hours old');
    expect(freshnessLabel('aging')).toBe('3 to 24 hours old');
    expect(freshnessLabel('stale')).toBe('over 24 hours old');
    expect(freshnessLabel('unknown')).toBe('age unknown — no timestamp in this snapshot');
  });
});
