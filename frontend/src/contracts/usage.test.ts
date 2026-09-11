import { describe, it, expect } from 'vitest';
import fixture from '../../../tests/fixtures/usage/daily.json';
import { parseUsageDaily } from './usage';
describe('public usage contract', () => {
  it('reads the shared collector fixture including zero and incomplete counts', () => {
    const result=parseUsageDaily(fixture);
    expect(result.days[1].count).toBe(0);
    expect(result.days[2].complete).toBe(false);
  });
  it('rejects missing data, invalid dates, duplicate days and invalid counts', () => {
    for (const value of [null, {}, { ...fixture, days:[{ date:'2026-02-30',count:1,complete:true }] },
      { ...fixture,days:[fixture.days[0],fixture.days[0]] },
      { ...fixture,days:[{ ...fixture.days[0],count:-1 }] }]) expect(() => parseUsageDaily(value)).toThrow();
  });
});
