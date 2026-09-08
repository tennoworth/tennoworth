import { describe, expect, it } from 'vitest';
import { selectSession, type SessionCandidate, type SessionMode } from './trade-session';
import { adviseOwned, slope30 } from './advisor';
import { points, yearStats, weekly, type History } from './history';
import type { Market } from '../contracts/data';
import selector from '../../../tests/fixtures/trade-session/selector.json';
import histories from '../../../tests/fixtures/advisor/history.json';
import verdicts from '../../../tests/fixtures/advisor/verdicts.json';

describe('native decision contracts', () => {
  for (const { name, request, expected } of selector) it(name, () => {
    const plan = selectSession(request.candidates as SessionCandidate[], request.mode as SessionMode, request.budget, request.target ?? undefined);
    // The score and volume are private sort inputs, not fields of SessionRow.
    const rows = plan.rows.map(row => Object.fromEntries(Object.entries(row).filter(([key]) => key !== 'weight' && key !== 'volume')));
    expect({ ...plan, rows }).toEqual(expected);
  });
  for (const { name, request, expected } of histories) it(name, () => {
    expect({ points: points(request.series), stats: yearStats(request.series, request.min_days), weekly: weekly(request.series, request.buckets), slope30: slope30(request.series.median) }).toEqual(expected);
  });
  for (const { name, request, expected } of verdicts) it(name, () => {
    expect(Object.fromEntries(adviseOwned(request.slugs, request.market as unknown as Market, request.history as unknown as History | null, request.now_ms))).toEqual(expected);
  });
});
