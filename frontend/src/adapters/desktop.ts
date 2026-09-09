import { DesktopCmdError } from '../contracts/errors';
import type { PingResponse, PlanItemInput, OrderPatch, PendingPlan, PlanResponse, ItemResult, Market, OverlaySettings, OverlayStatus } from '../contracts/data';
import { isHistory, type History } from '../domain/history';
import type { MarketRefreshResult, ScanReport, DesktopCapabilities, DesktopWfmStatus, LiveTopQuery, LiveTop, RivenAuction, Watch, NewWatch, WatchOutcome, TradeRow, EeLogStatus, NotificationEntry, NotificationPreferences } from '../contracts/desktop';
import { resolveInvoke, rethrowInvoke } from './runtime';

export async function desktopProtectionState(): Promise<import('../contracts/protection').ProtectionState> {
  try { return await resolveInvoke()('protection_state'); } catch (error) { return rethrowInvoke(error); }
}

export async function desktopSaveProtectionPlan(plan: import('../contracts/protection').ProtectionPlan): Promise<void> {
  try { await resolveInvoke()('save_protection_plan', { plan }); } catch (error) { rethrowInvoke(error); }
}

/** Native operations preserve command error codes for authentication routing. */
export class TauriTransport implements DesktopCapabilities {
  private activePlanRequest: string | null = null;
  async getOverlaySettings(): Promise<OverlaySettings> {
    return await resolveInvoke()<OverlaySettings>('get_overlay_settings');
  }

  async updateOverlaySettings(settings: OverlaySettings): Promise<OverlaySettings> {
    return await resolveInvoke()<OverlaySettings>('update_overlay_settings', { settings });
  }

  async overlayStatus(): Promise<OverlayStatus> {
    return await resolveInvoke()<OverlayStatus>('overlay_status');
  }

  async setupOverlayCapture(): Promise<OverlayStatus> {
    return await resolveInvoke()<OverlayStatus>('setup_overlay_capture');
  }

  async previewRelicOverlay(): Promise<void> {
    await resolveInvoke()<void>('preview_relic_overlay');
  }

  async scanOverlayNow(): Promise<void> {
    await resolveInvoke()<void>('scan_overlay_now');
  }
  async openOverlayDiagnostics(): Promise<void> {
    await resolveInvoke()<void>('open_overlay_diagnostics');
  }
  async clearOverlayDiagnostics(): Promise<void> {
    await resolveInvoke()<void>('clear_overlay_diagnostics');
  }
  async reportScanIssue(error: string | null): Promise<ScanReport> {
    return await resolveInvoke()<ScanReport>('report_scan_issue', { error });
  }

  async health(): Promise<PingResponse> {
    return await resolveInvoke()<PingResponse>('health');
  }

  async fetchInventory(): Promise<unknown> {
    // The command returns the inventory JSON as a string (the exact bytes the
    // old CLI would write); a rejected invoke carries wfm-core's graceful
    // message (e.g. "Warframe doesn't appear to be running…").
    const json = await resolveInvoke()<string>('scan_inventory');
    return JSON.parse(json);
  }

  // The `cached_market` command returns the raw cached body (or null). Parse it
  // here; a corrupt cache (parse throws) reads as "no cache" so the caller falls
  // back to the bundled floor rather than crashing the boot.
  async loadCachedMarket(): Promise<Market | null> {
    const raw = await resolveInvoke()<string | null>('cached_market');
    if (!raw) return null;
    try {
      return JSON.parse(raw) as Market;
    } catch {
      return null;
    }
  }

  async refreshMarket(): Promise<MarketRefreshResult> {
    // The Rust command swallows all network/HTTP failures and returns a no-op
    // RefreshResult, so this rejects only on a genuine IPC fault. `body` is
    // present only when `updated`; parse it into the Market to swap in.
    const r = await resolveInvoke()<{
      updated: boolean;
      updated_at: string | null;
      etag: string | null;
      body: string | null;
    }>('refresh_market');
    const market = r.updated && r.body ? (JSON.parse(r.body) as Market) : undefined;
    return { updated: !!r.updated, updatedAt: r.updated_at ?? null, etag: r.etag ?? null, market };
  }

