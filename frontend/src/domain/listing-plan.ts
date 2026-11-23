import type { PendingPlan } from '../contracts/data';
import type { InterruptedBatch } from '../contracts/generated/web';
import { call } from './native';

export type { InterruptedBatch };

/**
 * What an interrupted listing batch should say for itself, or null when
 * nothing about it needs saying. A `pending` item was never sent; an
 * `uncertain_mutation` item may already have reached the market, so Resume
 * checks current orders before it does anything with one. The rule is
 * native (market_domain::listing::interrupted_batch).
 */
export function interruptedBatch(plan: PendingPlan | null | undefined): InterruptedBatch | null {
  const items = plan?.items ?? [];
  if (items.length === 0) return null;
  return call({ op: 'interrupted_batch', input: { statuses: items.map(i => String(i.status)) } });
}
