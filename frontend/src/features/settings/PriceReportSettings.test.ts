import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import PriceReportSettings from './PriceReportSettings.svelte';
import type { DesktopCapabilities } from '../../contracts/desktop';
import type { PriceReportPreferences } from '../../contracts/price-reports';
afterEach(cleanup);

const off: PriceReportPreferences = { enabled: false, available: true, sent_this_week: 0 };
function transport(overrides: Partial<DesktopCapabilities>): DesktopCapabilities {
  return { getPriceReportPreferences: async () => off, ...overrides } as unknown as DesktopCapabilities;
}

describe('price sharing consent', () => {
  it('writes nothing until an explicit choice and supports withdrawal', async () => {
    const save = vi.fn(async (enabled: boolean) => ({ ...off, enabled }));
    render(PriceReportSettings, { transport: transport({ setPriceReportPreferences: save }) });
    const input = screen.getByRole('checkbox', { name: /Share sale prices/ }) as HTMLInputElement;
    expect(input.checked).toBe(false);
    await waitFor(() => expect(input.disabled).toBe(false));
    expect(save).not.toHaveBeenCalled();
    await fireEvent.click(input); await waitFor(() => expect(save).toHaveBeenCalledWith(true));
    await waitFor(() => expect(input.disabled).toBe(false));
    await fireEvent.click(input); await waitFor(() => expect(save).toHaveBeenLastCalledWith(false));
  });

  it('says how many sales this install shared this week', async () => {
    render(PriceReportSettings, { transport: transport({ getPriceReportPreferences: async () => ({ enabled: true, available: true, sent_this_week: 3 }) }) });
    expect((await screen.findByText(/shared this week/)).textContent).toMatch(/3\s*sales shared this week/);
  });

  it('keeps test launches disabled, including deletion', async () => {
    render(PriceReportSettings, { transport: transport({ getPriceReportPreferences: async () => ({ ...off, available: false }) }) });
    await screen.findByText(/Sharing is disabled in this build/);
    expect((screen.getByRole('checkbox') as HTMLInputElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: 'Delete recent reports' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('does not silently accept a failed save', async () => {
    render(PriceReportSettings, { transport: transport({ setPriceReportPreferences: async () => { throw new Error(); } }) });
    const input = screen.getByRole('checkbox') as HTMLInputElement;
    await waitFor(() => expect(input.disabled).toBe(false));
    await fireEvent.click(input);
    await screen.findByRole('alert');
    expect(input.checked).toBe(false);
  });

  it('confirms a deletion only when the service accepted it', async () => {
    const erase = vi.fn(async () => {});
    render(PriceReportSettings, { transport: transport({ erasePriceReports: erase }) });
    const button = screen.getByRole('button', { name: 'Delete recent reports' }) as HTMLButtonElement;
    await waitFor(() => expect(button.disabled).toBe(false));
    await fireEvent.click(button);
    await screen.findByText('Recent reports deleted from the service.');
    expect(erase).toHaveBeenCalledOnce();
  });

  it('says nothing was removed when deletion fails', async () => {
    render(PriceReportSettings, { transport: transport({ erasePriceReports: async () => { throw new Error(); } }) });
    const button = screen.getByRole('button', { name: 'Delete recent reports' }) as HTMLButtonElement;
    await waitFor(() => expect(button.disabled).toBe(false));
    await fireEvent.click(button);
    expect((await screen.findByRole('alert')).textContent).toMatch(/Nothing was removed/);
    expect(screen.queryByText('Recent reports deleted from the service.')).toBeNull();
  });

  it('states the terms the client keeps', () => {
    render(PriceReportSettings, { transport: transport({}) });
    const terms = screen.getByText(/Never sent:/).textContent ?? '';
    for (const withheld of ['who you traded with', 'time of day', 'inventory']) expect(terms).toContain(withheld);
    expect(screen.getByText(/Trades from before you enabled this are never sent/)).toBeTruthy();
  });
});
