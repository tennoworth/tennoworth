import { afterEach, describe, expect, it } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import SupportPrompt from './SupportPrompt.svelte';
import PriceSharingPrompt from '../settings/PriceSharingPrompt.svelte';
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
const support = () => screen.queryByRole('region', { name: 'Support TennoWorth' });

describe('support prompt', () => {
  const used = { first_seen: T0 - 20 * DAY, launches: 6, prompts: {} };

  it('waits for two weeks and five launches of real use', async () => {
    for (const saved of [{ ...used, first_seen: T0 - 13 * DAY }, { ...used, launches: 3 }]) {
      const s = session(saved);
      s.session.pass('price-sharing-v1');
      render(SupportPrompt, { session: s.session, active: true });
    }
    await new Promise(resolve => setTimeout(resolve, 0));
    expect(support()).toBeNull();
  });

  it('links to Ko-fi and is not asked again after a click', async () => {
    const s = session(used);
    s.session.pass('price-sharing-v1');
    render(SupportPrompt, { session: s.session, active: true });
    const link = await screen.findByRole('link', { name: 'Support on Ko-fi' });
    expect(link.getAttribute('href')).toBe('https://ko-fi.com/prowly');
    expect(screen.getByText(/Ignoring this changes nothing/)).toBeTruthy();
    link.addEventListener('click', event => event.preventDefault());
    await fireEvent.click(link);
    expect(support()).toBeNull();
    expect(s.saved().prompts['support-v1']).toMatchObject({ asks: 1, done: true });
  });

  it('gives way to the price sharing invitation on the same launch', async () => {
    const s = session(used);
    render(SupportPrompt, { session: s.session, active: true });
    render(PriceSharingPrompt, { transport: transport(), session: s.session, active: true, onsettings: () => {} });
    await waitFor(() => expect(invitation()).not.toBeNull());
    expect(support()).toBeNull();
  });

  it('shows once price sharing does not apply', async () => {
    const s = session(used);
    render(SupportPrompt, { session: s.session, active: true });
    render(PriceSharingPrompt, { transport: transport({ getPriceReportPreferences: async () => ({ ...off, enabled: true }) }), session: s.session, active: true, onsettings: () => {} });
    await waitFor(() => expect(support()).not.toBeNull());
  });
});
