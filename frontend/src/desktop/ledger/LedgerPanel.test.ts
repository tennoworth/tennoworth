// Ledger panel: renders trades + totals from the desktop, the "log not found"
// state, and the auto-close toggle writes through the provided callback.
import { describe, it, expect, vi, afterEach } from 'vitest';
import { screen, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
import { renderDesktop as render } from '../../dev/preview/render-desktop';
import LedgerPanel from './LedgerPanel.svelte';
import { installTauri, removeTauri } from '../../dev/testing/test-utils';
import { RECORDING_CHANGED_EVENT, TRADE_DETECTED_EVENT } from '../../contracts/events';
import type { ListingOutcomes } from '../../contracts/desktop';

afterEach(() => { cleanup(); removeTauri(); vi.restoreAllMocks(); });

const NOW = Math.floor(Date.now() / 1000);
const trades = [
  { id: 2, at: NOW - 60, partner: 'Buyer', kind: 'sale', plat: 45, items: [{ name: 'Primed Flow', qty: 1, direction: 'given' }], log_stamp: null, wfm_closed: true },
  { id: 1, at: NOW - 30 * 86400, partner: 'Seller', kind: 'purchase', plat: 20, items: [{ name: 'Ash Prime Blueprint', qty: 2, direction: 'received' }], log_stamp: null, wfm_closed: false },
];

type Recording = 'off' | 'recording' | { paused: { ledger: { error: string } } | { log: { error: string } } };

function install(
  status: { path: string | null; auto_close: boolean; recording?: Recording },
  rows = trades,
) {
  if (status.recording === undefined) status = { ...status, recording: 'recording' };
  const invoke = vi.fn(async (cmd: string) => {
    if (cmd === 'list_trades') return rows;
    if (cmd === 'eelog_status') return status;
    throw new Error(`unexpected ${cmd}`);
  });
  installTauri(invoke, undefined);
  return invoke;
}

describe('LedgerPanel', () => {
  it.each(['success', 'failure'])('ignores an older load %s after a recording update', async outcome => {
    let resolve!: (value: typeof trades) => void;
    let reject!: (error: Error) => void;
    const old = new Promise<typeof trades>((yes, no) => { resolve = yes; reject = no; });
    const handlers: Record<string, (event: { payload: unknown }) => void> = {};
    let loads = 0;
    installTauri(vi.fn(async (cmd: string) => {
      if (cmd === 'list_trades') return ++loads === 1 ? old : trades;
      if (cmd === 'eelog_status') return { path: '/x/EE.log', auto_close: loads > 1, recording: 'recording' };
      throw new Error(`unexpected ${cmd}`);
    }), vi.fn((name, handler) => { handlers[name] = handler; return Promise.resolve(() => {}); }));
    render(LedgerPanel, { props: {} });
    await waitFor(() => expect(loads).toBe(1));
    handlers[RECORDING_CHANGED_EVENT]({ payload: null });
    await screen.findByText('Primed Flow', { selector: 'td' });
    if (outcome === 'success') resolve([]);
    else reject(new Error('Stale ledger failure'));
    await new Promise(done => setTimeout(done, 0));
    expect(screen.getByText('Primed Flow', { selector: 'td' })).toBeTruthy();
    expect((screen.getByRole('checkbox') as HTMLInputElement).checked).toBe(true);
    expect(screen.queryByText(/Stale ledger failure/)).toBeNull();
  });

  it('shows totals, rows and the listing-updated marker', async () => {
    install({ path: '/x/EE.log', auto_close: true });
    render(LedgerPanel, { props: {} });
    await screen.findByText('+25p');          // all-time net 45 − 20
    expect(screen.getByText('+45p', { selector: 'td' })).toBeTruthy();
    expect(screen.getByText('−20p')).toBeTruthy();
    expect(screen.getByText('listing updated')).toBeTruthy();
    expect(screen.getByText('Ash Prime Blueprint ×2')).toBeTruthy();
  });

  it('explains when the game log was not found and disables the toggle', async () => {
    install({ path: null, auto_close: true }, []);
    render(LedgerPanel, { props: {} });
    await screen.findByText(/Game log not found/);
    expect((screen.getByRole('checkbox') as HTMLInputElement).disabled).toBe(true);
    expect(screen.getByText(/No trades recorded yet/)).toBeTruthy();
  });

  it('says recording is paused, and why, when the ledger refused a trade', async () => {
    install({
      path: '/x/EE.log',
      auto_close: true,
      recording: { paused: { ledger: { error: 'database is locked' } } },
    });
    render(LedgerPanel, { props: {} });
    await screen.findByText(/Trade recording paused/);
    // The consequence and the cause both have to be on screen: the user needs to
    // know their trades are missing, and what to do about it.
    expect(screen.getByText(/not in the history below yet/)).toBeTruthy();
    expect(screen.getByText(/database is locked/)).toBeTruthy();
  });

  it('names the log, not the ledger, when the log itself could not be read', async () => {
    install({
      path: '/x/EE.log',
      auto_close: true,
      recording: { paused: { log: { error: 'the game log could not be read' } } },
    });
    render(LedgerPanel, { props: {} });
    await screen.findByText(/Trade recording paused/);
    expect(screen.getByText(/may not be recorded/)).toBeTruthy();
    expect(screen.queryByText(/not in the history below yet/)).toBeNull();
  });

  it('shows no pause notice while recording is healthy', async () => {
    install({ path: '/x/EE.log', auto_close: true, recording: 'recording' });
    render(LedgerPanel, { props: {} });
    await screen.findByText('+25p');
    expect(screen.queryByText(/Trade recording paused/)).toBeNull();
  });

  it('the auto-close toggle writes through the callback', async () => {
    install({ path: '/x/EE.log', auto_close: true });
    const onsetautoclose = vi.fn().mockResolvedValue(undefined);
    render(LedgerPanel, { props: { onsetautoclose } });
    const box = (await screen.findByRole('checkbox')) as HTMLInputElement;
    expect(box.checked).toBe(true);
    await fireEvent.click(box);
    await waitFor(() => expect(onsetautoclose).toHaveBeenCalledWith(false));
  });
});

describe('LedgerPanel listing outcomes', () => {
  const none = { listings: 0, quantity_listed: 0, quantity_sold: 0, within_day: { eligible: 0, sold: 0 }, within_week: { eligible: 0, sold: 0 }, median_hours_to_first_sale: null, price_ratio: null, priced_quantity: 0 };
  const sample: ListingOutcomes = {
    enabled: true, window_days: 30,
    session: { listings: 4, quantity_listed: 10, quantity_sold: 6, within_day: { eligible: 4, sold: 2 }, within_week: { eligible: 3, sold: 3 }, median_hours_to_first_sale: 5.2, price_ratio: 0.95, priced_quantity: 5 },
    manual: none,
  };

  function installOutcomes(initial: ListingOutcomes | Error) {
    let current = initial;
    const invoke = vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === 'list_trades') return trades;
      if (cmd === 'eelog_status') return { path: '/x/EE.log', auto_close: true, recording: 'recording' };
      if (cmd === 'listing_outcomes') { if (current instanceof Error) throw current; return current; }
      if (cmd === 'set_listing_outcomes') { current = { ...(current as ListingOutcomes), enabled: Boolean(args?.enabled), session: none, manual: none }; return null; }
      throw new Error(`unexpected ${cmd}`);
    });
    installTauri(invoke, undefined);
    return invoke;
  }

  it('shows listing outcomes open, as meters, omitting a group with none', async () => {
    installOutcomes(sample);
    render(LedgerPanel, { props: {} });
    const block = await screen.findByTestId('listing-outcomes');
    expect((block as HTMLDetailsElement).open).toBe(true);
    expect(screen.getByRole('meter', { name: 'Copies sold' }).getAttribute('aria-valuetext')).toBe('6 of 10');
    expect(screen.getByRole('meter', { name: 'Sold in a week' }).getAttribute('aria-valuetext')).toBe('3 of 3');
    expect(block.textContent).toContain('4 listings in 30 days · 3 of 3 sold within a week');
    expect(screen.getByText('Trade Session')).toBeTruthy();
    expect(screen.queryByText('Other listings')).toBeNull();
    expect(screen.getByText('6 of 10')).toBeTruthy();
    expect(screen.getByText('5 h')).toBeTruthy();
    expect(screen.getByText('95%')).toBeTruthy();
  });

  it('turning the record off saves it and shows it as off', async () => {
    const invoke = installOutcomes(sample);
    render(LedgerPanel, { props: {} });
    const box = (await screen.findByLabelText(/Keep a record of listings/)) as HTMLInputElement;
    expect(box.checked).toBe(true);
    await fireEvent.click(box);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('set_listing_outcomes', { enabled: false }));
    await waitFor(() => expect(screen.getByTestId('listing-outcomes').textContent).toContain('· off'));
    expect(screen.queryByText('Trade Session')).toBeNull();
  });

  it('a failed outcome read leaves the ledger working and shows no block', async () => {
    installOutcomes(new Error('no such table'));
    render(LedgerPanel, { props: {} });
    await screen.findByText('+25p');
    expect(screen.queryByTestId('listing-outcomes')).toBeNull();
    expect(screen.queryByText(/no such table/)).toBeNull();
  });
});

