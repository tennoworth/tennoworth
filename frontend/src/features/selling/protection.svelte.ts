import type { DesktopServices } from '../../contracts/services';
import type { ProtectionPlan, ProtectionState } from '../../contracts/protection';
import { humanError } from '../../contracts/errors';

export class ProtectionController {
  constructor(private port: Pick<DesktopServices, 'desktopProtectionState' | 'desktopSaveProtectionPlan'>) {}
  state = $state<ProtectionState | null>(null);
  loading = $state(false);
  saving = $state(false);
  error = $state<string | null>(null);
  private request = 0;
  private disposed = false;

  async refresh() {
    const request = ++this.request;
    this.loading = true;
    try {
      const state = await this.port.desktopProtectionState();
      if (request !== this.request || this.disposed) return;
      if (!state?.plan || !state.items || !Array.isArray(state.issues)) throw new Error('Protected quantities are unavailable. Retry before listing.');
      // Unchanged polling results must not reset native calculations or disable
      // the listing trigger while the review restores keyboard focus.
      if (JSON.stringify(this.state) !== JSON.stringify(state)) this.state = state;
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
      // Invalidate old availability before the refresh; the saved protection
      // already governs native submission even if reading orders now fails.
      this.state = null;
      await this.refresh();
      return this.error == null;
    } catch (error) { this.error = humanError(error); return false; }
    finally { this.saving = false; }
  }

  destroy() { this.disposed = true; this.request++; }
}
