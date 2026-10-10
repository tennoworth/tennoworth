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
    expect(receipts.get('a')).toEqual({ slug: 'a', platinum: 6, quantity: 1, visible: false, action: 'created', orderId: 'o-a', at: 5 });
    // An updated order keeps whatever visibility WFM had, which is unknown here.
    expect(receipts.get('b')).toMatchObject({ visible: null, action: 'updated' });
    // Interrupted rows are the recovery banner's job, not a claim that they listed.
    expect(receipts.get('c')).toBeUndefined();
  });

  it('marks only the orders WFM confirmed as visible', () => {
    const receipts = new ListingReceipts();
    receipts.record([{ slug: 'a', platinum: 6, quantity: 1 }, { slug: 'b', platinum: 7, quantity: 1 }],
      [{ slug: 'a', status: 'ok', order_id: 'o-a' }, { slug: 'b', status: 'ok', order_id: 'o-b' }]);
    receipts.markVisible([{ slug: '', status: 'ok', order_id: 'o-a' }, { slug: '', status: 'error', order_id: 'o-b' }]);
    expect(receipts.get('a')?.visible).toBe(true);
    expect(receipts.get('b')?.visible).toBe(false);
  });
});
