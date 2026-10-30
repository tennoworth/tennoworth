// Automatic scanning: a scan the app started on its own must reach the open
// screen without a click, but must never invalidate a listing flow the user is
// in the middle of. This is the decision that keeps those two true at once.
import { describe, expect, it, vi } from 'vitest';
import { AutoScanController } from './auto-scan.svelte';
import { INVENTORY_SCANNED_EVENT } from '../../contracts/events';
import type { AutoScanSettings, AutoScanStatus } from '../../contracts/desktop';

const SETTINGS: AutoScanSettings = { enabled: true, cadenceMinutes: 30, adoptAutomatically: true };
const STATUS: AutoScanStatus = {
  enabled: true, cadenceMinutes: 30, adoptAutomatically: true, held: false,
  gameRunning: true, lastScanAt: 1_700_000_000, lastError: null, nextCheckAt: null,
};

function port(overrides: Partial<Record<string, unknown>> = {}) {
  let listener: ((payload: unknown) => void) | null = null;
  const holds: boolean[] = [];
  const api = {
    getAutoScanSettings: vi.fn(async () => SETTINGS),
    updateAutoScanSettings: vi.fn(async (next: AutoScanSettings) => next),
    autoScanStatus: vi.fn(async () => STATUS),
    setAutoScanHold: vi.fn(async (hold: boolean) => { holds.push(hold); }),
    listenForTauriEvent: vi.fn((_event: string, cb: (payload: unknown) => void) => {
      listener = cb;
      return () => { listener = null; };
    }),
    ...overrides,
  };
  return { api, holds, emit: (payload: unknown) => listener?.(payload) };
}

function controller(
  api: ReturnType<typeof port>['api'],
  options: { adopt?: (data: Record<string, unknown>, snapshotId: number | null) => Promise<void> | void; interactive?: () => boolean; parse?: (raw: { inventory: string; snapshot_id: number | null }) => unknown } = {},
) {
  const adopted: Array<{ data: unknown; snapshotId: number | null }> = [];
  const c = new AutoScanController({
    settings: api as never,
    listen: api.listenForTauriEvent as never,
    parse: (options.parse ?? ((raw: { inventory: string; snapshot_id: number | null }) => ({ data: JSON.parse(raw.inventory), snapshotId: raw.snapshot_id }))) as never,
    adopt: async (data, snapshotId) => { adopted.push({ data, snapshotId }); await options.adopt?.(data as never, snapshotId); },
    isInteractive: options.interactive ?? (() => false),
  });
  return { c, adopted };
}

describe('AutoScanController', () => {
  it('subscribes to the background-scan event and clears any stale hold', async () => {
    const { api } = port();
    const { c } = controller(api);
    c.start();
    expect(api.listenForTauriEvent).toHaveBeenCalledWith(INVENTORY_SCANNED_EVENT, expect.any(Function));
    // A reloaded webview must not leave the previous page's hold in place.
    expect(api.setAutoScanHold).toHaveBeenCalledWith(false);
  });

  it('adopts a finished scan when adoption is on and no listing flow is open', async () => {
    const { api, emit } = port();
    const { c, adopted } = controller(api);
    c.start();
    await c.load();
    emit({ inventory: '{"Suits":[]}', snapshot_id: 7 });
    expect(adopted).toEqual([{ data: { Suits: [] }, snapshotId: 7 }]);
    expect(c.pending).toBeNull();
  });

  it('offers the scan instead of swapping while a listing flow is open', async () => {
    let interactive = true;
    const { api, emit } = port();
    const { c, adopted } = controller(api, { interactive: () => interactive });
    c.start();
    await c.load();
    emit({ inventory: '{"Suits":[]}', snapshot_id: 7 });
    expect(adopted).toEqual([]);
    expect(c.pending?.snapshotId).toBe(7);
    // Leaving the flow does not retroactively swap: the user still decides.
    interactive = false;
    expect(adopted).toEqual([]);
    expect(c.pending?.snapshotId).toBe(7);
  });

  it('offers the scan when adoption is switched off', async () => {
    const { api, emit } = port({ getAutoScanSettings: vi.fn(async () => ({ ...SETTINGS, adoptAutomatically: false })) });
    const { c, adopted } = controller(api);
    c.start();
    await c.load();
    emit({ inventory: '{"Suits":[]}', snapshot_id: 7 });
    expect(adopted).toEqual([]);
    expect(c.pending?.snapshotId).toBe(7);
  });

  it('never adopts before the preference has loaded', async () => {
    const { api, emit } = port();
    const { c, adopted } = controller(api);
    c.start();
    emit({ inventory: '{"Suits":[]}', snapshot_id: 7 });
    expect(adopted).toEqual([]);
    expect(c.pending?.snapshotId).toBe(7);
  });

  it('keeps only the newest offered scan', async () => {
    const { api, emit } = port();
    const { c } = controller(api, { interactive: () => true });
    c.start();
    await c.load();
    emit({ inventory: '{"Suits":[]}', snapshot_id: 7 });
    emit({ inventory: '{"Suits":[]}', snapshot_id: 9 });
    expect(c.pending?.snapshotId).toBe(9);
    await c.loadPending();
    expect(c.pending).toBeNull();
  });

  it('keeps the offer when loading it fails, so it can be retried', async () => {
    const { api, emit } = port();
    const { c } = controller(api, { interactive: () => true, adopt: () => { throw new Error('normalize failed'); } });
    c.start();
    await c.load();
    emit({ inventory: '{"Suits":[]}', snapshot_id: 7 });
    await expect(c.loadPending()).resolves.toBeUndefined();
    expect(c.pending?.snapshotId).toBe(7);
  });

  it('offers the scan when adopting it fails, instead of dropping it', async () => {
    const { api, emit } = port();
    const { c } = controller(api, { adopt: () => { throw new Error('normalize failed'); } });
    c.start();
    await c.load();
    emit({ inventory: '{"Suits":[]}', snapshot_id: 7 });
    await Promise.resolve();
    await Promise.resolve();
    expect(c.pending?.snapshotId).toBe(7);
  });

  it('drops a scan whose payload cannot be attributed to a snapshot', async () => {
    const { api, emit } = port();
    const { c } = controller(api, {
      parse: () => { throw new Error('Invalid inventory scan identity. Scan again.'); },
    });
    c.start();
    await c.load();
    emit({ inventory: '{"Suits":[]}', snapshot_id: -1 });
    expect(c.pending).toBeNull();
  });

  it('drops the offered scan when dismissed', async () => {    const { api, emit } = port();
    const { c } = controller(api, { interactive: () => true });
    c.start();
    await c.load();
    emit({ inventory: '{"Suits":[]}', snapshot_id: 7 });
    c.dismiss();
    expect(c.pending).toBeNull();
  });

  it('pushes the hold once per change, not once per render', async () => {
    const { api, holds } = port();
    const { c } = controller(api);
    c.start();
    expect(holds).toEqual([false]);
    await c.setInteractive(true);
    await c.setInteractive(true);
    await c.setInteractive(false);
    expect(holds).toEqual([false, true, false]);
  });

  it('surfaces a settings failure instead of pretending it saved', async () => {
    const { api } = port({ updateAutoScanSettings: vi.fn(async () => { throw new Error('db locked'); }) });
    const { c } = controller(api);
    await c.load();
    await c.save({ ...SETTINGS, cadenceMinutes: 60 });
    expect(c.settings?.cadenceMinutes).toBe(30);
    expect(c.settingsError).toMatch(/db locked/);
  });
});
