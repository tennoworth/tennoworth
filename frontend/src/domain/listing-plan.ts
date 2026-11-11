import type { PendingPlan } from '../contracts/data';

/**
 * What an interrupted listing batch should say for itself.
 *
 * The batch holds items in three states and they do not mean the same thing.
 * A `pending` item was never sent, so Resume sends it. An `uncertain_mutation`
 * item may already have reached the market, so Resume never simply re-sends it:
 * it checks the account's current orders first, records a listing that landed,
 * sends one that clearly did not, and leaves anything unclear for the user to
 * check in My Orders. A batch can hold both at once, and the banner has to say
 * so rather than reporting one and hiding the other.
 */
export interface InterruptedBatch {
  pending: number;
  uncertain: number;
  done: number;
  /**
   * Whether Resume may be offered: there is unsent work, or an unknown outcome
   * a check of current orders may resolve.
   */
  resumable: boolean;
}

/**
 * Classify a batch, or null when nothing about it needs saying - a plan whose
 * items all finished has no interrupted work to report, and rendering an empty
 * banner is worse than rendering none.
 */
export function interruptedBatch(plan: PendingPlan | null | undefined): InterruptedBatch | null {
  const items = plan?.items ?? [];
  const pending = items.filter((i) => i.status === 'pending').length;
  const uncertain = items.filter((i) => i.status === 'uncertain_mutation').length;
  const done = items.filter((i) => i.status === 'ok').length;
  if (pending === 0 && uncertain === 0) return null;

  return {
    pending,
    uncertain,
    done,
    resumable: pending > 0 || uncertain > 0,
  };
}
