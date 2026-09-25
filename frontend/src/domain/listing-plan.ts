import type { PendingPlan } from '../contracts/data';

/**
 * What an interrupted listing batch should say for itself.
 *
 * The batch holds items in three states and they do not mean the same thing.
 * A `pending` item was never sent, so offering Resume is honest. An
 * `uncertain_mutation` item may already have reached the market, so offering
 * Resume would promise a retry nobody can safely perform - it needs
 * reconciliation against the account's live orders first. A batch can hold both
 * at once, and the banner has to say so rather than reporting one and hiding the
 * other.
 */
export interface InterruptedBatch {
  pending: number;
  uncertain: number;
  done: number;
  /** The line under the heading. Always non-empty for a non-null result. */
  detail: string;
  /**
   * Whether Resume may be offered. False when the only work left is an
   * unresolved outcome, because resuming would re-send a request whose fate is
   * unknown.
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

  const parts: string[] = [];
  if (pending > 0) parts.push(`${pending} pending`);
  if (uncertain > 0) {
    parts.push(`${uncertain} with an unknown outcome`);
  }
  if (done > 0) parts.push(`${done} already done`);
  return {
    pending,
    uncertain,
    done,
    detail: `· ${parts.join(', ')}`,
    resumable: pending > 0,
  };
}
