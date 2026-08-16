// Event-wiring tests for the listing review modal: the submit flow, the
// lock-state routing (needs_login / needs_unlock -> auth dialogs), and the
// bulk-visibility toggle. The lib layer (limits, format) is covered by its own
// tests; this file drives the component itself, which the lib tests can't.
import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
import { afterEach } from 'vitest';
import ListingReviewModal from './ListingReviewModal.svelte';
import { DesktopCmdError, type Transport } from '../lib/transport';
import { installTauri, removeTauri } from '../lib/test-utils';

// globals: false in vitest.config.ts, so testing-library's auto-cleanup
// does not run; unmount between tests or buttons accumulate in the DOM.
afterEach(cleanup);

const rows = [
  { slug: 'accelerated_blast', name: 'Accelerated Blast', owned: 3, sellable: 2, low_sell: 15, avg_price: 20, clearing_price: 18 },
  { slug: 'ash_prime_blueprint', name: 'Ash Prime Blueprint', owned: 1, sellable: 1, low_sell: 40, avg_price: 45, clearing_price: 42 },
];

function makeTransport(overrides: Partial<Transport> = {}) {
  return {
    submitPlan: vi.fn().mockResolvedValue({ plan_id: 'p1', results: [] }),
    bulkVisibility: vi.fn().mockResolvedValue({ results: [] }),
    ...overrides,
  } as unknown as Transport;
}

function openModal(overrides: Record<string, unknown> = {}) {
  return render(ListingReviewModal, {
    props: { open: true, rows, transport: makeTransport(), ...overrides },
  });
}

