import { expect, it } from 'vitest';
import { points, yearStats, weekly } from './history';
import histories from '../../../tests/fixtures/advisor/history.json';

// The market browser draws history with these; the native analysis reads the
// same fixture. slope30 is native-only now that advice is computed natively.
for (const { name, request, expected } of histories) it(name, () => {
  const { slope30: _, ...shared } = expected;
  expect({ points: points(request.series), stats: yearStats(request.series, request.min_days), weekly: weekly(request.series, request.buckets) }).toEqual(shared);
});
