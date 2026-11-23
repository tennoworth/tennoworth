import { expect, it } from 'vitest';
import { yearViews } from './history';
import histories from '../../../tests/fixtures/advisor/history.json';

// The market browser draws history with these; the native analysis reads the
// same fixture. slope30 is native-only now that advice is computed natively.
for (const { name, request, expected } of histories) it(name, () => {
  const [view] = yearViews([request.series], { minDays: request.min_days ?? undefined, buckets: request.buckets ?? undefined });
  expect(view).toEqual({ stats: expected.stats, weekly: expected.weekly });
});
