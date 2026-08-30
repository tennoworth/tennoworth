import { describe, expect, it } from 'vitest';

import SOURCE from './ResultsTable.svelte?raw';
import { PRESETS } from '../lib/presets';

/**
 * The results table is `table-layout: fixed`, so a column is exactly as wide
 * as its `width` says and its content is CLIPPED, never expanded. Three facts
 * have to agree, and nothing in the type system makes them:
 *
 *   - every column declares a width
 *   - the widest real content of a column fits inside it
 *   - the table's horizontal floor = the VISIBLE widths + Item's floor
 *
 * Two defects live here. "Played" was added at 4.5rem against content that
 * measures ~7.2rem, so it was clipped mid-word in every state but
 * "thin"/"slow"; and the floor was a flat `min-width` in the stylesheet, so a
 * six-column preset carried the sixteen-column view's floor and scrolled
 * sideways for columns it never renders.
 *
 * Read as source rather than rendered: these are CSS and literal-table facts
 * the component never exposes at runtime. `?raw` is how theme.test.ts already
 * asserts against a file it cannot import for its values.
 */
const ITEM_FLOOR_REM = 8.5;

function columnBlock(): string {
  const block = SOURCE.slice(SOURCE.indexOf('const ALL_COLUMNS'));
  return block.slice(0, block.indexOf('];'));
}

function columnWidths(): { key: string; width: number }[] {
  return [...columnBlock().matchAll(/key:\s*'(\w+)'[^}]*?width:\s*([\d.]+)/g)].map((m) => ({
    key: m[1],
    width: Number(m[2]),
  }));
}

/** The floor the component derives for a given set of visible column keys -
 *  the same arithmetic `floorRem` performs, against the same widths. */
function floorFor(keys: string[]): number {
  const byKey = new Map(columnWidths().map((c) => [c.key, c.width]));
  return keys.reduce((sum, k) => sum + (byKey.get(k) ?? 0), 0) + ITEM_FLOOR_REM;
}

describe('column width budget', () => {
  it('gives every column a declared width', () => {
    // A column with no width in a fixed-layout table is sized by the browser
    // from the FIRST row it sees, which makes the grid jump between pages.
    const declared = [...columnBlock().matchAll(/key:\s*'(\w+)'/g)].map((m) => m[1]);
    expect(columnWidths().map((c) => c.key)).toEqual(declared);
  });

  it('gives the Played column room for its widest real content', () => {
    // Measured in Firefox at the cell's own type: "12.3% ↑ cheap" is 95.3px
    // and the 100%-share extreme is 103.1px, plus 12px of td padding - 7.19rem
    // all in. The long labels this replaced needed 9.99rem, which the budget
    // could not pay.
    const played = columnWidths().find((c) => c.key === 'usage');
    expect(played?.width).toBeGreaterThanOrEqual(7.2);
  });
});

describe('the table floor', () => {
  it('is derived from the visible columns, never a fixed stylesheet value', () => {
    // The regression this file exists for: a static `min-width` on the table
    // rule applies to EVERY preset and both tables.
    const tableRule = SOURCE.match(/\n {2}table \{[^}]*\}/s)?.[0] ?? '';
    expect(tableRule).not.toMatch(/min-width/);
    expect(SOURCE).toMatch(/let floorRem = \$derived\(/);
    expect(SOURCE).toContain(`const ITEM_FLOOR_REM = ${ITEM_FLOOR_REM};`);
  });

  it('is carried by both tables, so the picks stay aligned over the rows', () => {
    const inline = [...SOURCE.matchAll(/<table[^>]*style="min-width:\{floorRem\}rem"/g)];
    expect(inline).toHaveLength(2);
  });

  it('leaves Item its floor on the full sixteen-column view', () => {
    const all = columnWidths().map((c) => c.key);
    expect(floorFor(all)).toBeCloseTo(
      columnWidths().reduce((a, c) => a + c.width, 0) + ITEM_FLOOR_REM,
      2,
    );
  });

  it('shrinks for a preset instead of charging it for columns it never shows', () => {
    // The six-column Ducats preset must not carry the everything-view's floor.
    const all = floorFor(columnWidths().map((c) => c.key));
    for (const [name, preset] of Object.entries(PRESETS)) {
      if (!preset.columns) continue;
      const floor = floorFor(preset.columns);
      expect(floor, `${name} should not pay the full-view floor`).toBeLessThan(all);
      // And it still has to cover what it does render.
      expect(floor).toBeCloseTo(floorFor(preset.columns), 5);
    }
  });

  it('never charges a preset for the Played column, which no preset shows', () => {
    for (const [name, preset] of Object.entries(PRESETS)) {
      expect(preset.columns ?? [], name).not.toContain('usage');
    }
  });
});
