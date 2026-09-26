import { describe, expect, it } from 'vitest';

import { GLYPH_PATHS, glyphFor } from './glyphs';

describe('glyph paths', () => {
  it('every named glyph has path data', () => {
    for (const [name, d] of Object.entries(GLYPH_PATHS)) {
      expect(d, name).toMatch(/^M/);
      expect(d.length, name).toBeGreaterThan(10);
    }
  });

  it('stays inside the 24x24 box so glyphs line up in a column', () => {
    for (const [name, d] of Object.entries(GLYPH_PATHS)) {
      for (const n of d.match(/-?\d+(\.\d+)?/g) ?? []) {
        expect(Number(n), `${name} coordinate ${n}`).toBeGreaterThanOrEqual(0);
        expect(Number(n), `${name} coordinate ${n}`).toBeLessThanOrEqual(24);
      }
    }
  });

  it('has no curve commands - the house style is angular', () => {
    for (const [name, d] of Object.entries(GLYPH_PATHS)) {
      expect(d, name).not.toMatch(/[CcSsQqTtAa]/);
    }
  });
});

describe('glyphFor', () => {
  it('normalises the plural and casing the two sources disagree on', () => {
    expect(glyphFor('Warframes')).toBe('warframe');
    expect(glyphFor('warframe')).toBe('warframe');
    expect(glyphFor('  Melee ')).toBe('melee');
  });

  it('returns unknown rather than guessing', () => {
    expect(glyphFor('Somethingelse')).toBe('unknown');
    expect(glyphFor(undefined)).toBe('unknown');
    expect(glyphFor('')).toBe('unknown');
  });
});
