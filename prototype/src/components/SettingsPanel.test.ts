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
import { installTauri, removeTauri } from '../lib/test-utils';

afterEach(() => {
  cleanup();
  removeTauri();
});

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
      overlayStatus: vi.fn(async () => ({ state: 'disabled', backend: 'x11-window', presentationBackend: 'tauri-window', placement: 'anchored', ocrReady: true })),
      setupOverlayCapture: vi.fn(async () => ({ state: 'watching', backend: 'x11-window', presentationBackend: 'tauri-window', placement: 'anchored', ocrReady: true })),
      previewRelicOverlay: vi.fn(async () => {}),
      scanOverlayNow: vi.fn(async () => {}),
      openOverlayDiagnostics: vi.fn(async () => {}),
      clearOverlayDiagnostics: vi.fn(async () => {}),
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

  it('checks for desktop updates on demand and reports the current version', async () => {
    const { theme } = fakeTheme();
    const invoke = vi.fn(async (command: string) => {
      if (command === 'check_update') {
        return { checked: true, available: false, support: 'supported', current_version: '0.6.1', version: null, notes: null };
      }
      throw new Error(`unexpected command: ${command}`);
    });
    installTauri(invoke, undefined);
    render(SettingsPanel, { props: { theme, isDesktop: true } });

    await fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
    expect(invoke).toHaveBeenCalledWith('check_update');
    expect(await screen.findByText('You’re up to date · v0.6.1')).toBeTruthy();
  });

  it('does not call an unsupported non-AppImage Linux install up to date', async () => {
    const { theme } = fakeTheme();
    installTauri(vi.fn().mockResolvedValue({
      checked: true,
      available: false,
      support: 'appimage_required',
      current_version: '0.6.1',
      version: null,
      notes: null,
    }), undefined);
    render(SettingsPanel, { props: { theme, isDesktop: true } });

    await fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
    expect(await screen.findByText(/This install can’t update itself/)).toBeTruthy();
    expect(screen.queryByText(/You’re up to date/)).toBeNull();
  });

  it('labels the branch-only OCR package as a test build', async () => {
    const { theme } = fakeTheme();
    installTauri(vi.fn().mockResolvedValue({
      checked: true,
      available: false,
      support: 'disabled_test_build',
      current_version: '0.6.1',
      version: null,
      notes: null,
    }), undefined);
    render(SettingsPanel, { props: { theme, isDesktop: true } });

    await fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
    expect(await screen.findByText('Updates are disabled in this test build.')).toBeTruthy();
    expect(screen.queryByText(/You’re up to date/)).toBeNull();
  });

  it('requires confirmation before logging out of warframe.market', async () => {
    const { theme } = fakeTheme();
    const onwfmlogout = vi.fn().mockResolvedValue(undefined);
    render(SettingsPanel, {
      props: {
        theme,
        isDesktop: true,
        wfmStatus: { logged_in: true, unlocked: true },
        onwfmlogout,
      },
    });

    expect(screen.getByText('Signed in · session unlocked')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Log out' }));
    expect(onwfmlogout).not.toHaveBeenCalled();
    await fireEvent.click(screen.getByRole('button', { name: 'Confirm log out' }));
    expect(onwfmlogout).toHaveBeenCalledOnce();
  });

  it('keeps the logout confirmation open when removing the login fails', async () => {
    const { theme } = fakeTheme();
    const onwfmlogout = vi.fn().mockRejectedValue(new Error('permission denied'));
    render(SettingsPanel, {
      props: {
        theme,
        isDesktop: true,
        wfmStatus: { logged_in: true, unlocked: false },
        onwfmlogout,
      },
    });

    await fireEvent.click(screen.getByRole('button', { name: 'Log out' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Confirm log out' }));
    expect((await screen.findByRole('alert')).textContent).toContain('Couldn’t log out: permission denied');
    expect(screen.getByRole('button', { name: 'Confirm log out' })).toBeTruthy();
  });

  it('distinguishes locked and signed-out session states', async () => {
    const { theme } = fakeTheme();
    render(SettingsPanel, {
      props: { theme, isDesktop: true, wfmStatus: { logged_in: true, unlocked: false } },
    });
    expect(screen.getByText('Signed in · session locked')).toBeTruthy();

    cleanup();
    render(SettingsPanel, {
      props: { theme, isDesktop: true, wfmStatus: { logged_in: false, unlocked: false } },
    });
    expect(screen.getByText('Not signed in')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Log out' })).toBeNull();
  });
});
