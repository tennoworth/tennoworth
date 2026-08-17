// Ledger panel: renders trades + totals from the desktop, the "log not found"
// state, and the auto-close toggle writes through the provided callback.
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
import LedgerPanel from './LedgerPanel.svelte';
import { installTauri, removeTauri } from '../lib/test-utils';

afterEach(() => { cleanup(); removeTauri(); });

const NOW = Math.floor(Date.now() / 1000);
const trades = [
  { id: 2, at: NOW - 60, partner: 'Buyer', kind: 'sale', plat: 45, items: [{ name: 'Primed Flow', qty: 1, direction: 'given' }], log_stamp: null, wfm_closed: true },
  { id: 1, at: NOW - 30 * 86400, partner: 'Seller', kind: 'purchase', plat: 20, items: [{ name: 'Ash Prime Blueprint', qty: 2, direction: 'received' }], log_stamp: null, wfm_closed: false },
];

function install(status: { path: string | null; auto_close: boolean }, rows = trades) {
  const invoke = vi.fn(async (cmd: string) => {
    if (cmd === 'list_trades') return rows;
    if (cmd === 'eelog_status') return status;
    throw new Error(`unexpected ${cmd}`);
  });
  installTauri(invoke, undefined);
  return invoke;
}

describe('LedgerPanel', () => {
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
