import type { OwnedRecord } from '../../contracts/data';
import type { DesktopServices } from '../../contracts/services';
import type { ProtectionPlan, ProtectionState, ProtectionInventory } from '../../contracts/protection';
import { humanError } from '../../contracts/errors';

export class ProtectionController {
  constructor(private port: Pick<DesktopServices, 'desktopProtectionState' | 'desktopSaveProtectionPlan'>) {}
  state = $state<ProtectionState | null>(null);
  loading = $state(false);
  saving = $state(false);
  error = $state<string | null>(null);
  private input = $state.raw<{ owned: Map<string, OwnedRecord>; snapshotId: number | null; request: ProtectionInventory } | null>(null);
  private request = 0;

  matchesInventory(owned: Map<string, OwnedRecord>, snapshotId: number | null) {
    return this.input?.owned === owned && this.input.snapshotId === snapshotId;
  }

  async setInventory(owned: Map<string, OwnedRecord>, snapshotId: number | null) {
    this.input = { owned, snapshotId, request: { snapshot_id: snapshotId, items: Object.fromEntries([...owned.values()]
      .filter(row => !row.subtype && !row.slug.endsWith('_set') && !row.slug.endsWith('_relic'))
      .map(row => [row.slug, { count: row.count, leveled: row.leveled ?? 0 }])) } };
    this.state = null;
    await this.refresh();
  }

  private disposed = false;

  async refresh() {
    const input = this.input;
    if (!input) return;
    const request = ++this.request;
    this.loading = true;
    try {
      const state = await this.port.desktopProtectionState(input.request);
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
