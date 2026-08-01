import { describe, it, expect } from 'vitest';

import { MAX_PLATINUM, MIN_PLATINUM, MAX_PLAN_ITEMS } from './limits.js';
import limitsFixture from '../../../tests/fixtures/limits.json';

// Parity gate: wfm-core enforces these server-side (MAX_PLAN_ITEMS and
// MIN_PLATINUM in plan.rs, MAX_PLATINUM in listing.rs) and asserts the same
// fixture from its own test. The companion is the source of truth; drift here
// surfaces as the UI cheerfully accepting a batch the companion then rejects.
describe('listing limits', () => {
  it('match the caps pinned in the shared fixture', () => {
    expect(MAX_PLAN_ITEMS).toBe(limitsFixture.max_plan_items);
    expect(MIN_PLATINUM).toBe(limitsFixture.min_platinum);
    expect(MAX_PLATINUM).toBe(limitsFixture.max_platinum);
  });
});
