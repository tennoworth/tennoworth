// @ts-nocheck - vitest runs these as JS-style fixtures; full TS shapes here would be busy-work without catching real bugs.
import { describe, it, expect } from 'vitest';
import { PRESETS, presetFilterValues, presetStillMatches } from './presets.js';

describe('presetFilterValues', () => {
  it('returns null for an unknown preset name', () => {
    expect(presetFilterValues('nonsense')).toBeNull();
  });

  it('mirrors the named preset, with activeTags as a fresh Set', () => {
    const v = presetFilterValues('sets');
    expect(v).toMatchObject({ minPrice: 0, hideAtLvl: 11, typeFilter: 'all' });
    expect(v.activeTags).toEqual(new Set(['set']));
  });

  it('returns an independent Set each call, not a shared reference', () => {
    const a = presetFilterValues('sets');
    const b = presetFilterValues('sets');
    a.activeTags.add('mutated');
    expect(b.activeTags.has('mutated')).toBe(false);
    expect(PRESETS.sets.activeTags).toEqual(['set']); // the source data is untouched
  });
});

describe('presetStillMatches', () => {
  it('is true right after applying the preset', () => {
    const v = presetFilterValues('default');
    expect(presetStillMatches('default', v)).toBe(true);
  });

  it('is false once any hand-set field diverges', () => {
    const v = presetFilterValues('default');
    expect(presetStillMatches('default', { ...v, minPrice: v.minPrice + 1 })).toBe(false);
    expect(presetStillMatches('default', { ...v, typeFilter: 'Mods' })).toBe(false);
    expect(presetStillMatches('default', { ...v, activeTags: new Set(['extra']) })).toBe(false);
  });

  it('is false for an unknown preset name', () => {
    const v = presetFilterValues('default');
    expect(presetStillMatches('nonsense', v)).toBe(false);
  });
});
