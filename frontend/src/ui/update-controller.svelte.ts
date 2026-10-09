/**
 * Desktop application updates, seen from the webview.
 *
 * The shell owns one instance so the update banner and Settings → Updates read
 * and drive the same check, download and restart. Two independent copies once
 * let Settings find an update that only the banner, at the top of the page,
 * could install.
 *
 * Contract with the Rust side is in adapters/desktop-update.ts: a check never
 * rejects for "no update", and only an install the user confirmed downloads
 * anything.
 */
import type { DesktopServices } from '../contracts/services';
import { UPDATE_CHECK_INTERVAL_MS, type UpdateStatus } from '../contracts/update';
import { humanError } from '../contracts/errors';

export type UpdateServices = Pick<
  DesktopServices,
  'updateStatus' | 'checkUpdate' | 'installUpdate' | 'restartApp' | 'onUpdateAvailable'
>;

export const LATEST_RELEASE_URL = 'https://github.com/tennoworth/tennoworth/releases/latest';

export class UpdateController {
  info = $state<UpdateStatus | null>(null);
  checking = $state(false);
  installing = $state(false);
  installed = $state(false);
  error = $state<string | null>(null);
  /**
   * The download's signature did not match the key this install trusts. Only a
   * manual install can cross that, so the UI says so instead of showing the
   * updater's raw error - which is all a user saw when the signing key was
   * replaced in 2026-09.
   */
  needsManualInstall = $state(false);
  /** The banner was closed; a newly offered update or an explicit check reopens it. */
  dismissed = $state(false);
  /** A check was asked for from a surface that reports through the banner. */
  announced = $state(false);

  constructor(private services: UpdateServices) {}

  /** Listen for launch pushes, read the stored status, and keep checking. Returns a disposer. */
  start(): () => void {
    // Best-effort throughout: a failure here must never disturb boot, and
    // "no update" needs no UI at all.
    const unlisten = this.services.onUpdateAvailable((s) => this.offer(s));
    void this.services.updateStatus().then(
      (s) => { if (s.available && !this.info?.available) this.info = s; },
      (e) => console.error('update status read failed', e),
    );
    const timer = window.setInterval(() => {
      if (this.checking || this.installing || this.installed) return;
      void this.services.checkUpdate().then(
        (s) => this.offer(s),
        (e) => console.error('periodic update check failed', e),
      );
    }, UPDATE_CHECK_INTERVAL_MS);
    return () => {
      window.clearInterval(timer);
      unlisten();
    };
  }

  /**
   * Fetch the manifest now. Nothing is downloaded. `announce` is for callers
   * with no status of their own, so the banner reports even "up to date".
   */
  async check({ announce = false } = {}): Promise<void> {
    if (announce) {
      this.announced = true;
      this.dismissed = false;
    }
    if (this.checking || this.installing || this.installed) return;
    this.checking = true;
    this.error = null;
    try {
      this.info = await this.services.checkUpdate();
    } catch (error) {
      this.error = humanError(error);
    } finally {
      this.checking = false;
    }
  }

  // Explicit confirmation is THE gate: install_update rejects on a download
  // failure or a bad bundle signature, which is shown while the running app
  // stays intact (and the update stays retryable).
  async install(): Promise<void> {
    if (this.installing || this.installed || !this.info?.available) return;
    this.error = null;
    this.needsManualInstall = false;
    this.installing = true;
    try {
      await this.services.installUpdate();
      this.installed = true;
    } catch (e) {
      const message = humanError(e);
      if (/signature/i.test(message)) this.needsManualInstall = true;
      else this.error = message;
    } finally {
      this.installing = false;
    }
  }

  async restart(): Promise<void> {
    this.error = null;
    try {
      await this.services.restartApp();
    } catch (e) {
      this.error = humanError(e);
    }
  }

  private offer(s: UpdateStatus): void {
    if (!s.available || this.installing || this.installed) return;
    this.info = s;
    this.dismissed = false;
  }
}
