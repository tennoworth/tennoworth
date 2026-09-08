import { describe, expect, it } from 'vitest';
import fixtures from '../../../tests/fixtures/planners/cases.json';
import { planBuild, cheapestPath } from './build-cost';
import { deriveRelicPlan } from './relic-planner';
import { deriveSetRecos } from './set-recos';
import { planDucats, scrapCandidates } from './ducat-plan';
import type { Market, OwnedRecord } from '../contracts/data';

describe('planner shared contracts', () => {
  for (const fixture of fixtures) it(fixture.name, () => {
    const input = fixture.input;
    const owned = new Map(input.owned.map((r, i) => [String(i), r as OwnedRecord]));
    const market = input.market as unknown as Market;
    let output: unknown;
    switch (fixture.operation) {
      case 'sets': output = deriveSetRecos(owned, market, input.limit ?? undefined); break;
      case 'relic': output = deriveRelicPlan(owned, market, input.limit ?? undefined); break;
      case 'ducat': {
        const candidates = scrapCandidates(owned, market);
        output = { candidates, plan: planDucats(candidates, input.target!, input.keepAbove ?? undefined) }; break;
      }
      case 'build': {
        const plan = planBuild(input.setSlug!, input.setName!, input.parts!, market, owned, input.recipes);
        output = { plan, cheapest: cheapestPath(plan) }; break;
      }
    }
    // JSON is the IPC contract: unpriced scrap ratios cross it as null.
    expect(JSON.parse(JSON.stringify(output))).toEqual(fixture.expected);
  });
});
