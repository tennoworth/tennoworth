// Session record of what was just listed, so the row the user acted on can say
// so. After Send and Done the Sell view used to look exactly as before - the
// pick still offered List, and the only mention that new listings start hidden
// was in the dialog that had just closed.

import type { ItemResult } from '../../contracts/data';

export interface SentItem {
  slug: string;
  platinum: number;
  quantity: number;
}

export interface ListingReceipt {
  slug: string;
  platinum: number;
  quantity: number;
  /** Created listings start hidden; an updated order keeps whatever WFM had,
   *  which this session does not know. */
  visible: boolean | null;
  action: 'created' | 'updated';
  orderId: string | null;
  at: number;
}

export class ListingReceipts {
  bySlug = $state(new Map<string, ListingReceipt>());
  #now: () => number;

  constructor(now: () => number = Date.now) {
    this.#now = now;
  }

  /** Only confirmed rows become receipts: pending, uncertain and failed rows
   *  are the interrupted-batch banner's job, not a claim that it listed. */
  record(sent: SentItem[], results: ItemResult[]): void {
    const bySlug = new Map(sent.map((item) => [item.slug, item]));
    const next = new Map(this.bySlug);
    for (const result of results) {
      const item = bySlug.get(result.slug);
      if (result.status !== 'ok' || !item) continue;
      const action = result.action === 'updated' ? 'updated' : 'created';
      next.set(result.slug, {
        slug: result.slug, platinum: item.platinum, quantity: item.quantity,
        visible: action === 'created' ? false : null, action,
        orderId: result.order_id ?? null, at: this.#now(),
      });
    }
    this.bySlug = next;
  }

  /** Marks the orders WFM confirmed as visible. */
  markVisible(results: ItemResult[]): void {
    const shown = new Set(results.filter((r) => r.status === 'ok' && r.order_id).map((r) => r.order_id as string));
    if (shown.size === 0) return;
    const next = new Map(this.bySlug);
    for (const [slug, receipt] of next) {
      if (receipt.orderId && shown.has(receipt.orderId)) next.set(slug, { ...receipt, visible: true });
    }
    this.bySlug = next;
  }

  get(slug: string): ListingReceipt | undefined {
    return this.bySlug.get(slug);
  }
}
