// Session record of what was just listed, so the row the user acted on can say
// so. After Send and Done the Sell view used to look exactly as before - the
// pick still offered List, and the only mention that new listings start hidden
// was in the dialog that had just closed.

import type { ItemResult } from '../../contracts/data';

export interface SentItem {
  slug: string;
  /** Relic refinement or similar variant; tiers of one slug list separately. */
  subtype?: string | null;
  rank?: number | null;
  platinum: number;
  quantity: number;
}

export interface ListingReceipt {
  slug: string;
  subtype: string | null;
  rank: number;
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
  byItem = $state(new Map<string, ListingReceipt>());
  #now: () => number;

  constructor(now: () => number = Date.now) {
    this.#now = now;
  }

  /** Only confirmed rows become receipts: pending, uncertain and failed rows
   *  are the interrupted-batch banner's job, not a claim that it listed.
   *  Results carry only a slug, so they are matched to what was sent by
   *  position (the native plan answers one result per item, in order), or by
   *  slug when that slug was sent once; never by slug across sibling tiers. */
  record(sent: SentItem[], results: ItemResult[]): void {
    const positional = results.length === sent.length && results.every((r, i) => r.slug === sent[i].slug);
    const once = new Map<string, SentItem | null>();
    for (const item of sent) once.set(item.slug, once.has(item.slug) ? null : item);
    const next = new Map(this.byItem);
    results.forEach((result, i) => {
      const item = positional ? sent[i] : once.get(result.slug);
      if (result.status !== 'ok' || !item) return;
      const action = result.action === 'updated' ? 'updated' : 'created';
      const receipt: ListingReceipt = {
        slug: item.slug, subtype: item.subtype ?? null, rank: item.rank ?? 0,
        platinum: item.platinum, quantity: item.quantity,
        visible: action === 'created' ? false : null, action,
        orderId: result.order_id ?? null, at: this.#now(),
      };
      next.set(receiptKey(receipt.slug, receipt.subtype, receipt.rank), receipt);
    });
    this.byItem = next;
  }

  /** Marks the orders WFM confirmed as visible. */
  markVisible(results: ItemResult[]): void {
    const shown = new Set(results.filter((r) => r.status === 'ok' && r.order_id).map((r) => r.order_id as string));
    if (shown.size === 0) return;
    const next = new Map(this.byItem);
    for (const [key, receipt] of next) {
      if (receipt.orderId && shown.has(receipt.orderId)) next.set(key, { ...receipt, visible: true });
    }
    this.byItem = next;
  }

  /** The latest receipt for one row (a slug and its variant, any rank). */
  forRow(slug: string, subtype: string | null | undefined): ListingReceipt | undefined {
    let latest: ListingReceipt | undefined;
    for (const receipt of this.byItem.values()) {
      if (receipt.slug !== slug || receipt.subtype !== (subtype ?? null)) continue;
      if (!latest || receipt.at >= latest.at) latest = receipt;
    }
    return latest;
  }

  /** Copies listed this session for one row, across ranks. */
  listedFor(slug: string, subtype: string | null | undefined): number {
    let total = 0;
    for (const receipt of this.byItem.values()) if (receipt.slug === slug && receipt.subtype === (subtype ?? null)) total += receipt.quantity;
    return total;
  }
}

function receiptKey(slug: string, subtype: string | null, rank: number): string {
  return `${slug}|${subtype ?? ''}|${rank}`;
}
