import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import PriceSharingPrompt from './PriceSharingPrompt.svelte';
import type { DesktopCapabilities } from '../../contracts/desktop';
import type { PriceReportPreferences } from '../../contracts/price-reports';
import type { SettingsStore } from '../../contracts/state-store';
import { PromptSession } from '../../ui/prompt-session.svelte';
import { PROMPT_ORDER } from '../prompt-policies';
afterEach(cleanup);

const DAY = 86_400_000;
const T0 = Date.UTC(2026, 9, 1);
const off: PriceReportPreferences = { enabled: false, available: true, sent_this_week: 0 };
function transport(overrides: Partial<DesktopCapabilities> = {}): DesktopCapabilities {
  return { getPriceReportPreferences: async () => off, setPriceReportPreferences: async (enabled: boolean) => ({ ...off, enabled }), ...overrides } as unknown as DesktopCapabilities;
}
function session(saved: object | null = null, now = T0) {
  const values = new Map<string, string>(saved ? [['prompts', JSON.stringify(saved)]] : []);
  const store: SettingsStore = { mode: 'local', hydrate: async () => {}, getSetting: key => values.get(key) ?? null, setSetting: async (key, value) => { values.set(key, value); } };
  const s = new PromptSession(store, PROMPT_ORDER, () => now);
  s.launch();
  return { session: s, saved: () => JSON.parse(values.get('prompts') ?? '{}') };
}
const invitation = () => screen.queryByRole('region', { name: 'Help show what items really sell for' });

describe('price sharing prompt', () => {
  it('invites without changing anything until the user accepts', async () => {
    const save = vi.fn(async (enabled: boolean) => ({ ...off, enabled }));
    const s = session();
    render(PriceSharingPrompt, { transport: transport({ setPriceReportPreferences: save }), session: s.session, active: true, onsettings: () => {} });
    await waitFor(() => expect(invitation()).not.toBeNull());
    expect(screen.getByText(/Ignoring this changes nothing/)).toBeTruthy();
    expect(save).not.toHaveBeenCalled();
    expect(s.saved().prompts['price-sharing-v1']).toEqual({ asks: 1, last_ask: T0 });
  });

  it('turns sharing on, confirms it and retires the prompt', async () => {
    const save = vi.fn(async (enabled: boolean) => ({ ...off, enabled }));
    const s = session();
    render(PriceSharingPrompt, { transport: transport({ setPriceReportPreferences: save }), session: s.session, active: true, onsettings: () => {} });
    await fireEvent.click(await screen.findByRole('button', { name: 'Share sale prices' }));
    expect(save).toHaveBeenCalledWith(true);
    await screen.findByText(/Trades you complete from now on are shared/);
    expect(s.saved().prompts['price-sharing-v1'].done).toBe(true);
    await fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(screen.queryByRole('region')).toBeNull();
  });

  it('keeps the invitation and says nothing changed when saving fails', async () => {
    const s = session();
    render(PriceSharingPrompt, { transport: transport({ setPriceReportPreferences: async () => { throw new Error(); } }), session: s.session, active: true, onsettings: () => {} });
    await fireEvent.click(await screen.findByRole('button', { name: 'Share sale prices' }));
    expect((await screen.findByRole('alert')).textContent).toMatch(/Nothing was changed/);
    expect(invitation()).not.toBeNull();
    expect(s.saved().prompts['price-sharing-v1'].done).toBeUndefined();
  });

  it('does not claim success when the service keeps sharing off', async () => {
    render(PriceSharingPrompt, { transport: transport({ setPriceReportPreferences: async () => off }), session: session().session, active: true, onsettings: () => {} });
    await fireEvent.click(await screen.findByRole('button', { name: 'Share sale prices' }));
    expect((await screen.findByRole('alert')).textContent).toMatch(/could not be turned on/);
    expect(screen.queryByText(/Trades you complete from now on are shared/)).toBeNull();
  });

  it('steps aside while the shell is busy and returns afterwards', async () => {
    const view = render(PriceSharingPrompt, { transport: transport(), session: session().session, active: true, onsettings: () => {} });
    await waitFor(() => expect(invitation()).not.toBeNull());
    await view.rerender({ active: false });
    expect(invitation()).toBeNull();
    await view.rerender({ active: true });
    await waitFor(() => expect(invitation()).not.toBeNull());
  });

  it('Not now hides it for this launch', async () => {
    render(PriceSharingPrompt, { transport: transport(), session: session().session, active: true, onsettings: () => {} });
    await fireEvent.click(await screen.findByRole('button', { name: 'Not now' }));
    expect(invitation()).toBeNull();
  });

  it('opens the full terms in Settings', async () => {
    const onsettings = vi.fn();
    render(PriceSharingPrompt, { transport: transport(), session: session().session, active: true, onsettings });
    await fireEvent.click(await screen.findByRole('button', { name: 'What is sent' }));
    expect(onsettings).toHaveBeenCalledOnce();
  });

  it.each([
    ['asked inside the last fortnight', { saved: { first_seen: T0, launches: 3, last_ask: T0 - 13 * DAY, prompts: { 'price-sharing-v1': { asks: 1, last_ask: T0 - 13 * DAY } } } }],
    ['already sharing', { prefs: { ...off, enabled: true } }],
    ['unavailable in this build', { prefs: { ...off, available: false } }],
    ['inactive', { active: false }],
    ['unreadable preference', { fail: true }],
  ])('stays hidden when %s', async (_name, c: { saved?: object; prefs?: PriceReportPreferences; active?: boolean; fail?: boolean }) => {
    const read = vi.fn(async () => { if (c.fail) throw new Error(); return c.prefs ?? off; });
    render(PriceSharingPrompt, { transport: transport({ getPriceReportPreferences: read }), session: session(c.saved ?? null).session, active: c.active ?? true, onsettings: () => {} });
    if (c.active !== false) await waitFor(() => expect(read).toHaveBeenCalled());
    await new Promise(resolve => setTimeout(resolve, 0));
    expect(screen.queryByRole('region')).toBeNull();
  });
});
