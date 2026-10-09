// Settings view: the Appearance section renders the mode control, drives the
// ThemeController, and explains what System does. This is the ONLY place the
// theme can be changed inside the shell now, so a regression here leaves a
// user with no way to override the OS scheme in the app.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { screen, fireEvent, cleanup, waitFor, within, act } from '@testing-library/svelte';
import { renderDesktop as render } from '../../dev/render-desktop';
import SettingsPanel from './SettingsPanel.svelte';
import type { ModePref, ThemeController } from '../../ui/theme';
import type { DesktopCapabilities } from '../../contracts/desktop';
import { AUTO_SCAN_CADENCE_CHOICES, type AutoScanSettings, type AutoScanStatus } from '../../contracts/desktop';
import { AutoScanController } from '../inventory/auto-scan.svelte';
import type { OverlaySettings } from '../../contracts/data';
import { installTauri, removeTauri } from '../../dev/test-utils';
import { AppIconController } from './app-icon.svelte';
import { LocalStorageStateStore, LOCAL_SETTING_KEYS } from '../../adapters/state-store';
import { UpdateController } from '../../ui/update-controller.svelte';
import { createDesktopServices } from '../../adapters/services';

vi.mock('../../adapters/desktop', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../adapters/desktop')>(),
  desktopNotificationPreferences: vi.fn(async () => ({ popups: true, categories: Object.fromEntries(['trades', 'watches', 'baro', 'calendar', 'digest'].map(k => [k, { enabled: true, native: true }])) })),
}));

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

// The status strip repeats each state above the page, so section assertions
// read the section itself.
const account = () => within(screen.getByRole('region', { name: 'warframe.market account' }));
const scanning = () => within(screen.getByRole('region', { name: 'Automatic scan' }));

function fakeTheme(pref: ModePref = 'system') {
  const setModePref = vi.fn();
  return {
    theme: { modePref: pref, setModePref, destroy: () => {} } as unknown as ThemeController,
    setModePref,
  };
}

