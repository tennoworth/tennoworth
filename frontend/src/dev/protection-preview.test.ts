import { expect, it } from 'vitest';
import fixture from '../../../tests/fixtures/protection/allocation.json';
import { sampleAllocation } from './protection-preview';

it('the fictional preview follows the native allocation contract', () => {
  for (const row of fixture) expect(sampleAllocation(row.owned, row.unavailable, row.global_keep, row.reserved, row.listed)).toEqual(row.expected);
});
