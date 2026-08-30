// @ts-nocheck - vitest fixtures; the module's TS contract is exercised by tsc.
import { describe, it, expect, afterEach, vi } from 'vitest';
import { installTauri, removeTauri } from './test-utils.js';
import updateSupportFixture from '../../../tests/fixtures/update-support.json';
import {
  updateStatus,
  checkUpdate,
  installUpdate,
  restartApp,
  onUpdateAvailable,
  UPDATE_CHECK_INTERVAL_MS,
  UPDATE_SUPPORT,
} from './desktop-update.js';


afterEach(() => {
  removeTauri();
  vi.restoreAllMocks();
});

const NO_UPDATE = {
  checked: true,
  available: false,
  support: 'supported',
  current_version: '0.1.0',
  version: null,
  notes: null,
};

describe('command mapping', () => {
  it('keeps updater support states aligned with Rust', () => {
    expect([...UPDATE_SUPPORT]).toEqual(updateSupportFixture);
  });

  it('checks for updates every half hour', () => {
    expect(UPDATE_CHECK_INTERVAL_MS).toBe(1_800_000);
  });

  it('checkUpdate invokes its command and passes the result through', async () => {
    const invoke = vi.fn().mockResolvedValue(NO_UPDATE);
    installTauri(invoke);
    expect(await checkUpdate()).toEqual(NO_UPDATE);
    expect(invoke).toHaveBeenCalledWith('check_update');
  });

  it('updateStatus invokes its command and passes the payload through', async () => {
    const invoke = vi.fn().mockResolvedValue(NO_UPDATE);
    installTauri(invoke);
    expect(await updateStatus()).toEqual(NO_UPDATE);
    expect(invoke.mock.calls.map((c) => c[0])).toEqual(['update_status']);
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
    const unlisten = vi.fn();
    const listen = vi.fn((name, h) => {
      handlers[name] = h;
      return Promise.resolve(unlisten);
    });
    installTauri(vi.fn(), listen);
    const seen = [];
    const stop = onUpdateAvailable((s) => seen.push(s));
    expect(listen).toHaveBeenCalledTimes(1);
    const status = { ...NO_UPDATE, available: true, version: '0.2.0' };
    handlers['update-available']({ payload: status });
    expect(seen).toEqual([status]);
    await Promise.resolve();
    stop();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it('is a no-op without the event API (never throws)', () => {
    installTauri(vi.fn()); // no event.listen
    expect(() => onUpdateAvailable(() => {})).not.toThrow();
  });

  it('swallows a rejected listen registration', async () => {
    const listen = vi.fn(() => Promise.reject(new Error('acl denied')));
    installTauri(vi.fn(), listen);
    expect(() => onUpdateAvailable(() => {})).not.toThrow();
    await Promise.resolve(); // let the rejection settle - must not surface
  });
});
