import type { OwnedRecord } from '../../contracts/data';
import type { DesktopServices } from '../../contracts/services';
import type { ProtectionPlan, ProtectionState, ProtectionInventory } from '../../contracts/protection';
import { humanError } from '../../contracts/errors';
import { type AccessStatus, ALLOWANCE_CHANGED_EVENT, TRADE_DETECTED_EVENT, WFM_ACCESS_EVENT } from '../../contracts/generated/desktop';

/**
 * Native changes that can move listed or available quantities: the trade
 * allowance, a trade the game confirmed, and market access policy changes. A new
 * inventory arrives through `setInventory`, and the app's own saves refresh
 * themselves.
 */
export const PROTECTION_REFRESH_EVENTS = [ALLOWANCE_CHANGED_EVENT, TRADE_DETECTED_EVENT, WFM_ACCESS_EVENT] as const;

/**
 * The backstop for listings changed outside the app, on the website or another
 * client. Each refresh reads the user's orders from warframe.market, and every
 * listing submission reads them again natively, so this only keeps the shown
 * quantities current; it was every 30 seconds whether or not anyone looked.
 */
export const PROTECTION_BACKSTOP_MS = 5 * 60_000;

/**
 * While a listing review or Trade Session is open, the quantities refresh this
 * often instead: that is how an open review learns the native snapshot moved
 * under it and blocks sending. A review is short and watched, so the cost is
 * bounded.
 */
export const PROTECTION_REVIEW_MS = 30_000;

export interface ProtectionSchedule {
  listen: DesktopServices['listenForTauriEvent'];
  document: Pick<Document, 'hidden' | 'addEventListener' | 'removeEventListener'>;
  setInterval: (fn: () => void, ms: number) => number;
  clearInterval: (id: number) => void;
  now: () => number;
  /** Whether a listing review or Trade Session is open. */
  reviewing: () => boolean;
}

export class ProtectionController {
  constructor(private port: Pick<DesktopServices, 'desktopProtectionState' | 'desktopSaveProtectionPlan'>) {}
  state = $state<ProtectionState | null>(null);
  savedPlan = $state<ProtectionPlan | null>(null);
  loading = $state(false);
  saving = $state(false);
  error = $state<string | null>(null);
  private input = $state.raw<{ owned: Map<string, OwnedRecord>; snapshotId: number | null; request: ProtectionInventory } | null>(null);
  private request = 0;

  matchesInventory(owned: Map<string, OwnedRecord>, snapshotId: number | null) {
    return this.input?.owned === owned && this.input.snapshotId === snapshotId;
  }

  async setInventory(owned: Map<string, OwnedRecord>, snapshotId: number | null) {
    this.request++;
    this.input = { owned, snapshotId, request: { snapshot_id: snapshotId, items: Object.fromEntries([...owned.values()]
      .filter(row => !row.subtype && !row.slug.endsWith('_set') && !row.slug.endsWith('_relic'))
      .map(row => [row.slug, { count: row.count, leveled: row.leveled ?? 0 }])) } };
    this.state = null;
    await this.refresh();
  }

  private disposed = false;
  private refreshTask: { input: object; started: boolean; again: boolean; promise: Promise<void> } | null = null;

  refresh(): Promise<void> {
    const input = this.input;
    if (!input || this.disposed) return Promise.resolve();
    const running = this.refreshTask;
    if (running?.input === input) {
      if (running.started) {
        running.again = true;
        this.request++;
      }
      return running.promise;
    }
    this.loading = true;
    const task = { input, started: false, again: false, promise: Promise.resolve() };
    this.refreshTask = task;
    // Events delivered together share one read; changes during that read get
    // one trailing read so an older observation cannot grant sale permission.
    task.promise = Promise.resolve().then(async () => {
      try {
        do {
          if (this.disposed || this.input !== input) return;
          task.again = false;
          task.started = true;
          await this.read(input.request);
        } while (task.again);
      } finally { if (this.refreshTask === task) this.refreshTask = null; }
    });
    return task.promise;
  }

  private async read(input: ProtectionInventory) {
    const request = ++this.request;
    this.loading = true;
    try {
      const state = await this.port.desktopProtectionState(input);
      if (request !== this.request || this.disposed) return;
      if (!state?.plan || !state.items || !Array.isArray(state.issues)) throw new Error('Protected quantities are unavailable. Retry before listing.');
      const quantity = (value: unknown): value is number => typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 && value <= 0xffff_ffff;
      if ((state.snapshot_id != null && (!Number.isSafeInteger(state.snapshot_id) || state.snapshot_id <= 0)) || Object.values(state.items).some(row =>
        !row || !quantity(row.owned) || !quantity(row.protected) || (row.listed != null && !quantity(row.listed))
        || (row.estimated != null && (!quantity(row.estimated) || row.estimated > row.owned))
        || (row.available != null && (!quantity(row.available) || row.available > row.owned)))) throw new Error('Protected quantities are invalid. Retry before listing.');
      // Unchanged polling results must not reset native calculations or disable
      // the listing trigger while the review restores keyboard focus.
      if (JSON.stringify(this.state) !== JSON.stringify(state)) this.state = state;
      this.savedPlan = state.plan;
      this.error = null;
    } catch (error) {
      if (request === this.request && !this.disposed) { this.state = null; this.error = humanError(error); }
    } finally { if (request === this.request && !this.disposed) this.loading = false; }
  }

  async save(plan: ProtectionPlan): Promise<boolean> {
    this.saving = true;
    this.error = null;
    try {
      await this.port.desktopSaveProtectionPlan(plan);
      this.savedPlan = plan;
      // Invalidate old availability before the refresh; the saved protection
      // already governs native submission even if reading orders now fails.
      this.state = null;
      await this.refresh();
      return this.error == null;
    } catch (error) { this.error = humanError(error); return false; }
    finally { this.saving = false; }
  }

  /**
   * Keep the quantities current: on native changes, when the window is shown
   * again, every 30 seconds while a review is open, and otherwise on a slow
   * backstop. Timed refreshes skip while the window is hidden. Returns the
   * teardown.
   */
  start(schedule: ProtectionSchedule): () => void {
    const idle = () => !this.loading && !this.saving;
    let last = schedule.now();
    const refresh = () => { last = schedule.now(); void this.refresh(); };
    let accessKey: string | undefined;
    const stops = PROTECTION_REFRESH_EVENTS.map(event => event === WFM_ACCESS_EVENT
      ? schedule.listen<AccessStatus>(event, status => {
        // A quantity read changes the request counters itself. Refreshing on
        // those changes feeds another read back into the shared market queue.
        const key = JSON.stringify([status.revision, status.cooldown_until_ms, status.restrictions, status.cooldown_until_ms > schedule.now()]);
        if (key === accessKey) return;
        accessKey = key;
        refresh();
      })
      : schedule.listen(event, refresh));
    const onVisibility = () => { if (!schedule.document.hidden && idle()) refresh(); };
    schedule.document.addEventListener('visibilitychange', onVisibility);
    const timer = schedule.setInterval(() => {
      if (schedule.document.hidden || !idle()) return;
      if (schedule.reviewing() || schedule.now() - last >= PROTECTION_BACKSTOP_MS) refresh();
    }, PROTECTION_REVIEW_MS);
    return () => {
      schedule.clearInterval(timer);
      schedule.document.removeEventListener('visibilitychange', onVisibility);
      for (const stop of stops) stop();
    };
  }

  destroy() { this.disposed = true; this.request++; }
}
