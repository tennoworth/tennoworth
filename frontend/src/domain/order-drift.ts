// Order drift - "the market moved under your live listings": which sell
// listings sit far enough from the snapshot's clearing price to be worth a
// reprice. The rule is native (market_domain::listing::select_drifted, which
// documents its gates and the snapshot's limits); this is its call shape.

import type { DriftInput, DriftRow } from '../contracts/generated/web';
import { call } from './native';

export type { DriftInput, DriftRow };

/** Drifted sell listings, biggest platinum swing first. Buy orders are
 *  skipped: every reference price describes what a seller clears at. */
export function selectDrifted(orders: readonly DriftInput[], limit?: number): DriftRow[] {
  if (orders.length === 0) return [];
  return call({ op: 'select_drifted', input: { orders: [...orders], ...(limit === undefined ? {} : { limit }) } });
}
