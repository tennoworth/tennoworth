// @ts-nocheck - malformed fixture rows are part of the public failure contract.
import { describe, expect, it } from 'vitest';
import fixture from '../../../tests/fixtures/usage-resolution/cases.json';
import { buildUsageParentIndex, usageFor } from './demand';

describe('usage resolution parity', () => {
  it('matches direct, inherited, malformed, missing, and ambiguous cases', () => {
    const market = { usage: fixture.usage, set_to_parts: fixture.set_to_parts };
    const index = buildUsageParentIndex(market);
    expect(index.get('ambiguous_part')).toBeNull();
    for (const testCase of fixture.cases) {
      const hit = usageFor(testCase.slug, market);
      expect(hit ? (hit.inherited ? index.get(testCase.slug) : testCase.slug) : null, testCase.slug)
        .toBe(testCase.expected_source);
      if (testCase.expected_source) {
        expect(hit?.inherited, testCase.slug).toBe(testCase.expected_inherited);
        expect(hit?.entry.share, testCase.slug).toBe(testCase.expected_share);
      } else {
        expect(hit, testCase.slug).toBeNull();
      }
    }
  });
});
