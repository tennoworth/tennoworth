/**
 * Trade presence, seen from the webview. The native loop owns the status
 * channel and pushes every change; this mirrors the latest push so the status
 * strip and Settings read one source, and turns the user's picks into calls.
 */
import type { DesktopCapabilities, PresenceChoice, PresenceSettings, PresenceStatus } from '../../contracts/desktop';
import type { DesktopServices } from '../../contracts/services';
import { PRESENCE_CHANGED_EVENT } from '../../contracts/events';
import { humanError } from '../../contracts/errors';

export interface PresenceDeps {
  native: Pick<DesktopCapabilities, 'presenceStatus' | 'setPresence' | 'updatePresenceSettings' | 'followGameNow'>;
  listen: DesktopServices['listenForTauriEvent'];
}

export class PresenceController {
  /** Null until the first read. */
  status = $state<PresenceStatus | null>(null);
  /** The choice waiting for warframe.market's answer. */
  pending = $state<PresenceChoice | null>(null);
  savingSettings = $state(false);
  /** The last failed pick or save, in the user's words; cleared by the next attempt. */
  error = $state('');

  /** Counts pushes, so a reply that was overtaken by one does not overwrite it. */
  #pushes = 0;

  constructor(private deps: PresenceDeps) {}

  #adopt(status: PresenceStatus, pushesBefore: number): void {
    if (this.#pushes === pushesBefore) this.status = status;
  }

  /** Follow native pushes and read the current state. Returns a disposer. */
  start(): () => void {
    const stop = this.deps.listen<PresenceStatus>(PRESENCE_CHANGED_EVENT, (status) => {
      this.#pushes += 1;
      this.status = status;
    });
    void this.refresh();
    return stop;
  }

  async refresh(): Promise<void> {
    const before = this.#pushes;
    try {
      this.#adopt(await this.deps.native.presenceStatus(), before);
    } catch {
      // The strip falls back to the session label; nothing here can fail the shell.
    }
  }

  async set(choice: PresenceChoice): Promise<void> {
    if (this.pending) return;
    this.pending = choice;
    this.error = '';
    const before = this.#pushes;
    try {
      this.#adopt(await this.deps.native.setPresence(choice), before);
    } catch (error) {
      this.error = humanError(error);
    } finally {
      this.pending = null;
    }
  }

  async saveSettings(next: PresenceSettings): Promise<void> {
    this.savingSettings = true;
    this.error = '';
    try {
      const saved = await this.deps.native.updatePresenceSettings(next);
      if (this.status) this.status = { ...this.status, settings: saved };
    } catch (error) {
      this.error = humanError(error);
    } finally {
      this.savingSettings = false;
    }
  }

  async followNow(): Promise<void> {
    this.error = '';
    try {
      await this.deps.native.followGameNow();
    } catch (error) {
      this.error = humanError(error);
    }
  }
}
