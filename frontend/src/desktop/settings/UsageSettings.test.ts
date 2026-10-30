import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import UsageSettings from './UsageSettings.svelte';
import type { DesktopCapabilities } from '../../contracts/desktop';
import checkIn from '../../../../tests/fixtures/usage/check-in.json';
afterEach(cleanup);
describe('usage consent', () => {
  it('sends no preference writes until an explicit choice and supports withdrawal', async () => {
    const save=vi.fn(async (enabled: boolean) => ({ enabled,available:true }));
    render(UsageSettings,{ transport:{ getUsagePreferences:async () => ({enabled:false,available:true}),setUsagePreferences:save } as unknown as DesktopCapabilities });
    const input=screen.getByRole('checkbox',{name:/Share a daily usage count/}) as HTMLInputElement;
    expect(input.checked).toBe(false); expect(save).not.toHaveBeenCalled();
    await waitFor(() => expect(input.disabled).toBe(false));
    await fireEvent.click(input); await waitFor(() => expect(save).toHaveBeenCalledWith(true));
    await waitFor(() => expect(input.disabled).toBe(false));
    await fireEvent.click(input); await waitFor(() => expect(save).toHaveBeenLastCalledWith(false));
  });
  it('states the retry terms the native client uses', () => {
    render(UsageSettings,{ transport:{ getUsagePreferences:async () => ({enabled:false,available:true}) } as unknown as DesktopCapabilities });
    const terms = screen.getByText(/check-in per UTC day/).textContent ?? '';
    expect(checkIn.retry_interval_seconds).toBe(60);
    expect(terms).toContain('once a minute');
    expect(terms).toContain(`at most ${checkIn.max_attempts_per_day} times that day`);
    expect(terms).not.toMatch(/no retries/i);
  });
  it('keeps test launches disabled', async () => {
    render(UsageSettings,{ transport:{ getUsagePreferences:async () => ({enabled:false,available:false}) } as unknown as DesktopCapabilities });
    await screen.findByText(/Sharing is disabled in this build/);
    expect((screen.getByRole('checkbox') as HTMLInputElement).disabled).toBe(true);
  });
  it('does not silently accept failed persistence', async () => {
    render(UsageSettings,{ transport:{ getUsagePreferences:async () => ({enabled:false,available:true}),setUsagePreferences:async () => { throw new Error(); } } as unknown as DesktopCapabilities });
    const input=screen.getByRole('checkbox') as HTMLInputElement;
    await waitFor(() => expect(input.disabled).toBe(false)); await fireEvent.click(input);
    await screen.findByRole('alert'); expect(input.checked).toBe(false);
  });
  it('reflects stopped sharing after a withdrawal could not be persisted', async () => {
    let enabled = true;
    render(UsageSettings, { transport: {
      getUsagePreferences: async () => ({ enabled, available: true }),
      setUsagePreferences: async () => { enabled = false; throw new Error(); },
    } as unknown as DesktopCapabilities });
    const input = screen.getByRole('checkbox') as HTMLInputElement;
    await waitFor(() => expect(input.checked).toBe(true));
    await fireEvent.click(input);
    await screen.findByRole('alert');
    expect(input.checked).toBe(false);
  });

});
