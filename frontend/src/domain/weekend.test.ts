import { describe, expect, it } from 'vitest';
import { weekendNudge } from './weekend';

// 2026-11-16 is a Monday.
const day = (offset: number) => new Date(2026, 10, 16 + offset, 12);

describe('weekendNudge', () => {
  it('stays quiet early in the week', () => {
    expect([0, 1, 2].map((d) => weekendNudge(day(d)))).toEqual([null, null, null]);
  });

  it('says list before the weekend on Thursday and Friday', () => {
    expect([3, 4].map((d) => weekendNudge(day(d)))).toEqual(['before', 'before']);
  });

  it('says the buyers are here on Saturday and Sunday', () => {
    expect([5, 6].map((d) => weekendNudge(day(d)))).toEqual(['during', 'during']);
  });
});
