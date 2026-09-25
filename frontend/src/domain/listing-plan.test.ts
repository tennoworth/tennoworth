import { describe, expect, it } from 'vitest';
import { interruptedBatch } from './listing-plan';
import type { PendingPlan } from '../contracts/data';

function plan(statuses: PendingPlan['items'][number]['status'][]): PendingPlan {
  return {
    plan_id: 'p',
    started_at: '2026-09-21T10:00:00Z',
    items: statuses.map((status, index) => ({
      slug: `item-${index}`,
      platinum: 12,
      quantity: 1,
      order_type: 'sell' as const,
      visible: false,
      status,
    })),
  };
}

describe('interruptedBatch', () => {
  it('says nothing about a batch with no work left', () => {
    expect(interruptedBatch(null)).toBeNull();
    expect(interruptedBatch(plan([]))).toBeNull();
    expect(interruptedBatch(plan(['ok', 'ok']))).toBeNull();
  });

  it('offers resume while an item was never sent', () => {
    const batch = interruptedBatch(plan(['ok', 'pending']));
    expect(batch).toEqual({
      pending: 1,
      uncertain: 0,
      done: 1,
      detail: '· 1 pending, 1 already done',
      resumable: true,
    });
  });

  // The reason this module exists: an unknown outcome is not retryable work.
  // Offering Resume for it would promise a re-send whose predecessor may
  // already have been applied.
  it('does not offer resume when the only work left has an unknown outcome', () => {
    const batch = interruptedBatch(plan(['ok', 'uncertain_mutation', 'uncertain_mutation']));
    expect(batch).toEqual({
      pending: 0,
      uncertain: 2,
      done: 1,
      detail: '· 2 with an unknown outcome, 1 already done',
      resumable: false,
    });
  });

  it('reports both kinds when a batch holds both', () => {
    const batch = interruptedBatch(plan(['pending', 'uncertain_mutation']));
    expect(batch?.detail).toBe('· 1 pending, 1 with an unknown outcome');
    expect(batch?.resumable).toBe(true);
  });

  // A status the frontend does not recognise must not be counted as work it can
  // act on, and must not make the batch vanish either.
  it('ignores a status it does not know rather than inventing work', () => {
    const unknown = plan(['ok']);
    (unknown.items[0] as { status: string }).status = 'unexpected';
    expect(interruptedBatch(unknown)).toBeNull();
  });
});