describe('SettingsPanel', () => {
  function pollingTransport() {
    const settings: OverlaySettings = { enabled: false, autoDetect: true, shortcut: 'Ctrl+Shift+O', scale: 1,
      livePrices: true, showOwned: true, diagnostics: false };
    const status = { state: 'disabled', backend: 'x11-window', presentationBackend: 'tauri-window', placement: 'anchored', ocrReady: true };
    let resolve!: (value: typeof status) => void;
    let reject!: (error: Error) => void;
    const pending = new Promise<typeof status>((yes, no) => { resolve = yes; reject = no; });
    const overlayStatus = vi.fn().mockResolvedValueOnce(status).mockReturnValueOnce(pending).mockResolvedValue(status);
    const transport = { getOverlaySettings: async () => settings, overlayStatus,
      updateOverlaySettings: async (next: OverlaySettings) => next,
      setupOverlayCapture: async () => ({ ...status, state: 'watching' }),
      getUsagePreferences: async () => ({ enabled: false, available: false }),
      getPriceReportPreferences: async () => ({ enabled: false, available: false, sent_this_week: 0 }),
    } as unknown as DesktopCapabilities;
    return { transport, overlayStatus, resolve, reject, status };
  }

  it('keeps one polling read in flight and retries after a failed status read', async () => {
    vi.useFakeTimers();
    try {
      const { transport, overlayStatus, reject } = pollingTransport();
      render(SettingsPanel, { props: { theme: fakeTheme().theme, transport } });
      await act(async () => {});
      await act(() => vi.advanceTimersByTimeAsync(3000));
      expect(overlayStatus).toHaveBeenCalledTimes(2);
      await act(async () => { reject(new Error('Status unavailable')); });
      await act(() => vi.advanceTimersByTimeAsync(1000));
      expect(overlayStatus).toHaveBeenCalledTimes(3);
    } finally { cleanup(); vi.useRealTimers(); }
  });

  it('does not overwrite the enabled overlay status with an earlier poll', async () => {
    vi.useFakeTimers();
    try {
      const { transport, resolve, status } = pollingTransport();
      render(SettingsPanel, { props: { theme: fakeTheme().theme, transport } });
      await act(async () => {});
      await act(() => vi.advanceTimersByTimeAsync(1000));
      await fireEvent.click(screen.getByRole('checkbox', { name: /Enable local screen recognition/ }));
      await act(async () => {});
      expect(screen.getByText('watching · OCR ready', { selector: 'strong' })).toBeTruthy();
      await act(async () => { resolve(status); });
      expect(screen.getByText('watching · OCR ready', { selector: 'strong' })).toBeTruthy();
    } finally { cleanup(); vi.useRealTimers(); }
  });

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

  it('there is no look picker left - the mode is the only choice', () => {
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
      getUsagePreferences: vi.fn(async () => ({ enabled: false, available: true })),
      getPriceReportPreferences: vi.fn(async () => ({ enabled: false, available: true, sent_this_week: 0 })),
      setUsagePreferences: vi.fn(async (enabled: boolean) => ({ enabled, available: true })),
      getOverlaySettings: vi.fn(async () => settings),
      updateOverlaySettings,
      overlayStatus: vi.fn(async () => ({ state: 'disabled', backend: 'x11-window', presentationBackend: 'tauri-window', placement: 'anchored', ocrReady: true })),
      setupOverlayCapture: vi.fn(async () => ({ state: 'watching', backend: 'x11-window', presentationBackend: 'tauri-window', placement: 'anchored', ocrReady: true })),
      previewRelicOverlay: vi.fn(async () => {}),
      scanOverlayNow: vi.fn(async () => {}),
      openOverlayDiagnostics: vi.fn(async () => {}),
      clearOverlayDiagnostics: vi.fn(async () => {}),
    } as unknown as DesktopCapabilities;
    render(SettingsPanel, { props: { theme, transport } });

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
    render(SettingsPanel, { props: { theme, updates: new UpdateController(createDesktopServices()) } });

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
    render(SettingsPanel, { props: { theme, updates: new UpdateController(createDesktopServices()) } });

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
    render(SettingsPanel, { props: { theme, updates: new UpdateController(createDesktopServices()) } });

    await fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
    expect(await screen.findByText('Updates are disabled in this test build.')).toBeTruthy();
    expect(screen.queryByText(/You’re up to date/)).toBeNull();
  });

  it('installs a found update in place and offers the restart there', async () => {
    const { theme } = fakeTheme();
    const invoke = vi.fn(async (command: string) => {
      if (command === 'check_update') {
        return { checked: true, available: true, support: 'supported', current_version: '0.8.99', version: '0.8.100', notes: null };
      }
      if (command === 'install_update') return null;
      throw new Error(`unexpected command: ${command}`);
    });
    installTauri(invoke, undefined);
    render(SettingsPanel, { props: { theme, updates: new UpdateController(createDesktopServices()) } });

    await fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
    expect(await screen.findByText(/Version 0\.8\.100 is available/)).toBeTruthy();
    expect(invoke).not.toHaveBeenCalledWith('install_update');
    await fireEvent.click(screen.getByRole('button', { name: 'Install update' }));
    expect(invoke).toHaveBeenCalledWith('install_update');
    expect(await screen.findByRole('button', { name: 'Restart now' })).toBeTruthy();
    expect(screen.getByText(/Version 0\.8\.100 is installed/)).toBeTruthy();
  });

  it('shows a failed in-place install beside the update controls', async () => {
    const { theme } = fakeTheme();
    installTauri(vi.fn(async (command: string) => {
      if (command === 'check_update') {
        return { checked: true, available: true, support: 'supported', current_version: '0.8.99', version: '0.8.100', notes: null };
      }
      throw new Error('download failed: connection reset');
    }), undefined);
    render(SettingsPanel, { props: { theme, updates: new UpdateController(createDesktopServices()) } });

    await fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
    await fireEvent.click(await screen.findByRole('button', { name: 'Install update' }));
    expect((await screen.findByRole('alert')).textContent).toContain('download failed: connection reset');
    expect(screen.getByRole('button', { name: 'Install update' })).toBeTruthy();
  });

  it('requires confirmation before logging out of warframe.market', async () => {
    const { theme } = fakeTheme();
    const onwfmlogout = vi.fn().mockResolvedValue(undefined);
    render(SettingsPanel, {
      props: {
        theme,

        wfmStatus: { logged_in: true, unlocked: true },
        onwfmlogout,
      },
    });

    expect(account().getByText('Signed in · session unlocked')).toBeTruthy();
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
      props: { theme, wfmStatus: { logged_in: true, unlocked: false } },
    });
    expect(account().getByText('Signed in · session locked')).toBeTruthy();

    cleanup();
    render(SettingsPanel, {
      props: { theme, wfmStatus: { logged_in: false, unlocked: false } },
    });
    expect(account().getByText('Not signed in')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Log out' })).toBeNull();
  });
});

