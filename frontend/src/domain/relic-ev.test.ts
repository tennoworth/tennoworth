import { expect, it } from 'vitest';
import { REFINE_WORTH_IT } from './relic-ev';
import fixture from '../../../tests/fixtures/relic-refine.json';

it('quotes the refinement bar the native planner applies', () => {
  expect(REFINE_WORTH_IT).toBe(fixture.plat_per_trace);
});
