import { describe, expect, it } from 'vitest';

import SOURCE from './ResultsTable.svelte?raw';

/**
 * The results table is `table-layout: fixed`, so a column is exactly as wide
 * as its `width` says and its content is CLIPPED, never expanded. Two numbers
 * therefore have to agree, and nothing in the type system makes them:
 *
 *   sum(column widths)  +  the floor left for Item  ==  the table's min-width
 *
 * "Played" was added at 4.5rem against content that measures ~7.2rem, so it
 * was clipped mid-word in every state but "thin"/"slow" — and because Item
 * takes the remainder, the sixteenth column also came straight out of the item
 * name without moving the floor. This gate holds the arithmetic so the next
 * column has to make the same trade deliberately.
 */
// Read as source rather than rendered: these are CSS and literal-table facts
// the component never exposes at runtime. `?raw` is how theme.test.ts already
// asserts against a file it cannot import for its values.

/** Width the Item column keeps at the narrow end, once every fixed column is
 *  paid for. Item is the one column a trader cannot do without. */
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

function tableMinWidthRem(): number {
  const m = SOURCE.match(/table\s*\{[^}]*?min-width:\s*([\d.]+)rem/s);
  if (!m) throw new Error('the results table lost its min-width');
  return Number(m[1]);
}

describe('column width budget', () => {
  it('gives every column a declared width', () => {
    // A column with no width in a fixed-layout table is sized by the browser
    // from the FIRST row it sees, which makes the grid jump between pages.
    const declared = [...columnBlock().matchAll(/key:\s*'(\w+)'/g)].map((m) => m[1]);
    expect(columnWidths().map((c) => c.key)).toEqual(declared);
  });

  it('leaves Item its floor at the table min-width', () => {
    const sum = columnWidths().reduce((a, c) => a + c.width, 0);
    expect(tableMinWidthRem()).toBeCloseTo(sum + ITEM_FLOOR_REM, 2);
  });

  it('gives the Played column room for its widest real content', () => {
    // Measured in Firefox at the cell's own type: "12.3% ↑ cheap" is 95.3px
    // and the 100%-share extreme is 103.1px, plus 12px of td padding — 7.19rem
    // all in. The long labels this replaced needed 9.99rem, which the budget
    // could not pay.
    const played = columnWidths().find((c) => c.key === 'usage');
    expect(played?.width).toBeGreaterThanOrEqual(7.2);
  });
});