describe('SettingsPanel automatic scanning', () => {
  async function autoScan(settings: AutoScanSettings, status: Partial<AutoScanStatus> = {}) {
    const port = {
      getAutoScanSettings: vi.fn(async () => settings),
      updateAutoScanSettings: vi.fn(async (next: AutoScanSettings) => next),
      autoScanStatus: vi.fn(async () => ({
        enabled: true, cadenceMinutes: 30, adoptAutomatically: true, held: false,
        gameRunning: true, lastScanAt: null, lastError: null, nextCheckAt: null,
        ...status,
      }) as AutoScanStatus),
      setAutoScanHold: vi.fn(async () => {}),
      listenForTauriEvent: vi.fn(() => () => {}),
    };
    const controller = new AutoScanController({
      settings: port as never,
      listen: port.listenForTauriEvent as never,
      parse: (raw) => ({ data: {}, snapshotId: raw.snapshot_id }) as never,
      adopt: vi.fn(),
      isInteractive: () => false,
    });
    await controller.load();
    return { controller, port };
  }

  it('is off with the cadence hidden, and turns on through the transport', async () => {
    const { theme } = fakeTheme();
    const { controller, port } = await autoScan({ enabled: false, cadenceMinutes: 30, adoptAutomatically: true });
    render(SettingsPanel, { props: { theme, autoScan: controller } });

    const toggle = screen.getByRole('checkbox', { name: /Scan automatically while Warframe is running/ }) as HTMLInputElement;
    expect(toggle.checked).toBe(false);
    expect(screen.queryByRole('combobox', { name: /Scan every/ })).toBeNull();

    await fireEvent.click(toggle);
    expect(port.updateAutoScanSettings).toHaveBeenCalledWith({ enabled: true, cadenceMinutes: 30, adoptAutomatically: true });
    expect(await screen.findByRole('combobox', { name: /Scan every/ })).toBeTruthy();
  });

  it('offers exactly the cadences the loop accepts', async () => {
    const { theme } = fakeTheme();
    const { controller, port } = await autoScan({ enabled: true, cadenceMinutes: 15, adoptAutomatically: true });
    render(SettingsPanel, { props: { theme, autoScan: controller } });

    const select = screen.getByRole('combobox', { name: /Scan every/ }) as HTMLSelectElement;
    expect([...select.options].map((option) => Number(option.value))).toEqual([...AUTO_SCAN_CADENCE_CHOICES]);
    await fireEvent.change(select, { target: { value: '60' } });
    expect(port.updateAutoScanSettings).toHaveBeenCalledWith({ enabled: true, cadenceMinutes: 60, adoptAutomatically: true });
  });

  it('writes the adopt preference, which covers any background scan', async () => {
    const { theme } = fakeTheme();
    const { controller, port } = await autoScan({ enabled: true, cadenceMinutes: 30, adoptAutomatically: true });
    render(SettingsPanel, { props: { theme, autoScan: controller } });

    await fireEvent.click(screen.getByRole('checkbox', { name: /Update the open app automatically/ }));
    expect(port.updateAutoScanSettings).toHaveBeenCalledWith({ enabled: true, cadenceMinutes: 30, adoptAutomatically: false });
  });

  it('puts a control back when its save fails', async () => {
    const { theme } = fakeTheme();
    const { controller, port } = await autoScan({ enabled: true, cadenceMinutes: 30, adoptAutomatically: true });
    port.updateAutoScanSettings.mockRejectedValue(new Error('database is locked'));
    render(SettingsPanel, { props: { theme, autoScan: controller } });

    const adopt = screen.getByRole('checkbox', { name: /Update the open app automatically/ }) as HTMLInputElement;
    await fireEvent.click(adopt);
    await screen.findByText(/database is locked/);
    expect(adopt.checked).toBe(true);

    const select = screen.getByRole('combobox', { name: /Scan every/ }) as HTMLSelectElement;
    await fireEvent.change(select, { target: { value: '60' } });
    await waitFor(() => expect(select.value).toBe('30'));
  });

  it('reports what the loop is doing, including a failure and a paused scan', async () => {
    const { theme } = fakeTheme();
    const waiting = await autoScan({ enabled: true, cadenceMinutes: 30, adoptAutomatically: true }, { gameRunning: false });
    render(SettingsPanel, { props: { theme, autoScan: waiting.controller } });
    expect(scanning().getByText(/Waiting for Warframe/)).toBeTruthy();

    cleanup();
    const failed = await autoScan({ enabled: true, cadenceMinutes: 30, adoptAutomatically: true }, { gameRunning: true, lastError: 'No accountId/nonce pair found in WF memory.' });
    render(SettingsPanel, { props: { theme, autoScan: failed.controller } });
    expect(scanning().getByText(/No accountId\/nonce pair found/)).toBeTruthy();

    cleanup();
    const held = await autoScan({ enabled: true, cadenceMinutes: 30, adoptAutomatically: true }, { held: true });
    render(SettingsPanel, { props: { theme, autoScan: held.controller } });
    expect(scanning().getByText(/paused while a listing review or Trade Session/)).toBeTruthy();
  });

  it('leads with a status strip that links each state to its section', async () => {
    const { theme } = fakeTheme();
    const { controller } = await autoScan({ enabled: true, cadenceMinutes: 15, adoptAutomatically: true }, { gameRunning: false });
    render(SettingsPanel, { props: { theme, autoScan: controller, wfmStatus: { logged_in: true, unlocked: false } } });
    const strip = within(screen.getByLabelText('Current state'));
    const scan = strip.getByRole('link', { name: /Automatic scan/ });
    expect(scan.getAttribute('href')).toBe('#settings-scan');
    expect(scan.textContent).toContain('On · every 15 min');
    expect(scan.textContent).toContain('Waiting for Warframe');
    expect(strip.getByRole('link', { name: /warframe.market/ }).textContent).toContain('Signed in · session locked');
    expect(document.getElementById('settings-scan')).toBe(screen.getByRole('region', { name: 'Automatic scan' }));
  });
});

