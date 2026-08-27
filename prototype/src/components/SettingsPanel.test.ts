// Settings view: the Appearance section renders the mode control, drives the
// ThemeController, and explains what System does. This is the ONLY place the
// theme can be changed inside the shell now, so a regression here leaves a
// user with no way to override the OS scheme in the app.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/svelte';
import SettingsPanel from './SettingsPanel.svelte';
import type { ModePref, ThemeController } from '../lib/theme';
import type { Transport } from '../lib/transport';
import type { OverlaySettings } from '../lib/types';

afterEach(cleanup);

beforeEach(() => {
  // jsdom has no matchMedia; ThemeSwitcher subscribes to it on mount.
  vi.stubGlobal('matchMedia', () => ({
    matches: false,
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
});

function fakeTheme(pref: ModePref = 'system') {
  const setModePref = vi.fn();
  return {
    theme: { modePref: pref, setModePref, destroy: () => {} } as unknown as ThemeController,
    setModePref,
  };
}

describe('SettingsPanel', () => {
  it('renders the Appearance section with the three modes and the System note', () => {
    const { theme } = fakeTheme();
    render(SettingsPanel, { props: { theme } });
    expect(screen.getByRole('heading', { name: 'Settings' })).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Appearance' })).toBeTruthy();
    for (const label of ['Light', 'Dark', 'System']) {
      expect(screen.getByRole('radio', { name: label })).toBeTruthy();
    }
    expect(screen.getByText(/System follows your operating system/)).toBeTruthy();
  });

  it('marks the stored preference and writes a change through the controller', async () => {
    const { theme, setModePref } = fakeTheme('system');
    render(SettingsPanel, { props: { theme } });
    expect(screen.getByRole('radio', { name: 'System' }).getAttribute('aria-checked')).toBe('true');
    await fireEvent.click(screen.getByRole('radio', { name: 'Dark' }));
    expect(setModePref).toHaveBeenCalledWith('dark');
    expect(screen.getByRole('radio', { name: 'Dark' }).getAttribute('aria-checked')).toBe('true');
    expect(screen.getByRole('radio', { name: 'System' }).getAttribute('aria-checked')).toBe('false');
  });

  it('there is no look picker left — the mode is the only choice', () => {
    const { theme } = fakeTheme();
    render(SettingsPanel, { props: { theme } });
    expect(screen.queryByRole('radiogroup', { name: 'Look' })).toBeNull();
    expect(screen.getAllByRole('radio')).toHaveLength(3);
  });

  it('requires an explicit desktop opt-in and persists it through the transport', async () => {
    const { theme } = fakeTheme();
    const settings: OverlaySettings = {
      enabled: false, autoDetect: true, shortcut: 'Ctrl+Shift+O', scale: 1,
      livePrices: true, showOwned: true,
      diagnostics: false,
    };
    const updateOverlaySettings = vi.fn(async (next: OverlaySettings) => next);
    const transport = {
      getOverlaySettings: vi.fn(async () => settings),
      updateOverlaySettings,
      overlayStatus: vi.fn(async () => ({ state: 'disabled', backend: 'x11-window', placement: 'anchored', ocrReady: true })),
      setupOverlayCapture: vi.fn(async () => ({ state: 'watching', backend: 'x11-window', placement: 'anchored', ocrReady: true })),
      scanOverlayNow: vi.fn(async () => {}),
    } as unknown as Transport;
    render(SettingsPanel, { props: { theme, transport, isDesktop: true } });

    const consent = await screen.findByRole('checkbox', { name: /Enable local screen recognition/ });
    expect((consent as HTMLInputElement).checked).toBe(false);
    await fireEvent.click(consent);
    expect(updateOverlaySettings).toHaveBeenCalledWith({ ...settings, enabled: true });

    const diagnostics = screen.getByRole('checkbox', { name: /Save local recognition diagnostics/ });
    await fireEvent.click(diagnostics);
    expect(await screen.findByText(/Diagnostic captures may contain player or game information/)).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Open diagnostics' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Clear diagnostics' })).toBeTruthy();
  });
});
