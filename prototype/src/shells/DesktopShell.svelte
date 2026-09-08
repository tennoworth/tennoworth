<script lang="ts">
  import { loadMarket } from '../adapters/market';
  import { loadCatalogs } from '../adapters/catalogs';
  import { TauriTransport } from '../adapters/desktop';
  import { useDesktopServices } from '../ui/desktop-context';
  const { desktopNotifications, desktopWfmStatus, desktopWfmLogout, listenForTauriEvent, updateStatus } = useDesktopServices();
  
  import { onMount, untrack } from 'svelte';
  import Faq from './Faq.svelte';
  import { FilterController, type View } from '../features/selling/filters.svelte';
  import { ListingController } from '../features/selling/controller.svelte';
  import { InventoryController } from '../features/inventory/controller.svelte';
  import ListingReviewModal from '../features/selling/ListingReviewModal.svelte';
  import MyOrdersPanel from '../features/orders/MyOrdersPanel.svelte';
  import WatchlistPanel from '../features/watches/WatchlistPanel.svelte';
  import NotificationInbox from '../features/settings/NotificationInbox.svelte';
  
import { NOTIFICATIONS_EVENT, MARKET_REFRESHED_EVENT } from '../contracts/events';
  import LedgerPanel from '../features/ledger/LedgerPanel.svelte';
  import MarketBrowser from '../features/market-context/MarketBrowser.svelte';
  import DesktopUpdateBanner from '../ui/DesktopUpdateBanner.svelte';
  import WfmAuthDialogs from '../features/settings/WfmAuthDialogs.svelte';
  import ExportImportDialogs from '../features/inventory/ExportImportDialogs.svelte';
  import SellPane from '../features/selling/SellPane.svelte';
  import TradeSessionPane from '../features/selling/TradeSessionPane.svelte';
  import RivensPanel from '../features/rivens/RivensPanel.svelte';
  import ThemeSwitcher from '../ui/ThemeSwitcher.svelte';
  import SettingsPanel from '../features/settings/SettingsPanel.svelte';
  import { resolveRivens } from '../domain/rivens';
  import { adviseOwned } from '../domain/advisor';
  import { buildMetaDrift } from '../domain/meta-drift';
  import type { History } from '../domain/history';
  import { lookup, staleSurfaceTimestamp } from '../domain/market';
import { startMarketRefreshLoop, type MarketRefreshLoop } from '../adapters/market';
  import { sellableQty } from '../domain/sell-priority';
  import { computeResults as computeFilteredResults, computeAvailableTags, computeEmptyReason, type FilterState } from '../domain/filter-engine';
  import { PRESETS, presetStillMatches } from '../domain/presets';

  const APP_COMMIT = __APP_COMMIT__;
  import { deriveSetRecos } from '../domain/set-recos';
  import { deriveRelicPlan } from '../domain/relic-planner';
  import type { StateStore } from '../contracts/state-store';
  import type { ThemeController } from '../ui/theme';

  import { wfmItemUrl, baroLocation, humanWindow } from '../ui/format';
  import BaroBoard from '../features/market-context/BaroBoard.svelte';
  import BuildVsBuy from '../features/selling/BuildVsBuy.svelte';
  import TraderCalendar from '../features/market-context/TraderCalendar.svelte';
  import MetaDriftPanel from '../features/market-context/MetaDriftPanel.svelte';
  import RefinementLadder from '../features/relics/RefinementLadder.svelte';
  
