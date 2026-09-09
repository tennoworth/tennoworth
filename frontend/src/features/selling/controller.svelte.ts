import { WFM_ACCESS_EVENT, type AccessStatus } from '../../contracts/generated/desktop';
import type { DesktopServices } from '../../contracts/services';
import { DesktopCmdError } from '../../contracts/errors';
import { type DesktopCapabilities, type DesktopWfmStatus } from '../../contracts/desktop';
import type { PendingPlan, ItemResult } from '../../contracts/data';
import type { ListingCandidate } from '../../contracts/listing';
import { humanError } from '../../contracts/errors';
type Port = Pick<DesktopCapabilities, 'resumePendingPlan' | 'discardPendingPlan' | 'getPendingPlan'> & { status(): Promise<DesktopWfmStatus>; logout(): Promise<void> };

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
    if (this.resumePhase === 'running') return;
    this.resumePhase = 'running';
    this.resumeError = null;
    try {
      const resp = await this.port.resumePendingPlan();
      this.resumeResults = resp?.results ?? [];
      this.pendingPlan = await this.port.getPendingPlan();
      this.resumePhase = this.pendingPlan ? 'idle' : 'done';
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
    try { await this.port.discardPendingPlan(); } catch (error) { this.resumeError = humanError(error); this.resumePhase = 'error'; return; }
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

export class WfmAccessController {
  constructor(private port: Pick<DesktopServices, 'desktopAccessStatus' | 'listenForTauriEvent'>) {}
  status = $state<AccessStatus | null>(null);
  now = $state(Date.now());
  cooling = $derived(!!this.status && this.status.cooldown_until_ms > this.now);
  mutationsBlocked = $derived(this.cooling || !!this.status?.restrictions.pause_all || !!this.status?.restrictions.pause_mutations);
  message = $derived.by(() => {
    const status = this.status;
    if (!status) return null;
    if (this.cooling) {
      const deadline = new Date(status.cooldown_until_ms);
      return `Market access is cooling down${Number.isFinite(deadline.getTime()) ? ` until ${deadline.toLocaleString()}` : ''}. Unfinished listings stay saved and require Resume.`;
    }
    const rules = status.restrictions;
    const paused = [rules.pause_all ? 'all market access' : null, rules.pause_background ? 'background checks' : null, rules.pause_contracts ? 'contract searches' : null, rules.pause_mutations ? 'listing changes' : null, rules.pause_websockets ? 'live watch updates' : null].filter(Boolean);
    const reason = status.reason.trim();
    if (paused.length) return `Paused: ${paused.join(', ')}.${reason ? ` ${reason}${/[.!?]$/.test(reason) ? '' : '.'}` : ''} Cached inventory and prices remain available.`;
    if (status.queue_count > 0) return `Waiting for market access · ${status.queue_count} queued. Your edits are preserved.`;
    return null;
  });
  start() {
    let live = true;
    let receivedEvent = false;
    const stop = this.port.listenForTauriEvent<AccessStatus>(WFM_ACCESS_EVENT, status => { receivedEvent = true; this.status = status; });
    void this.port.desktopAccessStatus().then(status => { if (live && !receivedEvent) this.status = status; }).catch(() => {});
    const timer = setInterval(() => { this.now = Date.now(); }, 1000);
    return () => { live = false; stop(); clearInterval(timer); };
  }
}
