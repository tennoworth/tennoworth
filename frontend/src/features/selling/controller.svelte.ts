import { DesktopCmdError } from '../../contracts/errors';
import { type DesktopCapabilities, type DesktopWfmStatus } from '../../contracts/desktop';
import type { PendingPlan, ItemResult } from '../../contracts/data';
import type { ListingCandidate } from '../../contracts/listing';
import { humanError } from '../../contracts/errors';
type Port = Pick<DesktopCapabilities, 'resumePendingPlan' | 'discardPendingPlan'> & { status(): Promise<DesktopWfmStatus>; logout(): Promise<void> };

export class ListingController {
  constructor(private port: Port, private requestAuth: (code: string, next?: string) => void) { }
  ordersSummary = $state<{ live: number; issues: number } | null>(null);
  wfmStatus = $state<{ logged_in: boolean; unlocked: boolean } | null>(null);
  pendingPlan = $state<PendingPlan | null>(null);
  resumePhase = $state<'idle' | 'running' | 'done' | 'error'>('idle');
  resumeError = $state<string | null>(null);
  resumeResults = $state<ItemResult[]>([]);
  sessionEpoch = $state(0);
  listingOpen = $state(false);
  reviewRowsOverride = $state<ListingCandidate[] | null>(null);
  async doResume() {
    this.resumePhase = 'running';
    this.resumeError = null;
    try {
      const resp = await this.port.resumePendingPlan();
      this.resumeResults = resp?.results ?? [];
      this.resumePhase = 'done';
      this.pendingPlan = null;
    } catch (e) {
      // Desktop locked-session rejection: keep the banner (the plan is still
      // pending) and open the matching auth dialog - Resume works after that.
      if (e instanceof DesktopCmdError && (e.code === 'needs_login' || e.code === 'needs_unlock')) {
        this.resumePhase = 'idle';
        this.requestAuth(e.code);
        return;
      }
      this.resumePhase = 'error';
      this.resumeError = humanError(e);
    }
  }
  async doDiscard() {
    try { await this.port.discardPendingPlan(); } catch { /* ignore */ }
    this.pendingPlan = null;
    this.resumePhase = 'idle';
    this.resumeResults = [];
  }
  handleWfmUnlocked(next: string | null) {
    this.sessionEpoch += 1;
    if (next === 'list') this.listingOpen = true;
  }
  async handleWfmLogout() {
    const previousStatus = this.wfmStatus;
    try {
      await this.port.logout();
    } catch (error) {
      this.wfmStatus = await this.port.status().catch(() => previousStatus);
      throw error;
    }
    this.wfmStatus = { logged_in: false, unlocked: false };
    this.ordersSummary = null;
    this.listingOpen = false;
    this.sessionEpoch += 1;
  }
  async openListingFlow(overrideRow: ListingCandidate | ListingCandidate[] | null = null) {
    this.reviewRowsOverride = Array.isArray(overrideRow) ? overrideRow : overrideRow ? [overrideRow] : null;
    try {
      const s = await this.port.status();
      if (s.unlocked) this.listingOpen = true;
      else this.requestAuth(s.logged_in ? 'needs_unlock' : 'needs_login', 'list');
    } catch (e) {
      // Status probe failed (IPC fault) - open the modal anyway; Send will
      // surface the typed code and route to the right dialog.
      console.error('wfm auth status check failed', e);
      this.listingOpen = true;
    }
  }
}
