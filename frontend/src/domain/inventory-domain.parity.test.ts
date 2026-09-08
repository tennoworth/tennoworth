import { describe, expect, it } from 'vitest';
import normalization from '../../../tests/fixtures/inventory-normalization/cases.json';
import scoring from '../../../tests/fixtures/inventory-scoring/cases.json';
import { normalizeInventoryPreview } from './normalize-inventory';
import { computeResults, type FilterState } from './filter-engine';
import type { Inventory, Market, OwnedRecord } from '../contracts/data';

const filters: FilterState = { minPrice: 0, minOwned: 0, typeFilter: 'all', hideAtLvl: 999, activeTags: new Set(), vaultOnly: false, ducatsOnly: false, minVol: 0, minMedian: 0, typesAny: [], sparesOnly: false, adviceOnly: false };

describe('inventory domain shared fixtures', () => {
  for (const row of normalization.cases) it(row.name, () => {
    const result = normalizeInventoryPreview(row.request.inventory as Inventory, { uniqueToInfo: new Map(row.request.catalog as Array<[string, {name: string; category: string}]>) }, row.request.market as unknown as Market);
    expect({ owned: [...result.owned], unresolved: result.unresolved, flat_count: result.flatCount }).toEqual(row.expected);
  });
  for (const row of scoring.cases) it(row.name, () => {
    const request = row.request;
    const results = computeResults(new Map(request.owned as Array<[string, OwnedRecord]>), request.market as unknown as Market, { ...filters, sparesOnly: request.spares_only }, request.reserve_copies);
    expect(results.map(result => Object.fromEntries(Object.keys(row.expected[0] ?? {}).map(field => [field, result[field as keyof typeof result]])))).toEqual(row.expected);
  });
});
