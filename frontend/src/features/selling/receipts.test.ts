import { describe, expect, it } from 'vitest';
import { ListingReceipts } from './receipts.svelte';

describe('ListingReceipts', () => {
  it('records only confirmed rows; created listings start hidden', () => {
    const receipts = new ListingReceipts(() => 5);
    receipts.record(
      [{ slug: 'a', platinum: 6, quantity: 1 }, { slug: 'b', platinum: 9, quantity: 2 }, { slug: 'c', platinum: 3, quantity: 1 }],
      [
        { slug: 'a', status: 'ok', action: 'created', order_id: 'o-a' },
        { slug: 'b', status: 'ok', action: 'updated', order_id: 'o-b' },
        { slug: 'c', status: 'uncertain_mutation', order_id: null },
      ],
    );
    expect(receipts.forRow('a', null)).toEqual({ slug: 'a', subtype: null, rank: 0, platinum: 6, quantity: 1, visible: false, action: 'created', orderId: 'o-a', at: 5 });
    // An updated order keeps whatever visibility WFM had, which is unknown here.
    expect(receipts.forRow('b', null)).toMatchObject({ visible: null, action: 'updated' });
    // Interrupted rows are the recovery banner's job, not a claim that they listed.
    expect(receipts.forRow('c', null)).toBeUndefined();
  });

  it('marks only the orders WFM confirmed as visible', () => {
    const receipts = new ListingReceipts();
    receipts.record([{ slug: 'a', platinum: 6, quantity: 1 }, { slug: 'b', platinum: 7, quantity: 1 }],
      [{ slug: 'a', status: 'ok', order_id: 'o-a' }, { slug: 'b', status: 'ok', order_id: 'o-b' }]);
    receipts.markVisible([{ slug: '', status: 'ok', order_id: 'o-a' }, { slug: '', status: 'error', order_id: 'o-b' }]);
    expect(receipts.forRow('a', null)?.visible).toBe(true);
    expect(receipts.forRow('b', null)?.visible).toBe(false);
  });

  it('keeps sibling tiers of one slug apart, and never lends one tier the price of another', () => {
    const receipts = new ListingReceipts();
    receipts.record(
      [{ slug: 'lith_a1_relic', subtype: 'intact', platinum: 2, quantity: 3 }, { slug: 'lith_a1_relic', subtype: 'radiant', platinum: 20, quantity: 1 }],
      [{ slug: 'lith_a1_relic', status: 'ok', action: 'created', order_id: 'intact' }, { slug: 'lith_a1_relic', status: 'error', order_id: null }],
    );
    expect(receipts.forRow('lith_a1_relic', 'intact')).toMatchObject({ quantity: 3, platinum: 2, orderId: 'intact' });
    expect(receipts.forRow('lith_a1_relic', 'radiant')).toBeUndefined();
    expect(receipts.listedFor('lith_a1_relic', 'intact')).toBe(3);
  });

  it('does not guess when results cannot be matched to what was sent', () => {
    const receipts = new ListingReceipts();
    // A batch-level failure answers once for many items: nothing becomes a receipt.
    receipts.record(
      [{ slug: 'x', subtype: 'intact', platinum: 2, quantity: 1 }, { slug: 'x', subtype: 'radiant', platinum: 9, quantity: 1 }],
      [{ slug: 'x', status: 'ok', order_id: 'o' }],
    );
    expect(receipts.forRow('x', 'intact')).toBeUndefined();
    expect(receipts.forRow('x', 'radiant')).toBeUndefined();
  });
});
