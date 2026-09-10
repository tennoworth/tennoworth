import { expect, it } from 'vitest';
import fixture from '../../../tests/fixtures/protection/allocation.json';
import { sampleAllocation, sampleGuidance } from './protection-preview';

it('the fictional preview follows the native allocation contract', () => {
  for (const row of fixture) expect(sampleAllocation(row.owned, row.unavailable, row.global_keep, row.reserved, row.listed)).toEqual(row.expected);
});

import guidanceCases from '../../../tests/fixtures/protection/guidance.json';

it('preview and native guidance share import, protection and scan-identity cases', () => {
  for (const row of guidanceCases) {
    const native: import('../contracts/protection').ProtectionState = { plan: { reserves: { part: row.reserved }, goal: row.goal ? 'test_set' : null }, snapshot_id: row.record ? 1 : null,
      items: row.record ? { part: sampleAllocation(row.native_count, row.leveled, row.keep, row.reserved + (row.goal ? 2 : 0), row.orders ? 0 : null) } : {}, issues: [] };
    const result = sampleGuidance(native, { snapshot_id: row.request_id, items: { part: { count: row.input_count, leveled: row.leveled } } }, row.keep, row.goal ? { part: 2 } : {}, row.source);
    expect(result.snapshot_id, row.name).toBe(row.expected_snapshot_id);
    expect(result.items.part, row.name).toEqual(row.expected);
  }
});