describe('App icon setting', () => {
  function appIconFor(mode: 'light' | 'dark', windowIcon = true) {
    localStorage.clear();
    const setAppIcon = vi.fn(async (_colour: string) => ({ windowIcon }));
    const appIcon = new AppIconController({
      store: new LocalStorageStateStore(),
      theme: { mode, subscribe: () => () => {} },
      native: { setAppIcon },
    });
    return { appIcon, setAppIcon };
  }
  const group = () => within(screen.getByRole('radiogroup', { name: 'App icon' }));

  it('offers the four colours under Colour mode, classic blue chosen by default', () => {
    const { appIcon } = appIconFor('light');
    render(SettingsPanel, { props: { theme: fakeTheme().theme, appIcon } });
    const names = group().getAllByRole('radio').map((radio) => radio.closest('label')?.querySelector('.name')?.textContent);
    expect(names).toEqual(['Classic blue', 'Match colour mode', 'Ink', 'Rag']);
    expect((group().getByRole('radio', { name: /^Classic blue/ }) as HTMLInputElement).checked).toBe(true);
    expect(group().getByRole('radio', { name: /^Rag/ }).closest('label')?.textContent).toContain('Hard to see on light taskbars.');
    expect(screen.getByText(/Your desktop shortcut and the installer keep the classic icon/)).toBeTruthy();
  });

  it('applies the resolved colour when a card is chosen', async () => {
    const { appIcon, setAppIcon } = appIconFor('dark');
    render(SettingsPanel, { props: { theme: fakeTheme().theme, appIcon } });
    await fireEvent.click(group().getByRole('radio', { name: /^Match colour mode/ }));
    await waitFor(() => expect(setAppIcon).toHaveBeenLastCalledWith('rag'));
    expect((group().getByRole('radio', { name: /^Match colour mode/ }) as HTMLInputElement).checked).toBe(true);
    expect(localStorage.getItem(LOCAL_SETTING_KEYS['app-icon'])).toBe('match');
  });

  it('says when this session cannot show a window icon, and reports failures', async () => {
    const { appIcon, setAppIcon } = appIconFor('light', false);
    render(SettingsPanel, { props: { theme: fakeTheme().theme, appIcon } });
    await fireEvent.click(group().getByRole('radio', { name: /^Ink/ }));
    await waitFor(() => expect(screen.getByText(/only the tray follows this setting/)).toBeTruthy());
    setAppIcon.mockRejectedValueOnce(new Error('no tray'));
    await fireEvent.click(group().getByRole('radio', { name: /^Rag/ }));
    await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('no tray'));
  });

  it('is absent where no controller is given (not a desktop build)', () => {
    render(SettingsPanel, { props: { theme: fakeTheme().theme } });
    expect(screen.queryByRole('radiogroup', { name: 'App icon' })).toBeNull();
  });
});