  async loadHistory(): Promise<History | null> {
    // Cached copy first (instant), then the conditional refresh; a refreshed
    // body wins. Both are Rust-side; the webview never fetches third-party.
    let best: History | null = null;
    try {
      const raw = await resolveInvoke()<string | null>('cached_history');
      if (raw) {
        const h = JSON.parse(raw) as History;
        if (isHistory(h)) best = h;
      }
    } catch {
      /* corrupt/absent cache → refresh decides */
    }
    try {
      const r = await resolveInvoke()<{ updated: boolean; body: string | null }>('refresh_history');
      if (r.updated && r.body) {
        const h = JSON.parse(r.body) as History;
        if (isHistory(h)) best = h;
      }
    } catch {
      /* IPC fault: keep whatever the cache gave us */
    }
    return best;
  }

  async cancelPlan(): Promise<void> {
    const requestId = this.activePlanRequest;
    if (!requestId) return;
    try { await resolveInvoke()('cancel_plan', { requestId }); } catch (error) { rethrowInvoke(error); }
  }
  async submitPlan(items: PlanItemInput[]): Promise<PlanResponse> {
    return this.runPlan('submit_plan', { items });
  }
  private async runPlan(command: string, args: Record<string, unknown> = {}): Promise<PlanResponse> {
    if (this.activePlanRequest) throw new DesktopCmdError('busy', 'A listing request is already running.');
    const requestId = crypto.randomUUID();
    this.activePlanRequest = requestId;
    try { return await resolveInvoke()<PlanResponse>(command, { ...args, requestId }); }
    catch (error) { return rethrowInvoke(error); }
    finally { if (this.activePlanRequest === requestId) this.activePlanRequest = null; }
  }
  async getPendingPlan(): Promise<PendingPlan | null> {
    // The command returns Option<PendingPlan> - null when there's nothing
    // queued, matching the HTTP path's 404 → null normalization.
    try {
      return await resolveInvoke()<PendingPlan | null>('get_pending_plan');
    } catch (e) {
      rethrowInvoke(e);
    }
  }
  async resumePendingPlan(): Promise<PlanResponse> {
    return this.runPlan('resume_pending_plan');
  }
  async discardPendingPlan(): Promise<unknown> {
    try {
      return await resolveInvoke()<null>('discard_pending_plan');
    } catch (e) {
      rethrowInvoke(e);
    }
  }
  async fetchOrders(): Promise<unknown> {
    try {
      return await resolveInvoke()<unknown>('fetch_orders');
    } catch (e) {
      rethrowInvoke(e);
    }
  }
  async updateOrder(orderId: string, patch: OrderPatch): Promise<unknown> {
    try {
      return await resolveInvoke()<unknown>('update_order', { orderId, patch });
    } catch (e) {
      rethrowInvoke(e);
    }
  }
  async deleteOrder(orderId: string): Promise<unknown> {
    try {
      return await resolveInvoke()<null>('delete_order', { orderId });
    } catch (e) {
      rethrowInvoke(e);
    }
  }
  async bulkVisibility(orderIds: string[], visible: boolean): Promise<{ results: ItemResult[] }> {
    try {
      const results = await resolveInvoke()<ItemResult[]>('bulk_visibility', { orderIds, visible });
      return { results };
    } catch (e) {
      rethrowInvoke(e);
    }
  }
}

export async function desktopWfmStatus(): Promise<DesktopWfmStatus> {
  try {
    return await resolveInvoke()<DesktopWfmStatus>('wfm_auth_status');
  } catch (e) {
    rethrowInvoke(e);
  }
}

export async function desktopWfmLogout(): Promise<void> {
  try {
    await resolveInvoke()<null>('wfm_logout');
  } catch (e) {
    rethrowInvoke(e);
  }
}

export async function desktopWfmLogin(
  email: string,
  password: string,
  passphrase: string,
  platform: string,
  remember: boolean,
): Promise<void> {
  try {
    await resolveInvoke()<null>('wfm_login', { email, password, passphrase, platform, remember });
  } catch (e) {
    rethrowInvoke(e);
  }
}

export async function desktopWfmUnlock(passphrase: string, remember: boolean): Promise<void> {
  try {
    await resolveInvoke()<null>('unlock_jwt', { passphrase, remember });
  } catch (e) {
    rethrowInvoke(e);
  }
}