import { TRAY_HINT_EVENT } from '../contracts/update';

  // Desktop (Tauri) vs hosted informational (browser) is decided ONCE at boot.
  // The hosted site is informational only: market data + the desktop showcase,
  // no files. Everything interactive - scan, list, orders, login - lives in
  // the desktop app, driven by the wfm_session commands.
  const isDesktop = true;
  const transport = new TauriTransport();

  // Persistence seam: localStorage in the browser, SQLite-over-IPC in desktop.
  // Selected + primed (scalar settings loaded into cache) in main.ts and passed
  // in, so the scalar-setting `$state` initializers below can read it
  // synchronously with no first-paint flash. Snapshot methods are async.
  // `theme` is the boot-time ThemeController (src/lib/theme.ts) that the mode
  // control in Settings → Appearance (and its footer twin) drives.
  let { store, theme }: { store: StateStore; theme: ThemeController } = $props();
  const filters = untrack(() => new FilterController(store));
  const inventory = untrack(() => new InventoryController(store, transport, { loadMarket, loadCatalogs }));
  const listing = new ListingController({ resumePendingPlan: () => transport.resumePendingPlan(), discardPendingPlan: () => transport.discardPendingPlan(), status: desktopWfmStatus, logout: desktopWfmLogout }, (code, next) => wfmAuthDialogsRef?.open(code, next));

  let resolvedRivens = $derived(resolveRivens(inventory.ownedRivens, inventory.market));
  
  import type { SellRow } from '../contracts/selling';
  let results = $state<SellRow[]>([]);
  // The Sell table pushes its filtered+sorted rows up here so the "List on WFM"
  // CTA stages exactly what the user sees (name filter + badge chips), not the
  // unfiltered preset results.
  let tableView = $state<{ rows: SellRow[]; active: boolean }>({ rows: [], active: false });
  // Rows eligible for the bulk "List on WFM" action: the table-filtered set when
  // a table filter is active, else all results - minus relics (subtyped rows),
  // since selling an intact relic at a few plat usually loses to cracking it
  // (the Relic planner ranks those), so they shouldn't be staged by default.
  let listableRows = $derived(
    (tableView.active ? tableView.rows : results).filter((r) => !r.subtype && r.sellable > 0)
  );

  function headerClearance(node: HTMLElement, inShell: boolean) {
    if (!inShell) return;
    const root = document.documentElement;
    const update = () => root.style.setProperty('--sticky-header-clearance', `${node.offsetHeight}px`);
    const observer = new ResizeObserver(update);
    observer.observe(node);
    update();
    return { destroy() { observer.disconnect(); root.style.removeProperty('--sticky-header-clearance'); } };
  }

  // Sidebar nav: if the user's persisted view is unavailable (Baro not
  // visiting, orders on the informational site), fall back to Sell rather than
  // rendering an empty pane. The nav itself hides those entries; this protects
  // against a stale localStorage value.
  let effectiveView = $derived.by<View>(() => {
    if (filters.view === 'baro' && !showBaroCard) return 'sell';
    if (filters.view === 'meta' && !buildMetaDrift(inventory.market)) return 'sell';
    if ((filters.view === 'session' || filters.view === 'orders' || filters.view === 'watches' || filters.view === 'ledger' || filters.view === 'notifications' || filters.view === 'rivens') && !isDesktop) return 'sell';
    return filters.view;
  });

  // Tray hint banner - set by the Rust tray-hint event when the user closes the
  // window while the tray still exists. Once-ever, persisted when SHOWN (not on
  // dismiss); the Rust side caps within-session duplicates with an AtomicBool.
  let trayHint = $state(false);

  // PRESETS itself, plus the pure lookup/matching logic, live in
  // lib/presets.ts. `columns` is the ordered visible-column list;
  // missing = all columns (Default).
  let visibleColumns = $derived<string[] | null>(filters.activePreset ? PRESETS[filters.activePreset ?? '']?.columns ?? null : null);
  // A preset's optional default sort, handed to ResultsTable. Stable object
  // identity per preset → switching presets re-applies it; header clicks don't.
  // Spread a fresh object so the derived's identity changes whenever it
  // recomputes - re-selecting a preset then re-applies its sort.
  let presetSort = $derived.by(() => {
    const sort = filters.activePreset ? PRESETS[filters.activePreset]?.defaultSort : null;
    return sort ? { ...sort } : null;
  });
  // The filter cascade's inputs, bundled for lib/filter-engine.ts - the
  // hand-set sliders/chips plus whatever the active preset restricts
  // (vault-only, ducats-only, min-volume, min-median).
  // Tradeable copies per slug|refinement from the latest scan - the My orders
  // panel's "you list ×5 but own 2" / "not owned" checks. Null without a scan
  // so those checks stay silent rather than calling every listing a ghost.
  let ownedQtyForOrders = $derived.by((): Map<string, number> | null => {
    if (!inventory.resolved.owned.size) return null;
    const m = new Map<string, number>();
    for (const rec of inventory.resolved.owned.values()) {
      m.set(`${rec.slug}|${rec.subtype ?? ''}`, Math.max(0, rec.count - (rec.leveled ?? 0)));
    }
    return m;
  });

  let filterState = $derived<FilterState>({
    minPrice: filters.minPrice, minOwned: filters.minOwned, typeFilter: filters.typeFilter, hideAtLvl: filters.hideAtLvl, activeTags: filters.activeTags,
    vaultOnly: !!PRESETS[filters.activePreset ?? '']?.vaultOnly,
    ducatsOnly: !!PRESETS[filters.activePreset ?? '']?.ducatsOnly,
    minVol: PRESETS[filters.activePreset ?? '']?.minVol ?? 0,
    minMedian: PRESETS[filters.activePreset ?? '']?.minMedian ?? 0,
    typesAny: PRESETS[filters.activePreset ?? '']?.typesAny ?? [],
    sparesOnly: !!PRESETS[filters.activePreset ?? '']?.sparesOnly,
    adviceOnly: !!PRESETS[filters.activePreset ?? '']?.adviceOnly,
  });

  // ---- Hold-or-sell advisor inputs ----
  // The year-long history loads once, on demand, the first time a surface
  // that uses advice opens (the Hold/Sell preset or the Set picks view) -
  // same lazy pattern as the market browser's 1-year toggle. Verdicts
  // degrade gracefully to the calendar-only rules until it lands.
  let advisorHistory = $state<History | null>(null);
  let advisorHistoryState = $state<'idle' | 'loading' | 'done'>('idle');
  $effect(() => {
    const wanted = filters.activePreset === 'holdsell' || effectiveView === 'sets';
    if (wanted && advisorHistoryState === 'idle') {
      advisorHistoryState = 'loading';
      transport.loadHistory().then((h) => {
        advisorHistory = h;
        advisorHistoryState = 'done';
      }).catch(() => { advisorHistoryState = 'done'; });
    }
  });
  // Verdicts per owned slug (calendar-dated primes only). Cheap: one
  // set_to_parts index + a rule walk per owned slug.
  let adviceMap = $derived.by(() => {
    if (!inventory.resolved.owned.size || !inventory.market?.calendar?.primes) return new Map();
    const slugs = [...inventory.resolved.owned.values()].map((r) => r.slug);
    return adviseOwned(slugs, inventory.market, advisorHistory, Date.now());
  });
  
  $effect(() => {
    // Depend ONLY on the filter primitives that define a preset (the void reads
    // below). Read/write activePreset inside untrack() so nulling the selection
    // when the user hand-edits a filter can't re-trigger this effect - the old
    // version read AND wrote activePreset in the same body, which re-fired it
    // (flagged in the audit).
    void filters.minPrice; void filters.minOwned; void filters.hideAtLvl; void filters.typeFilter; void filters.activeTags.size;
    untrack(() => {
      if (filters.activePreset === null) return;
      if (!presetStillMatches(filters.activePreset, { minPrice: filters.minPrice, hideAtLvl: filters.hideAtLvl, typeFilter: filters.typeFilter, activeTags: filters.activeTags })) {
        filters.activePreset = null;
      }
    });
  });

  let unreadNotifications = $state(0);
  onMount(() => {
    if (!isDesktop) return;
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
    if (!isDesktop) return;
    const loop = startMarketRefreshLoop(() => inventory.refreshMarketInBackground());
    marketRefreshLoop = loop;
    return () => {
      if (marketRefreshLoop === loop) marketRefreshLoop = null;
      loop.stop();
    };
  });

  onMount(() => {
    if (!isDesktop) return;
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
  onMount(async () => {
    // Desktop mode: a best-effort `health` invoke confirms wfm-core is linked
    // and records the platform for display; failure is non-fatal (the dashboard
    // still works). The hosted site is informational - it has no account
    // features.
    if (isDesktop) {
      try {
        const h = await transport.health();
        companionPlatform = h?.platform ?? null;
      } catch (e) {
        console.error('desktop health check failed', e);
      }
      // C5 update-available handshake now lives entirely in
      // DesktopUpdateBanner.svelte's own onMount.
      // Interrupted-batch recovery: get_pending_plan is JWT-free, so this needs
      // no unlock. Best-effort - a failure just hides the Resume banner.
      try {
        listing.pendingPlan = await transport.getPendingPlan();
      } catch (e) {
        console.error('desktop pending-plan check failed', e);
      }
    }

    if (isDesktop) await inventory.restore();

    // Cold landing (no saved inventory): preload the snapshot so the no-install
    // MarketBrowser has data to show. Best-effort - a failure just hides the
    // browser; the install steps below it still work.
    if (inventory.phase === 'idle' && !inventory.market) {
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
  });

  function handleClear() {
    void store.clearSnapshot();
    inventory.inventoryName = null;
    inventory.lastUpdated = null;
    inventory.resolved = { owned: new Map(), unresolved: {} };
    inventory.ownedRivens = [];
    inventory.deltas = new Map();
    inventory.previousOwned = null;
    results = [];
    tableView = { rows: [], active: false };
    inventory.phase = 'idle';
  }

  // Re-derive results whenever any filter input or the owned set changes.
  // We deliberately read the filter state inside the effect (so they're
  // tracked) but write only to `results`, which the effect doesn't read -
  // no chance of a re-run loop. The filter cascade itself lives in
  // lib/filter-engine.ts (shared with availableTags + emptyReason below).
  //
  // The several independent walks over `owned` here and below look like an
  // obvious merge target. Measured 2026-08-01 with a 2,165-item inventory:
  // 0.1 ms median / 0.2 ms max of synchronous JS per filter change. The ~33 ms
  // a slider drag actually costs is Svelte's flush and the table repaint, which
  // merging the walks does not touch. Don't trade this cascade's clarity for
  // it without measuring again.
  $effect(() => {
    filterState; filters.reserveCopies;                   // track filter changes
    if (inventory.resolved.owned.size && inventory.market) {          // track owned + market readiness
      results = computeFilteredResults(inventory.resolved.owned, inventory.market, filterState, filters.reserveCopies, adviceMap);
    }
  });

  // Set-completion recommendations. Pure derivation from owned × market -
  // see lib/set-recos.js for the three reco kinds (near-complete /
  // complete-with-extras / extras). Computed lazily; cheap (one walk per
  // set in the catalog).
  let setRecos = $derived.by(() => {
    if (!inventory.resolved.owned.size || !inventory.market?.set_to_parts) return [];
    return deriveSetRecos(inventory.resolved.owned, inventory.market);
  });

  // Baro Ki'Teer schedule, baked into market.json at build time (mirrors
  // relic_rewards / vault_status). No runtime warframestat fetch - that
  // broke the resolver-only rule and vanished during warframestat
  // outages. Null until market loads, or when the bake came back empty.
  // Footer stamp: the snapshot's own timestamp, in UTC so it matches the
  // scraper's log lines.
  let snapshotStamp = $derived.by(() => {
    const t = Date.parse(inventory.market?.updated_at ?? '');
    if (!Number.isFinite(t)) return '';
    return new Date(t).toISOString().slice(0, 16).replace('T', ' ') + ' UTC';
  });

  let voidTrader = $derived.by(() => {
    const b = inventory.market?.baro;
    if (!b) return null;
    return { ...b, location: baroLocation(b.location) };
  });

  // Total ducats across the user's currently-sellable inventory.
  // Only count rows that resolved to a market entry with ducats > 0;
  // skip relic refinements (subtype set) since those aren't a ducat
  // trade. Cap presented as `count_owned × ducats`.
  let ducatStats = $derived.by(() => {
    if (!inventory.resolved.owned.size || !inventory.market) return { count: 0, total: 0 };
    let count = 0, total = 0;
    for (const rec of inventory.resolved.owned.values()) {
      if (rec.subtype) continue;
      const m = inventory.market.items?.[rec.slug];
      const d = m?.ducats;
      if (typeof d === 'number' && d > 0) {
        count += rec.count;
        total += rec.count * d;
      }
    }
    return { count, total };
  });

  // Render the Baro card when (a) we got a voidTrader response and
  // (b) the user has a meaningful pile of ducat-earning inventory.
  // 500 ducats ≈ 5 prime junk parts; below that the card is noise.
  let showBaroCard = $derived(voidTrader != null && (isDesktop || ducatStats.total >= 500));

  // Pre-format strings so the template stays clean.
  let baroState = $derived.by(() => {
    if (!voidTrader) return null;
    const now = Date.now();
    const arr = Date.parse(voidTrader.activation);
    const exp = Date.parse(voidTrader.expiry);
    if (Number.isFinite(exp) && now < exp && Number.isFinite(arr) && now >= arr) {
      // Baro is currently visiting.
      const leavesIn = exp - now;
      return { phase: 'here', label: 'Baro is here', windowMs: leavesIn };
    }
    if (Number.isFinite(arr) && now < arr) {
      return { phase: 'incoming', label: 'Baro arrives in', windowMs: arr - now };
    }
    return { phase: 'unknown', label: 'Next Baro visit', windowMs: null };
  });

  // Daily/weekly profit-routine clocks. Warframe resets daily at 00:00 UTC
  // and weekly Monday 00:00 UTC; we show only countdowns + static reminders,
  // never completion state (acts done / Endo banked are account state the
  // inventory+market snapshot can't carry). Date.now() isn't reactive, so
  // these recompute on load / view change - the same non-ticking model as the
  // Baro card, which is fine for a "next reset in ~Xh" reminder.
  let routinesState = $derived.by(() => {
    const now = Date.now();
    const d = new Date(now);
    const nextDaily = Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate() + 1);
    const daysToMon = ((8 - d.getUTCDay()) % 7) || 7; // 0=Sun..6=Sat → next Mon
    const nextWeekly = Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate() + daysToMon);
    return { dailyMs: nextDaily - now, weeklyMs: nextWeekly - now };
  });

  // Relic planner - top 3 owned (Intact) relics by expected-plat-per-crack.
  let relicPlan = $derived.by(() => {
    if (!inventory.resolved.owned.size || !inventory.market?.relic_rewards) return [];
    return deriveRelicPlan(inventory.resolved.owned, inventory.market, Infinity);
  });
  const RELIC_PREVIEW = 6;
  let relicShowAll = $state(false);
  let relicVisible = $derived(relicShowAll ? relicPlan : relicPlan.slice(0, RELIC_PREVIEW));

  // Routine checklist is collapsed by default - the clocks carry the daily
  // urgency, the three routine cards are a long-read. Not persisted
  // (collapsed-by-default is the intent).
  let routineChecklistOpen = $state(false);

  // Available tags = every tag that appears on a row surviving the OTHER
  // filters (price/owned/type/kept), with its live count. Empty chips
  // (count 0) are still rendered (strikethrough) so the user can see what
  // categories exist in their inventory rather than wondering where they
  // went. Sorted by count desc, then alphabetical.
  let availableTags = $derived.by(() => {
    if (!inventory.resolved.owned.size || !inventory.market) return [];
    return computeAvailableTags(inventory.resolved.owned, inventory.market, filterState, adviceMap);
  });

  // Auto-derived options for the type dropdown: every category that has at
  // least one sellable item. Built off owned + market (not `results`), so it
  // doesn't shrink when the user narrows by min-price / min-owned.
  let availableTypes = $derived.by(() => {
    if (!inventory.resolved.owned.size || !inventory.market) return [];
    const set = new Set<string>();
    for (const rec of inventory.resolved.owned.values()) {
      if (lookup(inventory.market, rec.slug)) set.add(rec.type || 'Unknown');
    }
    return [...set].sort();
  });

  // Total + per-category breakdown of paths no catalog could price-match.
  // The number itself is reassurance ("the app saw these and skipped them,
  // your prime junk isn't missing"), the breakdown is hover detail.

  let unresolvedCount = $derived(
    Object.values(inventory.resolved.unresolved).reduce((s, n) => s + n, 0)
  );

  // Every owned row with ANY market match - no preset/filter applied. The
  // sidebar Sell badge pins to this so it stays stable while filters change.
  let sellableCount = $derived.by(() => {
    if (!inventory.market) return 0;
    let n = 0;
    // Count an item only if at least one copy is actually listable after the
    // keep-copies reserve and leveled (untradeable) copies - otherwise the
    // headline tile would claim items are sellable that the reserve holds back,
    // contradicting the dimmed rows and zeroed potential.
    for (const rec of inventory.resolved.owned.values())
      if (lookup(inventory.market, rec.slug) && sellableQty(rec.count, filters.reserveCopies, rec.leveled ?? 0) > 0) n += 1;
    return n;
  });
  let unresolvedSummary = $derived(
    Object.entries(inventory.resolved.unresolved)
      .map(([k, v]) => `${k}: ${v}`)
      .join(', ')
  );

  function ago(ts: string | number | null | undefined) {
    if (!ts) return null;
    // Clamp at 0 - a cron runner with skewed clock can produce
    // `updated_at` in the future, which used to render "-120 min ago".
    const minutes = Math.max(0, Math.round((Date.now() - new Date(ts).getTime()) / 60000));
    if (minutes < 1) return 'just now';
    if (minutes < 60) return `${minutes} min ago`;
    if (minutes < 60 * 24) return `${Math.round(minutes / 60)} h ago`;
    return `${Math.round(minutes / 1440)} d ago`;
  }

  let marketStaleness = $derived(ago(inventory.market?.updated_at));
  let inventoryStaleness = $derived(ago(inventory.lastUpdated));
  // Same buckets as the market dot, on the inventory's own clock: a scan is
  // "fresh" for a day (inventories move slower than the order book).
  let inventoryFreshness = $derived.by(() => {
    if (!inventory.lastUpdated) return 'unknown';
    const h = (Date.now() - inventory.lastUpdated) / 3.6e6;
    if (h <= 24) return 'fresh';
    if (h <= 24 * 7) return 'aging';
    return 'stale';
  });

  // Vendor surfaces can lag the price snapshot when their upstream fails.
  // A matching content hash is a successful freshness check even though the
  // retained payload keeps its original download timestamp.
  function surfaceAge(key: string) {
    const stamp = staleSurfaceTimestamp(inventory.market, key, Date.now());
    return stamp ? ago(stamp) : null;
  }
  let baroSurfaceAge = $derived(surfaceAge('baro'));
  let relicSurfaceAge = $derived(surfaceAge('relic_rewards'));
  let setSurfaceAge = $derived(surfaceAge('set_to_parts'));

  // Coarse freshness bucket for the small status dot next to "market Xh ago".
  // The scrape cron runs every 2h, so a healthy snapshot is under 3h old -
  // "fresh" means exactly that. Calling a 5h-old book "fresh" during an
  // event-week price spike would be generous to the point of misleading.
  let marketFreshness = $derived.by<'unknown' | 'fresh' | 'aging' | 'stale'>(() => {
    if (!inventory.market?.updated_at) return 'unknown';
    const h = (Date.now() - new Date(inventory.market.updated_at).getTime()) / 3.6e6;
    if (h <= 3) return 'fresh';
    if (h <= 24) return 'aging';
    return 'stale';
  });

  // Total theoretical plat across visible results - for the stats strip.
  let totalPotential = $derived(
    results.reduce((s, r) => s + r.potential_plat, 0)
  );

  // Since-last-scan deltas for the Sell summary cells. The previous snapshot is
  // pushed through the SAME filter cascade as the live one, so "Sellable ▲12"
  // means twelve more rows under the current preset/filters - not a different
  // definition of sellable. One extra computeResults per filter change; the
  // cascade costs ~0.1 ms on a 2k-item inventory (measured 2026-08-01).
  let prevSummary = $derived.by(() => {
    if (!inventory.previousOwned || !inventory.market) return null;
    const rows = computeFilteredResults(inventory.previousOwned, inventory.market, filterState, filters.reserveCopies);
    return {
      owned: inventory.previousOwned.size,
      sellable: rows.filter((r) => r.sellable > 0).length,
      potential: rows.reduce((s, r) => s + r.potential_plat, 0),
    };
  });
  // Row-level "what changed": keys new since the last scan, keys gone, keys
  // whose count moved. `deltas` only covers keys present now (diffOwned walks
  // the current map), so removals come from the previous map directly.
  let sinceScan = $derived.by(() => {
    if (!inventory.previousOwned) return null;
    let added = 0, removed = 0, changed = 0;
    for (const [key, d] of inventory.deltas) {
      if (!inventory.previousOwned.has(key)) added += 1;
      else if (d !== 0) changed += 1;
    }
    for (const key of inventory.previousOwned.keys()) if (!inventory.resolved.owned.has(key)) removed += 1;
    return { added, removed, changed };
  });


  $effect(() => {
    if (!isDesktop) return;
    void listing.sessionEpoch;
    desktopWfmStatus().then((s) => { listing.wfmStatus = s; }).catch(() => { listing.wfmStatus = null; });
  });
  let wfmLabel = $derived(
    !listing.wfmStatus ? '-' : listing.wfmStatus.unlocked ? 'session live' : listing.wfmStatus?.logged_in ? 'locked' : 'logged out',
  );

  // Friendly diagnosis of WHY the table is empty so we don't just shrug.
  let emptyReason = $derived.by(() =>
    computeEmptyReason(inventory.resolved.owned, inventory.market, filterState, results.length, filters.activePreset, adviceMap)
  );

  // Scan is the only inventory source - the refresh pop is a single action.
  let refreshOpen = $state(false);
  async function refreshFromGame() {
    refreshOpen = false;
    await inventory.pullInventory();
  }
  $effect(() => {
    if (!refreshOpen) return;
    const handler = (e: MouseEvent) => {
      const t = e.target instanceof Element ? e.target : null;
      if (!t?.closest('.refresh-pop, .refresh-trigger')) refreshOpen = false;
    };
    document.addEventListener('click', handler, true);
    return () => document.removeEventListener('click', handler, true);
  });

  // ---- Encrypted export / import ---------------------------------------
  // Path-of-Building style: passphrase-derived AES-GCM, no accounts. The
  // exported file decrypts back into the same {invName, owned} the UI
  // restores from localStorage on page load. The dialogs, their state, and
  // the encrypt/decrypt calls live in ExportImportDialogs.svelte; App.svelte
  // triggers them imperatively (the Export / Restore toolbar buttons) and
  // owns what a successful import means for its own state.
  let exportImportRef = $state<{ openExport(): void; pickImport(): void }>();

  // ---- Pending-plan recovery ----
            // {plan_id, started_at, items[]} | null
          // Platform the desktop session reports (from /health), for display.
  let companionPlatform = $state<string | null>(null);

  let pendingRemaining = $derived(
    listing.pendingPlan?.items?.filter((i) => i.status === 'pending').length ?? 0
  );
  let ordersToFix = $derived((listing.ordersSummary?.issues ?? 0) + (listing.pendingPlan && pendingRemaining > 0 ? pendingRemaining : 0));
  let pendingDone = $derived(
    listing.pendingPlan?.items?.filter((i) => i.status === 'ok').length ?? 0
  );

  let resumeOk = $derived(listing.resumeResults.filter((r) => r.status === 'ok').length);
  let resumeErr = $derived(listing.resumeResults.filter((r) => r.status !== 'ok').length);

  // Scan inventory straight from the running game and run it through the same
  // resolution pipeline - no file, no drag-in. Desktop only (the hosted site
  // is informational); the `scan_inventory` IPC command's rejection carries
  // the scanner's exact actionable message.
     // ---- Desktop WFM auth (login / unlock dialogs) -----------------------
  // The dialogs themselves, their state, and the login/unlock calls live in
  // WfmAuthDialogs.svelte; App.svelte triggers them imperatively (three call
  // sites: the Sell CTA below, doResume's needs_login/needs_unlock rejection,
  // and ListingReviewModal's onauthrequired) via this ref, and decides what
  // 'list' means on unlock (open the review modal).
  let wfmAuthDialogsRef = $state<{ open(code: string, next?: string | null): Promise<void> }>();
  let feedbackDialog: HTMLDialogElement;
  let feedbackVersion = $state<string | null>(null);
  const improvementUrl = 'https://github.com/tennoworth/tennoworth/issues/new?template=improvement.yml';
  let bugReportUrl = $derived.by(() => {
    const params = new URLSearchParams({
      template: 'bug-report.yml',
      version: feedbackVersion ? `${feedbackVersion} (build ${APP_COMMIT})` : `Build ${APP_COMMIT}`,
    });
    if (companionPlatform === 'windows') params.set('operating-system', 'Windows');
    if (companionPlatform === 'linux') params.set('operating-system', 'Linux');
    return `https://github.com/tennoworth/tennoworth/issues/new?${params}`;
  });

  async function openFeedback() {
    feedbackDialog.showModal();
    try {
      const status = await updateStatus();
      feedbackVersion = status?.current_version?.trim() || null;
    } catch {
      // Feedback must remain available when desktop metadata cannot be read.
      feedbackVersion = null;
    }
  }
</script>

<dialog data-shell bind:this={feedbackDialog} class="cryptobox feedback-dialog" aria-labelledby="feedback-title" aria-describedby="feedback-description">
  <form data-shell method="dialog">
    <header data-shell>
      <h3 data-shell id="feedback-title">Send feedback</h3>
      <p data-shell id="feedback-description">Help make TennoWorth more useful.</p>
    </header>
    <p data-shell class="feedback-note">What would you like to share?</p>
    <div data-shell class="feedback-options">
      <a data-shell href={bugReportUrl} target="_blank" rel="noopener noreferrer">
        <strong data-shell>Report a bug <span data-shell aria-hidden="true">↗</span></strong>
        <span data-shell>Something broke or didn’t work as expected.</span>
      </a>
      <a data-shell href={improvementUrl} target="_blank" rel="noopener noreferrer">
        <strong data-shell>Suggest an improvement <span data-shell aria-hidden="true">↗</span></strong>
        <span data-shell>Tell us what would make your next trade easier.</span>
      </a>
    </div>
    <p data-shell class="feedback-note">Opens GitHub · Account required · Reports are public.</p>
    <footer data-shell><button data-shell type="submit" class="btn">Close</button></footer>
  </form>
</dialog>

{#if inventory.phase !== 'done'}
<main data-shell class="landing" data-testid={isDesktop ? 'desktop-mode' : undefined}>
  {@render statusStrip(false)}
  <!-- One lede line under the strip (the strip's descriptor already says what
       this is). The old pitch paragraph / hero is gone - search is the first
       control. The theme switcher used to ride this line's right end; it now
       lives in Settings → Appearance, with a quiet copy in the site footer for
       visitors who never search their way into the shell. -->
  <header data-shell class="landing-head">
    <p data-shell class="lede">
      
        Scan your account and TennoWorth ranks <em data-shell>your</em> inventory by what to sell - until then, look anything up below.
      
    </p>
  </header>

  {@render generalBanners()}

  {#if inventory.phase === 'idle' || inventory.phase === 'loading'}
    
      <!-- The scan CTA leads the desktop empty state (fresh install AND
           post-Clear): scanning is the app's whole point, so it must never
           sit below the fold of the market browser - that's how a user ends
           up back on the manual file path. -->
      <section data-shell class="upsell-lead desktop-hero">
        <h2 data-shell>Get your personal sell list</h2>
        <p data-shell class="sub">
          With Warframe open and past the login screen, scan your account -
          TennoWorth ranks <em data-shell>your</em> inventory by what to sell right now.
        </p>
        <div data-shell class="desktop-scan-row">
          <button data-shell
            class="rp-primary"
            data-testid="desktop-scan"
            onclick={() => inventory.pullInventory()}
            disabled={inventory.pullingInventory}
          >{inventory.pullingInventory ? 'Scanning game…' : 'Scan inventory'}</button>
        </div>
        <span data-shell class="trust">Reads the running game's memory only - nothing leaves your machine.</span>
      </section>
    

    {#if inventory.market}
      
        <MarketBrowser market={inventory.market} staleness={marketStaleness} freshness={marketFreshness} loadHistory={() => transport.loadHistory()} />
      
    {:else}{/if}
  {/if}

  {#if inventory.phase === 'error' && isDesktop}
    <div data-shell class="card ui-panel error">
      Error: {inventory.error}
      <div data-shell style="margin-top:10px">
        <button data-shell class="rp-primary" data-testid="desktop-scan" onclick={() => inventory.pullInventory()} disabled={inventory.pullingInventory}>
          {inventory.pullingInventory ? 'Scanning game…' : 'Scan inventory'}
        </button>
      </div>
    </div>
  {/if}

  <!-- The hosted landing reads as a price-lookup tool: search, movers,
       vaulted, hand-off. Nothing above this reveals that the app also does
       set picks, relics, rivens, watches, the ledger, orders and the
       advisor. The rail is that reveal - one miniature per surface, built
       from sample data rather than screenshots so it re-skins with the
       theme and can never go stale. Hosted only: a desktop visitor has the
       real thing in the sidebar. -->
  

  <Faq />

  <footer data-shell class="sitefoot">
    <span data-shell class="grow">TennoWorth is a fan project, not affiliated with Digital Extremes or warframe.market. Open source · MIT · data from warframe.market and warframestat.us.</span>
    {#if inventory.market?.updated_at}<span data-shell title="When the market snapshot was taken">Snapshot {snapshotStamp}</span>{/if}
    <a data-shell href="#trust">Trust &amp; safety</a>
    <span data-shell class="ver" title="build {APP_COMMIT}">{APP_COMMIT}</span>
    <!-- The theme control's home is Settings → Appearance, inside the shell.
         A visitor who never searches never reaches the shell, so the mode
         control also sits here - quiet, right-aligned, on the footer's own
         type scale - rather than leaving the hosted site unable to override
         the OS scheme. -->
    <div data-shell class="foot-theme"><ThemeSwitcher {theme} compact label="Colour mode" /></div>
  </footer>
</main>
{:else}
<div data-shell class="shell">
  {@render statusStrip(true)}

  <aside data-shell class="sidebar">
    <nav data-shell>
      <div data-shell class="nav-group">
        <div data-shell class="nav-label">Trade</div>
        <button data-shell type="button" class="nav-item" class:active={effectiveView === 'sell'} onclick={() => filters.setView('sell')}>
          <span data-shell>Sell</span>
          <!-- Pinned to the unfiltered sellable count: with a narrow preset
               active (Vaulted on a no-vaulted inventory), a filter-driven
               "Sell 0" reads as "your inventory got wiped". -->
          <span data-shell class="badge">{sellableCount}</span>
        </button>
        
          <button data-shell type="button" class="nav-item" class:active={effectiveView === 'session'} onclick={() => filters.setView('session')}><span data-shell>Trade Session</span></button>
        
        {#if setRecos.length > 0}
          <button data-shell type="button" class="nav-item" class:active={effectiveView === 'sets'} onclick={() => filters.setView('sets')}>
            <span data-shell>Set picks</span>
            <span data-shell class="badge">{setRecos.length}</span>
          </button>
        {/if}
        {#if relicPlan.length > 0}
          <button data-shell type="button" class="nav-item" class:active={effectiveView === 'relics'} onclick={() => filters.setView('relics')}>
            <span data-shell>Relics</span>
            <span data-shell class="badge">{relicPlan.length}</span>
          </button>
        {/if}
        {#if isDesktop && resolvedRivens.length > 0}
          <button data-shell type="button" class="nav-item" class:active={effectiveView === 'rivens'} onclick={() => filters.setView('rivens')}>
            <span data-shell>Rivens</span>
            <span data-shell class="badge">{resolvedRivens.length}</span>
          </button>
        {/if}
        {#if showBaroCard}
          <button data-shell type="button" class="nav-item baro-nav" class:active={effectiveView === 'baro'} onclick={() => filters.setView('baro')}>
            <span data-shell>Baro</span>
            {#if baroState?.phase === 'here'}<span data-shell class="badge here">here</span>{/if}
          </button>
        {/if}
        <button data-shell type="button" class="nav-item" class:active={effectiveView === 'routines'} onclick={() => filters.setView('routines')}>
          <span data-shell>Routines</span>
        </button>
        {#if buildMetaDrift(inventory.market)}
          <button data-shell type="button" class="nav-item" class:active={effectiveView === 'meta'} onclick={() => filters.setView('meta')}>
            <span data-shell>Meta Drift</span>
          </button>
        {/if}
      </div>

      
      <div data-shell class="nav-group">
        <div data-shell class="nav-label">Manage</div>
        <button data-shell type="button" class="nav-item" class:active={effectiveView === 'orders'} onclick={() => filters.setView('orders')}>
          <span data-shell>My orders</span>
          {#if listing.pendingPlan && pendingRemaining > 0}<span data-shell class="badge warn">{pendingRemaining}</span>{/if}
        </button>
        <button data-shell type="button" class="nav-item" class:active={effectiveView === 'watches'} onclick={() => filters.setView('watches')}>
          <span data-shell>Price watches</span>
        </button>
        <button data-shell type="button" class="nav-item" class:active={effectiveView === 'notifications'} onclick={() => filters.setView('notifications')}>
          <span data-shell>Notifications</span>{#if unreadNotifications}<span data-shell class="badge">{unreadNotifications}</span>{/if}
        </button>
        <button data-shell type="button" class="nav-item" class:active={effectiveView === 'ledger'} onclick={() => filters.setView('ledger')}>
          <span data-shell>Ledger</span>
        </button>
      </div>
      

      <div data-shell class="nav-group">
        <div data-shell class="nav-label">Library</div>
        <button data-shell type="button" class="nav-item" class:active={effectiveView === 'install'} onclick={() => filters.setView('install')}>
          <span data-shell>FAQ</span>
        </button>
        <button data-shell type="button" class="nav-item" class:active={effectiveView === 'settings'} onclick={() => filters.setView('settings')}>
          <span data-shell>Settings</span>
        </button>
      </div>
    </nav>

    <div data-shell class="sfoot">
      
        <button data-shell type="button" class="btn feedback-trigger" onclick={openFeedback}>
          <svg data-shell viewBox="0 0 24 24" aria-hidden="true"><path data-shell d="M4 4h16v12H9l-5 4V4Z" /><path data-shell d="M8 8h8M8 12h5" /></svg>
          Send feedback
        </button>
      
      
        <nav data-shell class="project-links" aria-label="Project links">
          {@render projectLinkAnchors()}
        </nav>
      
      <div data-shell class="ver" title="build {APP_COMMIT}">Windows + Linux · {APP_COMMIT}</div>
    </div>
  </aside>

  <main data-shell class="workspace">

    {@render generalBanners()}

    {#if effectiveView === 'sell'}
      <SellPane
        bind:minPrice={filters.minPrice} bind:minOwned={filters.minOwned} bind:typeFilter={filters.typeFilter} bind:hideAtLvl={filters.hideAtLvl} bind:activeTags={filters.activeTags}
        bind:tableView
        resolved={inventory.resolved} {results} deltas={inventory.deltas} {totalPotential}
        {prevSummary} {sinceScan} ordersSummary={listing.ordersSummary}
        {marketFreshness} {marketStaleness} marketLoadError={inventory.marketLoadError}
        {listableRows} {availableTags} {availableTypes}
        {visibleColumns} {presetSort} {emptyReason}
        activePreset={filters.activePreset} reserveCopies={filters.reserveCopies} filtersOpen={filters.filtersOpen} scoreExplainerDismissed={filters.scoreExplainerDismissed}
        sellOnboardingDismissed={filters.sellOnboardingDismissed} keepCopiesNudgeDismissed={filters.keepCopiesNudgeDismissed}
        {isDesktop}
        applyPreset={(name) => filters.applyPreset(name)} setReserveCopies={(value) => filters.setReserveCopies(value)} toggleFiltersOpen={(event) => filters.toggleFiltersOpen(event)}
        dismissSellOnboarding={() => filters.dismissSellOnboarding()} dismissKeepCopiesNudge={() => filters.dismissKeepCopiesNudge()}
        openListingFlow={(rows) => listing.openListingFlow(rows)}
        {pendingBanner}
      />
    {:else if effectiveView === 'session'}
      <TradeSessionPane owned={inventory.resolved.owned} market={inventory.market} reserveCopies={filters.reserveCopies} advice={adviceMap}
        scanning={inventory.pullingInventory} onscan={inventory.pullInventory} onreview={(rows, budget, state) => listing.openListingFlow(rows.map(r => ({
          ...r, proposed_quantity: r.quantity, clearing_price: r.platinum, low_sell: r.platinum,
          avg_price: r.market.avg, session: { snapshot_id: state.allowance.snapshot_id!, utc_day: state.allowance.utc_day, budget },
        })))} />
    {:else if effectiveView === 'sets'}
      <section data-shell class="view-header">
        <h2 data-shell>Set picks</h2>
        <p data-shell class="lede">
          Inventory cross-referenced against {Object.keys(inventory.market?.set_to_parts ?? {}).length}
          prime sets. Ranked by net plat.
          {#if setSurfaceAge}
            <span data-shell class="muted">· ⚠ set/vault data {setSurfaceAge}</span>
          {/if}
        </p>
      </section>
      {#if setRecos.length > 0}
        <section data-shell class="card ui-panel set-recos">
          {#each setRecos as r (r.set_slug)}
            <div data-shell class="reco row">
              <div data-shell class="reco-body">
                <div data-shell class="reco-title">
                  <strong data-shell class="reco-verb">
                    {#if r.kind === 'near-complete'}Complete{:else if r.kind === 'complete-with-extras'}List{:else}List{/if}
                  </strong>
                  <a data-shell
                    href={wfmItemUrl(r.set_slug)}
                    target="_blank"
                    rel="noopener noreferrer"
                  >{r.set_name}</a>
                  <span data-shell class="reco-net-inline">+{r.net_plat}p</span>
                  {#if adviceMap.get(r.set_slug)}
                    {@const av = adviceMap.get(r.set_slug)}
                    <span data-shell class="advice-chip advice-{av.advice}" title={av.reasons.join(' · ')}>
                      {av.advice === 'sell_now' ? 'sell now' : av.advice}
                    </span>
                  {/if}
                  <span data-shell class="kind kind-{r.kind}">
                    {#if r.kind === 'near-complete'}
                      own {r.parts.reduce((n, p) => n + Math.min(p.count, p.required), 0)}/{r.parts.reduce((n, p) => n + p.required, 0)}
                    {:else if r.kind === 'complete-with-extras'}
                      {r.extras} spare{r.extras === 1 ? '' : 's'} + full set
                    {:else}
                      {r.extras} duplicate{r.extras === 1 ? '' : 's'}
                    {/if}
                  </span>
                  {#if r.set_vol !== undefined && (r.kind === 'near-complete' || r.kind === 'complete-with-extras')}
                    {#if r.set_vol < 1}
                      <span data-shell class="set-liq cold" title="The assembled set has traded under 1×/48h - a flip may sit unsold for a while.">set rarely trades</span>
                    {:else if r.set_vol < 5}
                      <span data-shell class="set-liq thin" title="Thin set volume - expect to wait for a buyer before you recoup the plat.">thin · {r.set_vol}/48h</span>
                    {:else}
                      <span data-shell class="set-liq moving" title="Healthy set volume.">{r.set_vol}/48h</span>
                    {/if}
                  {/if}
                </div>
                <p data-shell class="reco-detail muted">
                  {#if r.kind === 'near-complete'}
                    {@const ownedCount = r.parts.reduce((n, p) => n + Math.min(p.count, p.required), 0)}
                    Buy {(r.missing ?? []).map((m) => `${m.quantity > 1 ? `${m.quantity}× ` : ''}${m.name}`).join(' + ')} at current asks for
                    <strong data-shell class="bad-text">{r.missing_cost}p</strong>, then list the set at the current lowest ask,
                    <strong data-shell class="good-text">{r.set_low_sell}p</strong>.
                    That is <strong data-shell class="good-text">+{r.net_plat}p potential uplift</strong> versus listing your
                    {ownedCount} owned part{ownedCount === 1 ? '' : 's'} for {r.parts_low_sell}p.
                    {#if r.set_top_buy !== undefined && r.instant_uplift !== undefined}
                      Selling instantly to the {r.set_top_buy}p top bid would be
                      <strong data-shell class:good-text={r.instant_uplift >= 0} class:bad-text={r.instant_uplift < 0}>{r.instant_uplift >= 0 ? '+' : '−'}{Math.abs(r.instant_uplift)}p</strong>
                      versus those parts.
                    {/if}
                  {:else if r.kind === 'complete-with-extras'}
                    You hold a full set plus {r.extras} spare blueprint{r.extras === 1 ? '' : 's'}.
                    List the extras at <strong data-shell>{r.extras_plat}p</strong>.
                  {:else}
                    Duplicates of partial-set parts. List the {r.extras} spare {r.extras === 1 ? 'copy' : 'copies'}:
                    <strong data-shell>{r.extras_plat}p</strong>.
                  {/if}
                </p>
                <!-- The third option the spread above cannot express: buy only
                     what you lack and foundry the rest. Only for sets you are
                     actually assembling - on a spares play there is nothing to
                     build. -->
                {#if r.kind === 'near-complete' && inventory.market?.set_to_parts?.[r.set_slug]}
                  <details data-shell class="build-vs-buy">
                    <summary data-shell>build it or buy it</summary>
                    <BuildVsBuy
                      setSlug={r.set_slug}
                      setName={r.set_name}
                      parts={inventory.market.set_to_parts[r.set_slug].parts}
                      market={inventory.market}
                      owned={inventory.resolved.owned}
                    />
                  </details>
                {/if}
              </div>
            </div>
          {/each}
        </section>
      {:else}
        <div data-shell class="card ui-panel empty">
          <div data-shell>
            <strong data-shell>No set recommendations.</strong>
            <p data-shell class="muted">You don't currently own enough prime parts to surface near-complete sets or spare-blueprint plays.</p>
          </div>
        </div>
      {/if}

    {:else if effectiveView === 'relics'}
      <section data-shell class="view-header">
        <h2 data-shell>Relic planner</h2>
        <p data-shell class="lede">
          {#if relicPlan.length > relicVisible.length}
            Top {relicVisible.length} of {relicPlan.length} relics you own, ranked by expected plat per solo crack (Intact); the ladder shows what refining would add.
          {:else}
            Your {relicPlan.length} relic{relicPlan.length === 1 ? '' : 's'} ranked by expected plat per solo crack (Intact); the ladder shows what refining would add.
          {/if}
          {#if relicSurfaceAge}
            <span data-shell class="muted">· ⚠ drop-table data {relicSurfaceAge}</span>
          {/if}
        </p>
      </section>
      {#if relicPlan.length > 0}
        <section data-shell class="card ui-panel relic-planner">
          <div data-shell class="relic-grid">
            {#each relicVisible as p (p.relic_slug)}
              <div data-shell class="relic-card">
                <div data-shell class="relic-title">
                  <strong data-shell class="reco-verb">Crack</strong>
                  <a data-shell
                    href={wfmItemUrl(p.relic_slug)}
                    target="_blank"
                    rel="noopener noreferrer"
                  >{p.relic_name}</a>
                  <span data-shell class="muted small">×{p.owned}</span>
                </div>
                <div data-shell class="relic-epp">
                  {p.epp.toFixed(1)}<span data-shell class="unit">p / crack</span>
                </div>
                <div data-shell class="relic-meta">
                  <span data-shell class:bad-text={p.moving_count < p.total_rewards / 2}>
                    {p.moving_count}/{p.total_rewards} rewards moving
                  </span>
                  <span data-shell class="muted">·</span>
                  <span data-shell title="If you cracked every one you own.">
                    {p.epp_owned.toFixed(0)}p total
                  </span>
                  {#if p.sell_now > 0}
                    <span data-shell class="muted">·</span>
                    <span data-shell
                      class:bad-text={p.sell_now > p.epp}
                      title="What this relic clears at sold intact on WFM, no cracking. When this beats the crack EV, selling wins."
                    >or sell: {p.sell_now.toFixed(0)}p ea</span>
                  {/if}
                </div>
                {#if p.decision}
                  <RefinementLadder decision={p.decision} />
                {/if}
                <details data-shell class="relic-rewards">
                  <summary data-shell>top drops</summary>
                  <ul data-shell>
                    {#each p.rewards.slice(0, 4) as r (r.slug)}
                      <li data-shell>
                        <span data-shell class="rarity rarity-{r.rarity.toLowerCase()}">{r.rarity[0]}</span>
                        <span data-shell class="reward-name">{r.name}</span>
                        <span data-shell class="muted small">{r.chance.toFixed(0)}%</span>
                        <span data-shell class={r.low_sell > 0 ? '' : 'muted'}>{r.low_sell || '-'}p</span>
                      </li>
                    {/each}
                  </ul>
                </details>
              </div>
            {/each}
          </div>
          {#if relicPlan.length > RELIC_PREVIEW}
            <div data-shell class="relic-more">
              <button data-shell class="ghost" onclick={() => (relicShowAll = !relicShowAll)}>
                {relicShowAll ? 'Show fewer' : `Show ${relicPlan.length - RELIC_PREVIEW} more`}
              </button>
            </div>
          {/if}
        </section>
      {:else}
        <div data-shell class="card ui-panel empty">
          <div data-shell>
            <strong data-shell>No relics in your inventory.</strong>
            <p data-shell class="muted">Once you pick up relics, this planner ranks them by expected plat per crack.</p>
          </div>
        </div>
      {/if}

    {:else if effectiveView === 'rivens'}
      <RivensPanel market={inventory.market} rivens={resolvedRivens} />
    {:else if effectiveView === 'baro'}
      <section data-shell class="view-header">
        <h2 data-shell>Baro Ki'Teer</h2>
        <p data-shell class="lede">
          {#if baroState?.phase === 'here'}
            Here at {voidTrader?.location} - leaves in {humanWindow(baroState.windowMs)}.
          {:else if baroState?.phase === 'incoming'}
            Arrives in {humanWindow(baroState.windowMs)} at {voidTrader?.location}.
          {:else}
            Next visit at {voidTrader?.location}.
          {/if}
          {#if baroSurfaceAge}
            <span data-shell class="muted">· ⚠ schedule data {baroSurfaceAge} - may be a rotation behind</span>
          {/if}
        </p>
      </section>
      <section data-shell class="card ui-panel baro-card" class:here={baroState?.phase === 'here'}>
        <div data-shell class="row">
          <div data-shell class="src">
            <span data-shell class="baro-icon" aria-hidden="true">⌬</span>
            <div data-shell class="baro-body">
              <p data-shell class="baro-detail">
                You hold <strong data-shell>{ducatStats.total.toLocaleString()}<span data-shell class="unit">d</span></strong>
                across <strong data-shell>{ducatStats.count.toLocaleString()}</strong>
                ducat-earning {ducatStats.count === 1 ? 'item' : 'items'}.
                {#if baroState?.phase === 'here'}
                  Spend them on Baro's offerings - open the <strong data-shell>Ducats</strong>
                  preset to see what's worth dumping.
                {:else}
                  Earmark these for Baro using the <strong data-shell>Ducats</strong> preset.
                {/if}
              </p>
            </div>
          </div>
          <div data-shell class="row gap-sm">
            <button data-shell onclick={() => { filters.setView('sell'); filters.applyPreset('ducats'); }}>Open Ducats preset →</button>
          </div>
        </div>
      </section>

      <!-- His actual stock, priced. Only rendered when the snapshot carries a
           manifest: worldState publishes one from announcement, but a snapshot
           built before that switch (or carried through a DE outage) may not
           have it, and an empty table would read as "he is selling nothing". -->
      {#if voidTrader?.inventory?.length}
        <section data-shell class="card ui-panel">
          <BaroBoard market={inventory.market} baro={voidTrader} owned={inventory.resolved.owned} />
        </section>
      {/if}

      <!-- Vault rotations and Darvo, from the same worldState poll. An
           unvaulting is the most expensive surprise in prime trading and it is
           announced days ahead. -->
      <section data-shell class="card ui-panel">
        <TraderCalendar market={inventory.market} owned={inventory.resolved.owned} />
      </section>

    {:else if effectiveView === 'routines'}
      <section data-shell class="view-header">
        <h2 data-shell>Profit routines</h2>
        <p data-shell class="lede">
          Daily and weekly habits that compound - including the Endo sources that fund the
          buy-unranked → max → resell flip. Countdowns are live; what you've already claimed
          isn't tracked (your inventory + the market snapshot can't see account state).
        </p>
      </section>

      <section data-shell class="card ui-panel">
        <TraderCalendar market={inventory.market} owned={inventory.resolved.owned} />
      </section>
      <section data-shell class="card ui-panel routine">
        <div data-shell class="routine-clocks">
          <div data-shell class="clock">
            <span data-shell class="clock-label">Daily reset</span>
            <strong data-shell class="clock-val">{humanWindow(routinesState.dailyMs)}</strong>
            <span data-shell class="clock-sub">00:00 UTC</span>
          </div>
          <div data-shell class="clock">
            <span data-shell class="clock-label">Weekly reset</span>
            <strong data-shell class="clock-val">{humanWindow(routinesState.weeklyMs)}</strong>
            <span data-shell class="clock-sub">Mon 00:00 UTC</span>
          </div>
          <div data-shell class="clock">
            <span data-shell class="clock-label">{baroState?.phase === 'here' ? 'Baro leaves' : 'Baro arrives'}</span>
            <strong data-shell class="clock-val">{voidTrader ? humanWindow(baroState?.windowMs) : '-'}</strong>
            <span data-shell class="clock-sub">{voidTrader?.location ?? 'schedule unknown'}</span>
          </div>
        </div>
      </section>

      <details data-shell class="routine-checklist" bind:open={routineChecklistOpen}>
        <summary data-shell>{routineChecklistOpen ? 'Hide checklist' : "Show me today's checklist"}</summary>

        <section data-shell class="card ui-panel routine">
          <h3 data-shell>Daily</h3>
        <ul data-shell class="routine-list">
          <li data-shell><strong data-shell>Login tribute</strong> - claim it; the milestone days hand out Endo and the exclusive weapons/Forma that fund everything else.</li>
          <li data-shell><strong data-shell>Keep the foundry busy</strong> - start a Forma or a sellable BP every day; an idle foundry is lost plat.</li>
          <li data-shell><strong data-shell>Cap syndicate standing</strong> → buy augment mods / arcanes to flip on WFM - a steady daily plat trickle.</li>
          <li data-shell><strong data-shell>6 Steel Path incursions</strong> → Steel Essence → Teshin's weekly rotation (Riven slivers, Kuva, Umbra Forma).</li>
          <li data-shell><strong data-shell>Sortie</strong> - ~4,000 Endo on the Endo reward, plus a Riven chance.</li>
        </ul>
      </section>

      <section data-shell class="card ui-panel routine">
        <h3 data-shell>Weekly <span data-shell class="muted">· resets Monday</span></h3>
        <ul data-shell class="routine-list">
          <li data-shell><strong data-shell>Maroo's Ayatan Treasure Hunt</strong> - a free sculpture worth ~1,500–3,450 Endo once filled with stars.</li>
          <li data-shell><strong data-shell>Archon Hunt</strong> - up to ~8,000 Endo in one clear, plus an Archon Shard.</li>
          <li data-shell><strong data-shell>Nightwave acts</strong> → Cred for potatoes/Forma. This <em data-shell>saves</em> plat (those items are account-bound) - it doesn't earn it.</li>
          <li data-shell><strong data-shell>Baro check</strong> on arrival - but buy to <strong data-shell>hold</strong>, not flip: his mods crater ~50% on arrival and recover over weeks (watch the Sell view's “hold” tags).</li>
        </ul>
      </section>

      <section data-shell class="card ui-panel routine">
        <h3 data-shell>Endo - to fund the rank-up flip</h3>
        <p data-shell class="routine-note">
          Maxing one Primed mod ≈ <strong data-shell>20,000 Endo + ~1.3M credits</strong> and roughly doubles its
          value (e.g. Primed Continuity ~69p unranked → ~139p maxed). Best sources:
        </p>
        <ul data-shell class="routine-list">
          <li data-shell><strong data-shell>Arbitrations</strong> - ~5,000–10,000 Endo/hr (the grind option; needs the full star chart cleared).</li>
          <li data-shell><strong data-shell>Vodyanoi</strong> (Sedna, Steel Path) - the throughput king; a coordinated squad pushes far higher.</li>
          <li data-shell><strong data-shell>Hieracon (Pluto) excavation</strong> - steady and solo-friendly, with relics as a byproduct.</li>
          <li data-shell><strong data-shell>Archon (~8k/wk) + Sortie (~4k/day) + Maroo's weekly</strong> - passive lumps from the routines above.</li>
          <li data-shell class="routine-avoid"><strong data-shell>Skip Eidolons &amp; Profit-Taker for Endo</strong> - they pay ~zero Endo; farm those for arcanes/plat instead.</li>
        </ul>
      </section>
      </details>

    {:else if effectiveView === 'meta'}
      <MetaDriftPanel market={inventory.market} />
    {:else if effectiveView === 'orders'}
      <section data-shell class="view-header">
        <h2 data-shell>My orders</h2>
        <p data-shell class="lede">Your active warframe.market listings, fetched live from the desktop app.</p>
      </section>
      {@render pendingBanner()}
      <MyOrdersPanel
        {transport}
        market={inventory.market}
        sessionEpoch={listing.sessionEpoch}
        ownedQty={ownedQtyForOrders}
        onauthrequired={(code) => wfmAuthDialogsRef?.open(code)}
        onsummary={(s) => (listing.ordersSummary = s)}
      />

    {:else if effectiveView === 'watches'}
      <section data-shell class="view-header">
        <h2 data-shell>Price watches</h2>
        <p data-shell class="lede">Desktop notifications when an item hits your price - checked every 10 minutes against live warframe.market orders.</p>
      </section>
      <WatchlistPanel market={inventory.market} />

    {:else if effectiveView === 'notifications'}
      <NotificationInbox onopen={(target) => filters.setView(target)} onsettings={() => filters.setView('settings')} />

    {:else if effectiveView === 'ledger'}
      <section data-shell class="view-header">
        <h2 data-shell>Ledger</h2>
        <p data-shell class="lede">Every trade the game confirmed, read from its own log - realised plat, not estimates.</p>
      </section>
      <LedgerPanel onsetautoclose={(on) => store.setSetting('auto-close-sold', on ? 'on' : 'off')} />

    {:else if effectiveView === 'install'}
      <section data-shell class="view-header">
        <h2 data-shell>FAQ</h2>
        <p data-shell class="lede">Answers to common questions.</p>
      </section>
      <Faq />

    {:else if effectiveView === 'settings'}
      <SettingsPanel {theme} {transport} {isDesktop} wfmStatus={listing.wfmStatus} onwfmlogout={() => listing.handleWfmLogout()} />
    {/if}

  </main>
</div>
{/if}

{#snippet projectLinkAnchors()}
  <a data-shell class="project-link" href="https://github.com/tennoworth/tennoworth" target="_blank" rel="noopener noreferrer">
    <svg data-shell class="github-mark" viewBox="0 0 24 24" aria-hidden="true">
      <path data-shell d="M12 2.7a9.5 9.5 0 0 0-3 18.5c.5.1.7-.2.7-.5v-1.9c-2.8.6-3.4-1.2-3.4-1.2-.5-1.2-1.1-1.5-1.1-1.5-.9-.6.1-.6.1-.6 1 0 1.6 1 1.6 1 .9 1.6 2.4 1.1 3 .8.1-.7.4-1.1.7-1.3-2.2-.3-4.6-1.1-4.6-4.7 0-1 .4-1.9 1-2.6-.1-.3-.4-1.3.1-2.6 0 0 .8-.3 2.7 1a9.2 9.2 0 0 1 4.9 0c1.9-1.3 2.7-1 2.7-1 .5 1.3.2 2.3.1 2.6.6.7 1 1.6 1 2.6 0 3.7-2.4 4.5-4.6 4.7.4.3.7.9.7 1.8v2.8c0 .4.2.6.7.5A9.5 9.5 0 0 0 12 2.7Z" />
    </svg>
    <span data-shell>GitHub</span>
  </a>
  <a data-shell class="project-link" href="https://ko-fi.com/prowly" target="_blank" rel="noopener noreferrer">
    <svg data-shell viewBox="0 0 24 24" aria-hidden="true">
      <path data-shell d="M4 7 H17 V16 H6 L4 14 Z M17 9 H19 L21 11 V13 L19 15 H17 M7 10 L10 13 L14 9" />
    </svg>
    <span data-shell>Buy me a coffee</span>
  </a>
{/snippet}

{#snippet statusStrip(inShell: boolean)}
  <!-- Shell-level status strip: one 40px spine on the landing AND the
       workspace. In the shell its brand cell sits exactly over the sidebar
       column; the rest answers "is what I'm looking at still true?" -
       inventory age, market age, orders to fix, Baro, WFM session. Rare
       inventory actions (Export / Restore / Clear) live one click deeper in
       the Refresh menu. -->
  <header data-shell class="statusbar" class:shell-strip={inShell} use:headerClearance={inShell}>
    <div data-shell class="brand">
      <h1 data-shell>TennoWorth</h1>
      {#if !inShell}<span data-shell class="sub">warframe.market prices, ranked by what actually sells</span>{/if}
    </div>
    
      {#if !inShell}<button data-shell class="btn" onclick={() => { inventory.phase = 'done'; filters.setView('notifications'); }}>Notifications{unreadNotifications ? ` (${unreadNotifications})` : ''}</button>{/if}
      <div data-shell class="cell inv" title={unresolvedCount > 0 ? `${unresolvedCount} items couldn't be price-matched (${unresolvedSummary}) - usually untradeable blueprints, quest items and very new content.` : undefined}>
        {#if inventory.inventoryName}
          <span data-shell class="dot {inventoryFreshness}" role="img" aria-label="Inventory {inventoryFreshness}"></span>
          <span data-shell>Inventory</span>
          <b data-shell class="file" title={inventory.inventoryName}>{inventory.inventoryName}</b>
          {#if inventoryStaleness}<span data-shell>·</span><b data-shell>{inventoryStaleness}</b>{/if}
        {:else}
          <span data-shell class="dot" aria-hidden="true"></span>
          <span data-shell>No inventory yet</span>
        {/if}
        <div data-shell class="refresh-wrap">
          <!-- Reflects the scan itself, not just the menu: refreshFromGame
               closes the popover before awaiting, so the "Scanning game…"
               label inside it vanished the moment it mattered and a ~10s scan
               looked like a dead click. This trigger stays on screen. -->
          <button data-shell
            class="refresh-trigger"
            class:busy={inventory.pullingInventory}
            onclick={() => (refreshOpen = !refreshOpen)}
            aria-expanded={refreshOpen}
            aria-busy={inventory.pullingInventory}
            disabled={inventory.pullingInventory}
            title={inventory.pullingInventory
              ? 'Reading the running game’s memory - this can take a few seconds.'
              : 'Load fresh inventory - re-fetch from the game. Export / Restore / Clear live in this menu too.'}
          >{inventory.pullingInventory ? 'Scanning…' : 'Refresh ▾'}</button>
          {#if refreshOpen}
            <div data-shell class="refresh-pop">
              <p data-shell class="rp-lede">Scan the running game - no file needed.</p>
              <button data-shell class="rp-primary" data-testid="desktop-scan" onclick={refreshFromGame} disabled={inventory.pullingInventory}>
                {inventory.pullingInventory ? 'Scanning game…' : 'Scan game'}
              </button>
              <div data-shell class="rp-sep" aria-hidden="true"></div>
              {#if inventory.inventoryName}
                <button data-shell class="rp-item" onclick={() => { refreshOpen = false; exportImportRef?.openExport(); }} title="Download an encrypted snapshot for another device or backup.">Export…</button>
              {/if}
              <button data-shell class="rp-item" onclick={() => { refreshOpen = false; exportImportRef?.pickImport(); }} title="Restore an encrypted snapshot exported from another device.">Restore…</button>
              {#if inventory.inventoryName}
                <button data-shell class="rp-item danger" onclick={() => { refreshOpen = false; handleClear(); }} title="Forget the saved inventory entirely.">Clear</button>
              {/if}
              {#if unresolvedCount > 0}
                <p data-shell class="rp-note" title="Breakdown: {unresolvedSummary}.">{unresolvedCount} items couldn't be price-matched (not shown) - usually untradeable blueprints, quest items and very new content; your sellable items aren't affected.</p>
              {/if}
            </div>
          {/if}
        </div>
      </div>
    
    <div data-shell class="cell">
      <span data-shell class="dot {marketFreshness}" role="img" aria-label="Market data {marketFreshness}"></span>
      <span data-shell>Market</span>
      <b data-shell>{marketStaleness ?? '-'}</b>
      {#if marketFreshness !== 'unknown'}<span data-shell>· {marketFreshness}</span>{/if}
    </div>
    {#if inShell && isDesktop && ordersToFix > 0}
      <div data-shell class="cell attn">
        <b data-shell>{ordersToFix}</b>
        <span data-shell>{ordersToFix === 1 ? 'order' : 'orders'} to fix</span>
        <button data-shell type="button" class="link" onclick={() => filters.setView('orders')} aria-label="Open My orders">→</button>
      </div>
    {/if}
    {#if baroState && baroState.phase !== 'unknown'}
      <div data-shell class="cell baro">
        <span data-shell class="ducat" aria-hidden="true">⌬</span>
        <span data-shell>{baroState.phase === 'here' ? 'Baro leaves in' : 'Baro arrives in'}</span>
        <b data-shell>{humanWindow(baroState.windowMs)}</b>
      </div>
    {/if}
    <span data-shell class="grow"></span>
    {#if !inShell}
      <nav data-shell class="cell end site-links" aria-label="Site">
        <a data-shell href="#faq">FAQ</a>
        
        {@render projectLinkAnchors()}
      </nav>
    {:else}
      <div data-shell class="cell end">
        <span data-shell>WFM</span>
        {#if listing.wfmStatus && !listing.wfmStatus.unlocked}
          <button data-shell type="button" class="link" onclick={() => wfmAuthDialogsRef?.open(listing.wfmStatus?.logged_in ? 'needs_unlock' : 'needs_login')}>{wfmLabel}</button>
        {:else}
          <b data-shell>{wfmLabel}</b>
        {/if}
        {#if listing.ordersSummary}<span data-shell>· {listing.ordersSummary.live} live</span>{/if}
      </div>
    {/if}
  </header>
{/snippet}





{#snippet generalBanners()}
  <!-- Cross-view banner region: pull-error and the desktop update banner, each
       independently dismissible. Rendered on both the landing and the workspace
       so a failure is visible wherever the user is standing. -->
  {#if inventory.pullError}
    <div data-shell class="card ui-panel warn-banner general-banner" role="alert">
      <div data-shell class="gb-body gb-pre">{inventory.pullError}</div>
      <div data-shell class="gb-actions">
        
          <button data-shell class="gb-report" onclick={() => inventory.reportScanBroke()} disabled={inventory.reportingScan}>
            {inventory.reportingScan ? 'Opening…' : 'Report this'}
          </button>
        
        <button data-shell class="gb-dismiss" aria-label="Dismiss" onclick={() => (inventory.pullError = null)}>×</button>
      </div>
    </div>
    {#if inventory.reportUrl}
      <!-- Shown only when the browser did not open: the report must still be
           filable by hand rather than dead-ending on a failed launch. -->
      <div data-shell class="card ui-panel warn-banner general-banner" role="status">
        <div data-shell class="gb-body">
          Couldn't open a browser. Copy this link to file the report:
          <div data-shell class="gb-pre report-url">{inventory.reportUrl}</div>
        </div>
        <div data-shell class="gb-actions">
          <button data-shell class="gb-dismiss" aria-label="Dismiss" onclick={() => (inventory.reportUrl = null)}>×</button>
        </div>
      </div>
    {/if}
  {/if}
  {#if isDesktop && trayHint}
    <div data-shell class="card ui-panel warn-banner general-banner" role="status">
      <div data-shell class="gb-body">
        Still running in your tray. Closing the window keeps TennoWorth in the
        background - use the tray icon's Quit to exit.
      </div>
      <div data-shell class="gb-actions">
        <button data-shell class="gb-dismiss" aria-label="Dismiss" onclick={() => (trayHint = false)}>×</button>
      </div>
    </div>
  {/if}
  
    <DesktopUpdateBanner />
  
{/snippet}

{#snippet pendingBanner()}
  {#if isDesktop && (listing.pendingPlan || listing.resumePhase !== 'idle')}
    <section data-shell class="card ui-panel pending-banner">
      {#if listing.resumePhase === 'running'}
        <div data-shell class="row">
          <div data-shell class="src">
            <span data-shell class="dot aging" aria-hidden="true"></span>
            <strong data-shell>Resuming interrupted batch…</strong>
            <span data-shell class="muted">~{Math.ceil(pendingRemaining * 0.35 + 1)}s</span>
          </div>
        </div>
      {:else if listing.resumePhase === 'done'}
        <div data-shell class="row">
          <div data-shell class="src">
            <span data-shell class="dot fresh" aria-hidden="true"></span>
            <strong data-shell>Resumed.</strong>
            <span data-shell class="muted">
              <span data-shell class="ok-text">{resumeOk} created</span>
              {#if resumeErr > 0}· <span data-shell class="bad">{resumeErr} failed</span>{/if}.
              New listings are still hidden - toggle from the orders panel.
            </span>
          </div>
          <div data-shell class="row gap-sm">
            <button data-shell class="ghost" onclick={() => { listing.resumePhase = 'idle'; listing.resumeResults = []; }}>Dismiss</button>
          </div>
        </div>
      {:else if listing.resumePhase === 'error'}
        <div data-shell class="row">
          <div data-shell class="src">
            <span data-shell class="dot stale" aria-hidden="true"></span>
            <strong data-shell>Resume failed.</strong>
            <span data-shell class="muted bad">{listing.resumeError}</span>
          </div>
          <div data-shell class="row gap-sm">
            <button data-shell onclick={() => listing.doResume()}>Retry</button>
            <button data-shell class="ghost" onclick={() => listing.doDiscard()}>Discard pending</button>
          </div>
        </div>
      {:else if listing.pendingPlan && pendingRemaining > 0}
        <div data-shell class="row">
          <div data-shell class="src">
            <span data-shell class="dot aging" aria-hidden="true"></span>
            <strong data-shell>Interrupted batch from {new Date(listing.pendingPlan.started_at).toLocaleString()}</strong>
            <span data-shell class="muted">
              · {pendingRemaining} pending{pendingDone > 0 ? `, ${pendingDone} already done` : ''}
            </span>
          </div>
          <div data-shell class="row gap-sm">
            <button data-shell onclick={() => listing.doResume()}>Resume</button>
            <button data-shell class="ghost" onclick={() => listing.doDiscard()}>Discard</button>
          </div>
        </div>
      {/if}
    </section>
  {/if}
{/snippet}

<!-- Desktop only (listing needs wfm-core's session). onauthrequired fires on
     typed needs_login/needs_unlock rejections. -->
<ListingReviewModal
  bind:open={listing.listingOpen}
  rows={listing.reviewRowsOverride ?? listableRows.slice(0, 50)}
  {transport}
  onauthrequired={(code) => wfmAuthDialogsRef?.open(code, 'list')}
  onclose={() => (listing.reviewRowsOverride = null)}
/>


  <WfmAuthDialogs bind:this={wfmAuthDialogsRef} onunlocked={(next) => listing.handleWfmUnlocked(next)} />


<ExportImportDialogs
  bind:this={exportImportRef}
  owned={inventory.resolved.owned}
  inventoryName={inventory.inventoryName}
  lastUpdated={inventory.lastUpdated}
  onimport={(result) => inventory.handleImported(result)}
/>


