// @ts-nocheck — vitest fixtures; the module's TS contract is exercised by tsc.
import { describe, it, expect, afterEach, vi } from 'vitest';
import {
  updateStatus,
  checkUpdate,
  installUpdate,
  restartApp,
  onUpdateAvailable,
} from './desktop-update.js';

function installTauri(invoke, listen) {
  globalThis.__TAURI_INTERNALS__ = { invoke };
  globalThis.__TAURI__ = { core: { invoke }, ...(listen ? { event: { listen } } : {}) };
}
function removeTauri() {
  delete globalThis.__TAURI_INTERNALS__;
  delete globalThis.__TAURI__;
}

afterEach(() => {
  removeTauri();
  vi.restoreAllMocks();
});

const NO_UPDATE = {
  checked: true,
  available: false,
  current_version: '0.1.0',
  version: null,
  notes: null,
};

describe('command mapping', () => {
  it('updateStatus / checkUpdate invoke their commands and pass the payload through', async () => {
    const invoke = vi.fn().mockResolvedValue(NO_UPDATE);
    installTauri(invoke);
    expect(await updateStatus()).toEqual(NO_UPDATE);
    expect(await checkUpdate()).toEqual(NO_UPDATE);
    expect(invoke.mock.calls.map((c) => c[0])).toEqual(['update_status', 'check_update']);
  });

  it('installUpdate invokes install_update and surfaces a rejection (bad signature) verbatim', async () => {
    const invoke = vi.fn().mockRejectedValue('Update could not be installed: signature mismatch');
    installTauri(invoke);
    await expect(installUpdate()).rejects.toBe(
      'Update could not be installed: signature mismatch',
    );
    expect(invoke).toHaveBeenCalledWith('install_update');
  });

  it('restartApp invokes restart_app', async () => {
    const invoke = vi.fn().mockResolvedValue(undefined);
    installTauri(invoke);
    await restartApp();
    expect(invoke).toHaveBeenCalledWith('restart_app');
  });
});

describe('onUpdateAvailable', () => {
  it('registers on the update-available event and forwards the payload', async () => {
    const handlers = {};
    const listen = vi.fn((name, h) => {
      handlers[name] = h;
      return Promise.resolve(() => {});
    });
    installTauri(vi.fn(), listen);
    const seen = [];
    onUpdateAvailable((s) => seen.push(s));
    expect(listen).toHaveBeenCalledTimes(1);
    const status = { ...NO_UPDATE, available: true, version: '0.2.0' };
    handlers['update-available']({ payload: status });
    expect(seen).toEqual([status]);
  });

  it('is a no-op without the event API (never throws)', () => {
    installTauri(vi.fn()); // no event.listen
    expect(() => onUpdateAvailable(() => {})).not.toThrow();
  });

  it('swallows a rejected listen registration', async () => {
    const listen = vi.fn(() => Promise.reject(new Error('acl denied')));
    installTauri(vi.fn(), listen);
    expect(() => onUpdateAvailable(() => {})).not.toThrow();
    await Promise.resolve(); // let the rejection settle — must not surface
  });
});
