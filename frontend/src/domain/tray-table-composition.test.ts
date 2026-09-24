import { expect, it } from 'vitest';
import fixture from '../../../tests/fixtures/tray-table-composition/cases.json';
import { normalizeInventoryPreview } from './normalize-inventory';
import { computeResults, type FilterState } from './filter-engine';
import type { Inventory, Market } from '../contracts/data';

it('composes the shared scan into the table with its documented local differences', () => {
  const inventory: Record<string, Array<{ ItemType: string; ItemCount: number; XP: number }>> = {};
  const path_to_info: Record<string, { name: string; slug: string; category: string }> = {};
  for (const row of fixture.inventory) {
    (inventory[row.category] ??= []).push({ ItemType: row.path, ItemCount: row.count, XP: row.xp });
    path_to_info[row.path] = { name: row.name, slug: row.slug, category: row.category };
  }
  const market = { ...fixture.market, path_to_info } as unknown as Market;
  const { owned, unresolved } = normalizeInventoryPreview(inventory as Inventory, { uniqueToInfo: new Map() }, market);
  expect(unresolved).toEqual({});
  expect(owned.get('alpha|')?.count).toBe(3); // two DE paths resolve to one market row

  const filters: FilterState = {
    minPrice: 0, minOwned: 1, typeFilter: 'all', hideAtLvl: 999,
    activeTags: new Set(), vaultOnly: false, ducatsOnly: false,
    minVol: 0, minMedian: 0, typesAny: [], sparesOnly: false, adviceOnly: false,
  };
  const results = computeResults(owned, market, filters, fixture.reserve_copies);
  expect(results.map(({ slug, sellable }) => ({ slug, sellable }))).toEqual(fixture.expected.table);
  expect(results.filter(row => fixture.expected.shared_order.includes(row.slug)).map(row => row.slug))
    .toEqual(fixture.expected.shared_order);
  expect(results.find(row => row.slug === 'beta')?.sellable).toBe(3);
  expect(results.find(row => row.slug === 'gamma')?.sellable).toBe(1);
  expect(results.find(row => row.slug === 'delta')?.sellable).toBe(1);
});