describe('ListingReviewModal', () => {
  it('prefills rows on open with the clearing-price clamp and an enabled Send', () => {
    openModal();
    expect(screen.getByText('Accelerated Blast')).toBeTruthy();
    // clearing price 18, not raw low_sell 15, not avg 20
    expect(screen.getByDisplayValue('18')).toBeTruthy();
    const send = screen.getByRole('button', { name: /Send 2 listings/ }) as HTMLButtonElement;
    expect(send.disabled).toBe(false);
  });

  it('deselect-all disables Send; select-all re-enables it', async () => {
    openModal();
    await fireEvent.click(screen.getByText('Deselect all'));
    await waitFor(() => {
      const send = screen.getByRole('button', { name: /Send 0 listings/ }) as HTMLButtonElement;
      expect(send.disabled).toBe(true);
    });
    await fireEvent.click(screen.getByText('Select all'));
    await waitFor(() => {
      const send = screen.getByRole('button', { name: /Send 2 listings/ }) as HTMLButtonElement;
      expect(send.disabled).toBe(false);
    });
  });

  it('sends the clamped payload, then bulk-visibility toggles created orders', async () => {
    const transport = makeTransport({
      submitPlan: vi.fn().mockResolvedValue({
        plan_id: 'p1',
        results: [
          { slug: 'accelerated_blast', status: 'ok', action: 'created', order_id: 'abc123' },
          { slug: 'ash_prime_blueprint', status: 'ok', action: 'created', order_id: 'def456' },
        ],
      }),
      bulkVisibility: vi.fn().mockResolvedValue({
        results: [
          { slug: 'accelerated_blast', status: 'ok' },
          { slug: 'ash_prime_blueprint', status: 'ok' },
        ],
      }),
    });
    openModal({ transport });
    await fireEvent.click(screen.getByRole('button', { name: /Send 2 listings/ }));
    await waitFor(() => expect(screen.getByText('2 created')).toBeTruthy());
    expect(transport.submitPlan).toHaveBeenCalledWith(
      expect.arrayContaining([
        expect.objectContaining({ slug: 'accelerated_blast', platinum: 18, quantity: 1, order_type: 'sell', visible: false }),
        expect.objectContaining({ slug: 'ash_prime_blueprint', platinum: 42 }),
      ]),
    );
    await fireEvent.click(screen.getByRole('button', { name: /Make 2 visible/ }));
    await waitFor(() =>
      expect(transport.bulkVisibility).toHaveBeenCalledWith(['abc123', 'def456'], true),
    );
    await waitFor(() => expect(screen.getByText('2 now visible')).toBeTruthy());
  });

  it('routes a needs_login rejection to onauthrequired and returns to review', async () => {
    const onauthrequired = vi.fn();
    const transport = makeTransport({
      submitPlan: vi.fn().mockRejectedValue(new DesktopCmdError('needs_login', 'not signed in')),
    });
    openModal({ transport, onauthrequired });
    await fireEvent.click(screen.getByRole('button', { name: /Send 2 listings/ }));
    await waitFor(() => expect(onauthrequired).toHaveBeenCalledWith('needs_login'));
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /Send 2 listings/ })).toBeTruthy(),
    );
  });

  it('a plain failure surfaces humanError and offers Back to review', async () => {
    const transport = makeTransport({
      submitPlan: vi.fn().mockRejectedValue(new Error('connection refused')),
    });
    openModal({ transport });
    await fireEvent.click(screen.getByRole('button', { name: /Send 2 listings/ }));
    await waitFor(() => expect(screen.getByText('connection refused')).toBeTruthy());
    await fireEvent.click(screen.getByText('Back to review'));
    expect(screen.getByRole('button', { name: /Send 2 listings/ })).toBeTruthy();
  });

  describe('live prices (desktop only)', () => {
    afterEach(removeTauri);

    it('is absent in the hosted build', () => {
      openModal();
      expect(screen.queryByRole('button', { name: /Check live prices/ })).toBeNull();
    });

    it('asks the desktop for each selected row\'s exact tier, renders ask/bid, and one click matches the ask', async () => {
      const invoke = vi.fn(async (cmd: string, args: { queries: Array<{ slug: string; rank: number; subtype: string | null }> }) => {
        expect(cmd).toBe('live_top_prices');
        return args.queries.map((q) => ({
          slug: q.slug, rank: q.rank, subtype: q.subtype,
          sells: q.slug === 'accelerated_blast' ? [12, 14, 15] : [],
          buys: q.slug === 'accelerated_blast' ? [9] : [30],
          low_sell: q.slug === 'accelerated_blast' ? 12 : null,
          top_buy: q.slug === 'accelerated_blast' ? 9 : 30,
          error: null,
        }));
      });
      installTauri(invoke, undefined);
      openModal();
      const btn = screen.getByRole('button', { name: /Check live prices/ });
      await fireEvent.click(btn);
      await waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));
      // exact-tier queries: rank 0 default, no subtype
      expect(invoke.mock.calls[0][1].queries).toEqual([
        { slug: 'accelerated_blast', rank: 0, subtype: null },
        { slug: 'ash_prime_blueprint', rank: 0, subtype: null },
      ]);
      // ask rendered as a clickable price; the row with no online seller says so
      const ask = await screen.findByRole('button', { name: '12p' });
      expect(screen.getByText('no ask')).toBeTruthy();
      // the prefilled 18p is above the 12p live ask → warned; click matches it
      expect(screen.getByDisplayValue('18')).toBeTruthy();
      await fireEvent.click(ask);
      expect(screen.getByDisplayValue('12')).toBeTruthy();
      // and the bulk "match lowest asks" control appeared
      expect(screen.getByRole('button', { name: /Match lowest asks/ })).toBeTruthy();
    });

    it('surfaces a desktop error without losing the review', async () => {
      const invoke = vi.fn(async () => { throw { code: 'wfm', message: 'HTTP 503' }; });
      installTauri(invoke, undefined);
      openModal();
      await fireEvent.click(screen.getByRole('button', { name: /Check live prices/ }));
      await screen.findByText(/HTTP 503/);
      expect(screen.getByRole('button', { name: /Send 2 listings/ })).toBeTruthy();
    });
  });
});
