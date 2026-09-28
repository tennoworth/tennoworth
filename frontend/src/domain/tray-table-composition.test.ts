import { expect, it } from 'vitest';
import fixture from '../../../tests/fixtures/tray-table-composition/cases.json';
import { computeResults, type FilterState } from './filter-engine';
import type { Market, OwnedRecord } from '../contracts/data';

it('composes the shared scan into the table with its documented local differences', () => {
  // Native normalization turns the scan into these rows (its side of this
  // fixture is sellables.rs); two DE paths resolve to one market row.
  const owned = new Map<string, OwnedRecord>();
  for (const row of fixture.inventory) {
    const rec = owned.get(`${row.slug}|`) ?? { slug: row.slug, name: row.name, type: row.category, subtype: null, count: 0, leveled: 0, kept_lvl: null };
    rec.count += row.count;
    if (row.xp > 0) rec.leveled += row.count;
    owned.set(`${row.slug}|`, rec);
  }
  expect(owned.get('alpha|')?.count).toBe(3);
  const market = fixture.market as unknown as Market;

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
