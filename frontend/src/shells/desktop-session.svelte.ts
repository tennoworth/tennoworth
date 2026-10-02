import { marketFreshness as marketFreshnessBucket } from '../ui/format';
import { staleSurfaceTimestamp } from '../domain/market';
import { onMount } from 'svelte';
import type { InventoryController } from '../features/inventory/controller.svelte';
import type { ListingController } from '../features/selling/controller.svelte';
import type { DesktopCapabilities } from '../contracts/desktop';
import type { DesktopServices } from '../contracts/services';
import type { StateStore } from '../contracts/state-store';
import { NOTIFICATIONS_EVENT, MARKET_REFRESHED_EVENT } from '../contracts/events';
import { TRAY_HINT_EVENT } from '../contracts/update';
import { startMarketRefreshLoop, type MarketRefreshLoop } from '../adapters/market';

export function createDesktopSession({ inventory, listing, transport, store, services }: { inventory: InventoryController; listing: ListingController; transport: DesktopCapabilities; store: StateStore; services: Pick<DesktopServices, 'desktopNotifications' | 'listenForTauriEvent' | 'desktopWfmStatus'> }) {
  const { desktopNotifications, listenForTauriEvent, desktopWfmStatus } = services;
  let desktopAppVersion = $state<string | null>(null);
  let desktopPlatform = $state<string | null>(null);
  let notesReady = $state(false);
  let trayHint = $state(false);

  let unreadNotifications = $state(0);
  onMount(() => {
    let active = true;
    let request = 0;
    const reload = async () => {
      const current = ++request;
      try { const rows = await desktopNotifications(); if (active && current === request) unreadNotifications = rows.filter(n => !n.read).length; } catch { /* Inbox exposes retryable errors. */ }
    };
    const stop = listenForTauriEvent(NOTIFICATIONS_EVENT, () => { void reload(); });
    const stopMarket = listenForTauriEvent(MARKET_REFRESHED_EVENT, async () => {
      const cached = await transport.loadCachedMarket().catch(() => null);
      if (active && cached && (!inventory.market || Date.parse(cached.updated_at) > Date.parse(inventory.market.updated_at))) inventory.market = cached;
    });
    void reload();
    return () => { active = false; stop(); stopMarket(); };
  });

  let marketRefreshLoop: MarketRefreshLoop | null = null;
  onMount(() => {
    const loop = startMarketRefreshLoop(() => inventory.refreshMarketInBackground());
    marketRefreshLoop = loop;
    return () => {
      if (marketRefreshLoop === loop) marketRefreshLoop = null;
      loop.stop();
    };
  });

  onMount(() => {
    return listenForTauriEvent(TRAY_HINT_EVENT, () => {
      if (store.getSetting('tray-toast-seen') !== '1') {
        trayHint = true;
        void store.setSetting('tray-toast-seen', '1');
      }
    });
  });

  // Restore the last snapshot exactly once after mount. Using onMount (not
  // $effect) is critical: $effect tracks any state read inside its body as
  // a dependency, so writing `resolved` here and then reading it via
  // recomputeResults caused an infinite re-run loop.
  // Interrupted-batch recovery, in one place so both callers agree on what a
  // failure means. A journal that is there but unreadable rejects: that is not
  // "no interrupted batch", and hiding it leaves a damaged saved batch with no
  // way for the user to learn about it or clear it.
  onMount(async () => {
    // A best-effort `health` invoke confirms wfm-core is linked and records the
    // platform for display; failure is non-fatal.
    try {
      const h = await transport.health();
      desktopPlatform = h?.platform ?? null;
      desktopAppVersion = h?.app_version ?? null;
    } catch (e) {
      console.error('desktop health check failed', e);
    }
    // C5 update-available handshake now lives entirely in
    // DesktopUpdateBanner.svelte's own onMount.
    // Interrupted-batch recovery: get_pending_plan is JWT-free, so this needs no
    // unlock. Read failures remain visible in the recovery state.
    await listing.refreshPendingPlan();

    await inventory.restore();

    // Cold landing (no saved inventory): preload the snapshot so the no-install
    // MarketBrowser has data to show. Best-effort - a failure just hides the
    // browser; the install steps below it still work.
    if (!inventory.market) {
      try {
        inventory.market = await inventory.loadBestMarket();
      } catch (e) {
        console.error(e);
      }
    }

    // Desktop only: start after the bundled/cached copy is on screen. The loop
    // retries on reconnect and every 30 minutes, so an offline launch recovers
    // without restarting; hosted builds already fetch same-origin from the box.
    marketRefreshLoop?.trigger();
    notesReady = true;
  });

  let displayNow = $state(Date.now());

  onMount(() => {
    const refreshClock = () => { displayNow = Date.now(); };
    const timer = window.setInterval(() => { if (!document.hidden) refreshClock(); }, 60_000);
    document.addEventListener('visibilitychange', refreshClock);
    window.addEventListener('focus', refreshClock);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener('visibilitychange', refreshClock);
      window.removeEventListener('focus', refreshClock);
    };
  });

  $effect(() => {
    void listing.sessionEpoch;
    desktopWfmStatus().then((s) => { listing.wfmStatus = s; }).catch(() => { listing.wfmStatus = null; });
  });

  let snapshotStamp = $derived.by(() => {
    const t = Date.parse(inventory.market?.updated_at ?? '');
    if (!Number.isFinite(t)) return '';
    return new Date(t).toISOString().slice(0, 16).replace('T', ' ') + ' UTC';
  });

  function ago(ts: string | number | null | undefined) {
    if (!ts || !Number.isFinite(new Date(ts).getTime())) return null;
    // Clamp at 0 - a cron runner with skewed clock can produce
    // `updated_at` in the future, which used to render "-120 min ago".
    const minutes = Math.max(0, Math.round((displayNow - new Date(ts).getTime()) / 60000));
    if (minutes < 1) return 'just now';
    if (minutes < 60) return `${minutes} min ago`;
    if (minutes < 60 * 24) return `${Math.round(minutes / 60)} h ago`;
    return `${Math.round(minutes / 1440)} d ago`;
  }

  let marketStaleness = $derived(ago(inventory.market?.updated_at));
  let inventoryStaleness = $derived(ago(inventory.lastUpdated));
  let inventoryTimestamp = $derived(inventory.lastUpdated && Number.isFinite(new Date(inventory.lastUpdated).getTime()) ? new Date(inventory.lastUpdated).toISOString() : null);
  // Same buckets as the market dot, on the inventory's own clock: a scan is
  // "fresh" for a day (inventories move slower than the order book).
  let inventoryFreshness = $derived.by(() => {
    if (!inventory.lastUpdated) return 'unknown';
    const h = (displayNow - inventory.lastUpdated) / 3.6e6;
    if (h <= 24) return 'fresh';
    if (h <= 24 * 7) return 'aging';
    return 'stale';
  });

  // Vendor surfaces can lag the price snapshot when their upstream fails.
  // A matching content hash is a successful freshness check even though the
  // retained payload keeps its original download timestamp.
  function surfaceAge(key: string) {
    const stamp = staleSurfaceTimestamp(inventory.market, key, displayNow);
    return stamp === 'unknown' ? 'age unknown' : stamp ? ago(stamp) : null;
  }
  let baroSurfaceAge = $derived(surfaceAge('baro'));
  let relicSurfaceAge = $derived(surfaceAge('relic_rewards'));
  let setSurfaceAge = $derived(surfaceAge('set_to_parts'));

  let marketFreshness = $derived(marketFreshnessBucket(inventory.market?.updated_at, displayNow));

  let wfmLabel = $derived(
    !listing.wfmStatus ? '-' : listing.wfmStatus.unlocked ? 'session live' : listing.wfmStatus?.logged_in ? 'locked' : 'logged out',
  );

  return {
    get snapshotStamp() { return snapshotStamp; },
    get marketStaleness() { return marketStaleness; },
    get inventoryStaleness() { return inventoryStaleness; },
    get inventoryTimestamp() { return inventoryTimestamp; },
    get inventoryFreshness() { return inventoryFreshness; },
    get baroSurfaceAge() { return baroSurfaceAge; },
    get relicSurfaceAge() { return relicSurfaceAge; },
    get setSurfaceAge() { return setSurfaceAge; },
    get marketFreshness() { return marketFreshness; },
    get wfmLabel() { return wfmLabel; },

    get notesReady() { return notesReady; },
    get desktopPlatform() { return desktopPlatform; },
    get desktopAppVersion() { return desktopAppVersion; },
    get trayHint() { return trayHint; },
    set trayHint(value: typeof trayHint) { trayHint = value; },
    get unreadNotifications() { return unreadNotifications; },
    get displayNow() { return displayNow; },
  };
}