it.each(['manual', 'destroy'])('cancels a ledger toast timer on %s dismissal', async dismissal => {
  const handlers: Record<string, (event: { payload: unknown }) => void> = {};
  installTauri(vi.fn(async (cmd: string) => {
    if (cmd === 'list_trades') return trades;
    if (cmd === 'eelog_status') return { path: '/x/EE.log', auto_close: true, recording: 'recording' };
    throw new Error(`unexpected ${cmd}`);
  }), vi.fn((name, handler) => { handlers[name] = handler; return Promise.resolve(() => {}); }));
  const view = render(LedgerPanel, { props: {} });
  await waitFor(() => expect(handlers[TRADE_DETECTED_EVENT]).toBeDefined());
  const scheduled = vi.spyOn(window, 'setTimeout');
  const cancelled = vi.spyOn(window, 'clearTimeout');
  handlers[TRADE_DETECTED_EVENT]({ payload: { id: 3, trade: trades[0], adjusted: [] } });
  await screen.findByText(/Sold for 45p:/);
  const index = scheduled.mock.calls.findIndex(call => call[1] === 5000);
  expect(index).toBeGreaterThanOrEqual(0);
  const timer = scheduled.mock.results[index].value;
  if (dismissal === 'manual') await fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
  else view.unmount();
  expect(cancelled).toHaveBeenCalledWith(timer);
});
