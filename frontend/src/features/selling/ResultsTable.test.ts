import { afterEach, describe, expect, it } from 'vitest';
import { cleanup, render } from '@testing-library/svelte';
import ResultsTable from './ResultsTable.svelte';
import type { SellRow } from '../../contracts/selling';

afterEach(cleanup);

import SOURCE from './ResultsTable.svelte?raw';
import { PRESETS } from '../../domain/presets';

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
 * Width declarations are checked in source; the floor checks mount the real
 * component so a broken production calculation fails the gate.
 */
const ITEM_FLOOR_REM = 20;

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

const ROW: SellRow = {
  slug: 'part', subtype: null, name: 'Part', owned: 2, sellable: 2, leveled: 0,
  type: 'Weapon', kept_lvl: null, ducats: 15, plat_per_100d: 10,
  avg_price: 10, low_sell: 10, low5_avg: 10, top_buy: 8, volume_48h: 20,
  ratio: 1, potential_plat: 20, raw_value: 20, sell_score: 20,
  advice: 'hold', patience: false, timing: 'neutral', medians_7d: [],
  median_90d: null, delta_90d_pct: null, tags: [], is_augment: false, vault_status: null,
};

function renderedFloor(columns: string[], withFacts = true): number {
  const { container, unmount } = render(ResultsTable, {
    results: [{ ...ROW, advice: withFacts ? 'hold' : null }],
    deltas: withFacts ? new Map([['part', 1]]) : new Map(),
    visibleColumns: columns,
  });
  const table = container.querySelector('table');
  expect(table).not.toBeNull();
  const floor = table!.style.minWidth;
  expect(floor).toMatch(/^\d+(?:\.\d+)?rem$/);
  unmount();
  return Number.parseFloat(floor);
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

  it('leaves Item its floor on the full sixteen-column view', () => {
    const all = columnWidths().map((c) => c.key);
    expect(renderedFloor(all)).toBeCloseTo(
      columnWidths().reduce((a, c) => a + c.width, 0) + ITEM_FLOOR_REM,
      2,
    );
  });

  it('shrinks for a preset instead of charging it for columns it never shows', () => {
    // The six-column Ducats preset must not carry the everything-view's floor.
    const all = renderedFloor(columnWidths().map((c) => c.key));
    for (const [name, preset] of Object.entries(PRESETS)) {
      if (!preset.columns) continue;
      const floor = renderedFloor(preset.columns);
      expect(floor, `${name} should not pay the full-view floor`).toBeLessThan(all);
      // And it still has to cover what it does render.
      const widths = columnWidths().filter(column => preset.columns!.includes(column.key));
      expect(floor).toBeCloseTo(widths.reduce((sum, column) => sum + column.width, ITEM_FLOOR_REM), 5);
    }
  });

  it('charges only rendered facts when delta and advice are unavailable', () => {
    expect(renderedFloor(['name', 'owned', 'delta', 'advice'], false)).toBe(26);
    expect(renderedFloor(['name', 'owned', 'delta', 'advice'])).toBe(33.75);
  });

  it('never charges a preset for the Played column, which no preset shows', () => {
    for (const [name, preset] of Object.entries(PRESETS)) {
      expect(preset.columns ?? [], name).not.toContain('usage');
    }
  });
});
