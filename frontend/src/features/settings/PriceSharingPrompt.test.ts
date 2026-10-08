import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import PriceSharingPrompt from './PriceSharingPrompt.svelte';
import type { DesktopCapabilities } from '../../contracts/desktop';
import type { PriceReportPreferences } from '../../contracts/price-reports';
import type { SettingsStore } from '../../contracts/state-store';
afterEach(cleanup);

const off: PriceReportPreferences = { enabled: false, available: true, sent_this_week: 0 };
function transport(overrides: Partial<DesktopCapabilities> = {}): DesktopCapabilities {
  return { getPriceReportPreferences: async () => off, setPriceReportPreferences: async (enabled: boolean) => ({ ...off, enabled }), ...overrides } as unknown as DesktopCapabilities;
}
function memory(dismissed: string | null = null) {
  const values = new Map<string, string>(dismissed ? [['dismissed-prompts', dismissed]] : []);
  const store: SettingsStore = { mode: 'local', hydrate: async () => {}, getSetting: key => values.get(key) ?? null, setSetting: async (key, value) => { values.set(key, value); } };
  return { store, values };
}
const invitation = () => screen.queryByRole('region', { name: 'Help show what items really sell for' });

describe('price sharing prompt', () => {
  it('invites without changing anything until the user accepts', async () => {
    const save = vi.fn(async (enabled: boolean) => ({ ...off, enabled }));
    const { store, values } = memory();
    render(PriceSharingPrompt, { transport: transport({ setPriceReportPreferences: save }), store, active: true, onsettings: () => {} });
    await waitFor(() => expect(invitation()).not.toBeNull());
    expect(screen.getByText(/Ignoring this changes nothing/)).toBeTruthy();
    expect(save).not.toHaveBeenCalled();
    expect(values.size).toBe(0);
  });

  it('turns sharing on, confirms it and does not ask again', async () => {
    const save = vi.fn(async (enabled: boolean) => ({ ...off, enabled }));
    const { store, values } = memory();
    render(PriceSharingPrompt, { transport: transport({ setPriceReportPreferences: save }), store, active: true, onsettings: () => {} });
    await fireEvent.click(await screen.findByRole('button', { name: 'Share sale prices' }));
    expect(save).toHaveBeenCalledWith(true);
    await screen.findByText(/Trades you complete from now on are shared/);
    expect(values.get('dismissed-prompts')).toBe('["price-sharing-v1"]');
    await fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(screen.queryByRole('region')).toBeNull();
  });

  it('keeps the invitation and says nothing changed when saving fails', async () => {
    const { store, values } = memory();
    render(PriceSharingPrompt, { transport: transport({ setPriceReportPreferences: async () => { throw new Error(); } }), store, active: true, onsettings: () => {} });
    await fireEvent.click(await screen.findByRole('button', { name: 'Share sale prices' }));
    expect((await screen.findByRole('alert')).textContent).toMatch(/Nothing was changed/);
    expect(invitation()).not.toBeNull();
    expect(values.size).toBe(0);
  });

  it('does not claim success when the service keeps sharing off', async () => {
    const { store, values } = memory();
    render(PriceSharingPrompt, { transport: transport({ setPriceReportPreferences: async () => off }), store, active: true, onsettings: () => {} });
    await fireEvent.click(await screen.findByRole('button', { name: 'Share sale prices' }));
    expect((await screen.findByRole('alert')).textContent).toMatch(/could not be turned on/);
    expect(screen.queryByText(/Trades you complete from now on are shared/)).toBeNull();
    expect(values.size).toBe(0);
  });

  it('steps aside while the shell is busy and returns afterwards', async () => {
    const view = render(PriceSharingPrompt, { transport: transport(), store: memory().store, active: true, onsettings: () => {} });
    await waitFor(() => expect(invitation()).not.toBeNull());
    await view.rerender({ active: false });
    expect(invitation()).toBeNull();
    await view.rerender({ active: true });
    await waitFor(() => expect(invitation()).not.toBeNull());
  });

  it('remembers Not now', async () => {
    const { store, values } = memory();
    render(PriceSharingPrompt, { transport: transport(), store, active: true, onsettings: () => {} });
    await fireEvent.click(await screen.findByRole('button', { name: 'Not now' }));
    expect(invitation()).toBeNull();
    expect(values.get('dismissed-prompts')).toBe('["price-sharing-v1"]');
  });

  it('opens the full terms in Settings', async () => {
    const onsettings = vi.fn();
    render(PriceSharingPrompt, { transport: transport(), store: memory().store, active: true, onsettings });
    await fireEvent.click(await screen.findByRole('button', { name: 'What is sent' }));
    expect(onsettings).toHaveBeenCalledOnce();
  });

  it.each([
    ['already declined', { dismissed: '["price-sharing-v1"]' }],
    ['already sharing', { prefs: { ...off, enabled: true } }],
    ['unavailable in this build', { prefs: { ...off, available: false } }],
    ['inactive', { active: false }],
    ['unreadable preference', { fail: true }],
  ])('stays hidden when %s', async (_name, c: { dismissed?: string; prefs?: PriceReportPreferences; active?: boolean; fail?: boolean }) => {
    const read = vi.fn(async () => { if (c.fail) throw new Error(); return c.prefs ?? off; });
    render(PriceSharingPrompt, { transport: transport({ getPriceReportPreferences: read }), store: memory(c.dismissed).store, active: c.active ?? true, onsettings: () => {} });
    if (c.active !== false) await waitFor(() => expect(read).toHaveBeenCalled());
    await new Promise(resolve => setTimeout(resolve, 0));
    expect(screen.queryByRole('region')).toBeNull();
  });
});
