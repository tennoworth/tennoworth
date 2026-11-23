import { describe, expect, it } from 'vitest';
import scoring from '../../../tests/fixtures/inventory-scoring.json';
import { computeResults, type FilterState } from './filter-engine';
import type { Market, OwnedRecord } from '../contracts/data';
import type { ScoredInventoryFact } from '../contracts/generated/domain';

const filters: FilterState = { minPrice: 0, minOwned: 0, typeFilter: 'all', hideAtLvl: 999, activeTags: new Set(), vaultOnly: false, ducatsOnly: false, minVol: 0, minMedian: 0, typesAny: [], sparesOnly: false, adviceOnly: false };

// market-domain scores these cases (scoring.rs reads the same file); the table
// must show what it answered without re-deriving any of it.
describe('the Sell table shows the native inventory facts', () => {
  for (const row of scoring.cases) it(row.name, () => {
    const request = row.request;
    const facts = new Map((row.expected as unknown as ScoredInventoryFact[]).map(fact => [fact.key, fact]));
    const results = computeResults(new Map(request.owned as Array<[string, OwnedRecord]>), request.market as unknown as Market, { ...filters, sparesOnly: request.spares_only }, facts, undefined, 'availability' in request ? new Map(Object.entries(request.availability ?? {})) : undefined);
    expect(results.map(result => Object.fromEntries(Object.keys(row.expected[0] ?? {}).map(field => [field, result[field as keyof typeof result]])))).toEqual(row.expected);
  });
});
