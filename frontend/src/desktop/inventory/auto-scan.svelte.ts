/**
 * Automatic scanning, seen from the webview.
 *
 * Owns the one user-visible decision the loop cannot make for itself: a scan
 * that finishes on its own either replaces the inventory on screen or waits for
 * "Load new scan". It also owns the hold that suspends scanning while an
 * interactive listing flow is open, because a background scan records a new
 * snapshot and a listing submit is rejected unless it names the latest one.
 *
 * The settings live here rather than in the Settings panel so the shell and the
 * panel read one source of truth - the same arrangement as the theme controller.
 */
import type { Inventory } from '../../contracts/data';
import type { AutoScanSettings, AutoScanStatus, DesktopCapabilities } from '../../contracts/desktop';
import type { DesktopServices } from '../../contracts/services';
import { INVENTORY_SCANNED_EVENT } from '../../contracts/events';
import { humanError } from '../../contracts/errors';

/** The Rust struct the background-scan event carries. */
export interface RawScannedInventory {
  inventory: string;
  snapshot_id: number | null;
}

export interface AutoScanDeps {
  settings: Pick<
    DesktopCapabilities,
    'getAutoScanSettings' | 'updateAutoScanSettings' | 'autoScanStatus' | 'setAutoScanHold'
  >;
  listen: DesktopServices['listenForTauriEvent'];
  /**
   * Validate and parse a background scan. The shell supplies the adapter's own
   * parser, so the command response and the event enforce one snapshot-identity
   * rule rather than two that can drift.
   */
  parse: (raw: RawScannedInventory) => { data: Inventory; snapshotId: number | null };
  adopt: (data: Inventory, snapshotId: number | null) => Promise<void> | void;
  /** True while a listing review or Trade Session batch is being prepared. */
  isInteractive: () => boolean;
}

/** A scan the app finished on its own, waiting for the user to take it. */
export interface OfferedScan {
  data: Inventory;
  snapshotId: number | null;
  at: number;
}

export class AutoScanController {
  /** Null until the stored preferences load. */
  settings = $state<AutoScanSettings | null>(null);
  /** Null until the first status read; the loop's own view of itself. */
  status = $state<AutoScanStatus | null>(null);
  /** A background scan held back instead of being swapped in. */
  pending = $state<OfferedScan | null>(null);
  settingsError = $state('');

  #interactive: boolean | null = null;

  constructor(private deps: AutoScanDeps) {}

  /** Subscribe to background scans and sync the initial hold. Returns a disposer. */
  start(): () => void {
    const stop = this.deps.listen<RawScannedInventory>(INVENTORY_SCANNED_EVENT, (raw) =>
      this.handleScan(raw),
    );
    // A reloaded webview inherits the Rust process's hold. Clearing it here is
    // what stops a page that was closed mid-review from suspending scanning for
    // the rest of the app's uptime.
    void this.setInteractive(false);
    return stop;
  }

  async load(): Promise<void> {
    this.settingsError = '';
    try {
      this.settings = await this.deps.settings.getAutoScanSettings();
    } catch (error) {
      this.settingsError = humanError(error);
    }
    await this.refreshStatus();
  }

  async save(next: AutoScanSettings): Promise<void> {
    this.settingsError = '';
    try {
      this.settings = await this.deps.settings.updateAutoScanSettings(next);
      await this.refreshStatus();
    } catch (error) {
      this.settingsError = humanError(error);
    }
  }

  async refreshStatus(): Promise<void> {
    try {
      this.status = await this.deps.settings.autoScanStatus();
    } catch {
      // A failed poll leaves the last known status on screen; the panel keeps
      // polling, and Settings must never fail because the loop is unreachable.
    }
  }

  /** Hold or release automatic scanning, pushing only real changes. */
  async setInteractive(active: boolean): Promise<void> {
    if (this.#interactive === active) return;
    this.#interactive = active;
    try {
      await this.deps.settings.setAutoScanHold(active);
    } catch {
      // The hold is best-effort: a failure leaves scanning enabled rather than
      // blocking the user's listing flow.
    }
  }

  async loadPending(): Promise<void> {
    const offered = this.pending;
    if (!offered) return;
    try {
      await this.deps.adopt(offered.data, offered.snapshotId);
    } catch {
      // The inventory controller reports the failure; keeping the offer lets the
      // user retry it instead of losing the scan.
      return;
    }
    if (this.pending === offered) this.pending = null;
  }

  dismiss(): void {
    this.pending = null;
  }

  private handleScan(raw: RawScannedInventory): void {
    let parsed: { data: Inventory; snapshotId: number | null };
    try {
      parsed = this.deps.parse(raw);
    } catch {
      // An unattributable scan is dropped, not offered: the command path refuses
      // the same payload, and offering it would put a snapshot nothing can be
      // submitted against in front of the user.
      return;
    }
    const offered: OfferedScan = { data: parsed.data, snapshotId: parsed.snapshotId, at: Date.now() };
    // Unknown preference or an open listing flow both mean "do not swap": the
    // first because adopting without knowing the setting would ignore it, the
    // second because the rows behind a review are about to be submitted.
    if (this.settings?.adoptAutomatically !== true || this.deps.isInteractive()) {
      this.pending = offered;
      return;
    }
    void this.adoptOffered(offered);
  }

  /** Adopt a scan the app took on its own; a failure hands it back to the user. */
  private async adoptOffered(offered: OfferedScan): Promise<void> {
    try {
      await this.deps.adopt(offered.data, offered.snapshotId);
    } catch {
      this.pending = offered;
    }
  }
}