/**
 * Try the OS-keyring "remember on this device" key before showing the
 * passphrase modal. Never throws for a miss - false just means "ask the
 * human"; a genuine IPC fault still rethrows so the caller's fallback
 * (open the modal) runs.
 */
export async function desktopTrySilentUnlock(): Promise<boolean> {
  try {
    return await resolveInvoke()<boolean>('try_silent_unlock');
  } catch (e) {
    rethrowInvoke(e);
  }
}

/**
 * Exact-tier live asks/bids for up to 100 items, paced at WFM's 3 req/s
 * (≈17 s per 50) - listen on `LIVE_TOP_PROGRESS_EVENT` for `{done,total}`.
 */
export async function desktopLiveTopPrices(queries: LiveTopQuery[]): Promise<LiveTop[]> {
  try {
    return await resolveInvoke()<LiveTop[]>('live_top_prices', { queries });
  } catch (e) {
    rethrowInvoke(e);
  }
}

/**
 * The ≤20 cheapest matching auctions for one weapon's rivens, from WFM's v1
 * auctions search. The desktop paces calls through its shared 10/min auction
 * gate, so rapid "Show comps" clicks queue politely instead of tripping WFM.
 */
export async function desktopRivenComps(weapon: string): Promise<RivenAuction[]> {
  try {
    return await resolveInvoke()<RivenAuction[]>('riven_comps', { weapon });
  } catch (e) {
    rethrowInvoke(e);
  }
}

export async function desktopListWatches(): Promise<Watch[]> {
  try { return await resolveInvoke()<Watch[]>('list_watches'); } catch (e) { rethrowInvoke(e); }
}

export async function desktopAddWatch(watch: NewWatch): Promise<Watch[]> {
  try { return await resolveInvoke()<Watch[]>('add_watch', { watch }); } catch (e) { rethrowInvoke(e); }
}

export async function desktopDeleteWatch(id: number): Promise<Watch[]> {
  try { return await resolveInvoke()<Watch[]>('delete_watch', { id }); } catch (e) { rethrowInvoke(e); }
}

export async function desktopCheckWatchesNow(): Promise<WatchOutcome[]> {
  try { return await resolveInvoke()<WatchOutcome[]>('check_watches_now'); } catch (e) { rethrowInvoke(e); }
}

export async function desktopListTrades(limit = 200): Promise<TradeRow[]> {
  try { return await resolveInvoke()<TradeRow[]>('list_trades', { limit }); } catch (e) { rethrowInvoke(e); }
}

export async function desktopEelogStatus(): Promise<EeLogStatus> {
  try { return await resolveInvoke()<EeLogStatus>('eelog_status'); } catch (e) { rethrowInvoke(e); }
}

export async function desktopTradeSessionState(): Promise<import('../contracts/data').TradeSessionState> {
  try { return await resolveInvoke()<import('../contracts/data').TradeSessionState>('trade_session_state'); }
  catch (e) { rethrowInvoke(e); }
}

export async function desktopNotifications(): Promise<NotificationEntry[]> {
  return resolveInvoke()<NotificationEntry[]>('list_notifications');
}

export async function desktopReadNotifications(id: number | null = null): Promise<void> {
  return resolveInvoke()<void>('mark_notifications_read', { id });
}

export async function desktopClearNotifications(): Promise<void> {
  return resolveInvoke()<void>('clear_notifications');
}

export async function desktopNotificationPreferences(): Promise<NotificationPreferences> {
  return resolveInvoke()<NotificationPreferences>('get_notification_preferences');
}

export async function desktopSaveNotificationPreferences(preferences: NotificationPreferences): Promise<NotificationPreferences> {
  return resolveInvoke()<NotificationPreferences>('set_notification_preferences', { preferences });
}

export async function desktopTestNotification(): Promise<string> {
  return resolveInvoke()<string>('test_notification');
}

export async function currentOverlayResult(): Promise<import('../contracts/data').RelicOverlayResult | null> {
  return resolveInvoke()<import('../contracts/data').RelicOverlayResult | null>('current_overlay_result');
}

export async function desktopAccessStatus(): Promise<import('../contracts/generated/desktop').AccessStatus> {
  try { return await resolveInvoke()('wfm_access_status'); } catch (error) { return rethrowInvoke(error); }
}
