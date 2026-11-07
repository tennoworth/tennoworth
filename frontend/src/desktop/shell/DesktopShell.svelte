<script lang="ts">
  import { t } from '../../i18n/i18n.svelte';
  import { richParts } from '../../i18n/message';
import KeyboardNavigation from '../../ui/KeyboardNavigation.svelte';
import KeyLegend from './KeyLegend.svelte';
import NavIcon from './NavIcon.svelte';
import { SidebarLayout, SIDEBAR_MAX } from './sidebar.svelte';
import { createDesktopSession } from './desktop-session.svelte';
import { createSellWorkspace } from '../selling/sell-workspace.svelte';
import StatusStrip from './StatusStrip.svelte';
import FeedbackDialog from '../../features/feedback/FeedbackDialog.svelte';
import PendingBatchBanner from '../selling/PendingBatchBanner.svelte';
import BaroView from '../baro/BaroView.svelte';
import MarketAnnouncement from '../../features/announcements/MarketAnnouncement.svelte';
import CalendarView from '../../features/calendar/CalendarView.svelte';
import RelicPlannerView from '../relics/RelicPlannerView.svelte';
import { CrackAnyway } from '../relics/crack-anyway.svelte';
import SetPicksView from '../selling/SetPicksView.svelte';
import { ActivityLog } from './activity.svelte';
import { ListingReceipts } from '../selling/receipts.svelte';
import type { PlanResponse, TradeAllowance } from '../../contracts/data';

  import { loadMarket } from '../../adapters/market';
  import { loadCatalogs } from '../../adapters/catalogs';
  import { TauriTransport, parseScanPayload } from '../../adapters/desktop';
  import { useDesktopServices } from './desktop-context';
  const { desktopAccessStatus, desktopWfmStatus, desktopWfmLogout, listenForTauriEvent, desktopProtectionState, desktopSaveProtectionPlan, normalizeInventoryNative } = useDesktopServices();
  import { ProtectionController } from '../selling/protection.svelte';
  import ProtectedPlan from '../selling/ProtectedPlan.svelte';
  import { type WfmSession } from '../../features/feedback/feedback';
  import { humanError, DesktopCmdError } from '../../contracts/errors';
  import { humanWindow } from '../../ui/format';
  
  import { onMount, tick, untrack } from 'svelte';
  import Faq from '../../features/faq/Faq.svelte';
  import { FilterController, type View } from '../selling/filters.svelte';
  import { ListingController, WfmAccessController } from '../selling/controller.svelte';
  import { InventoryController } from '../inventory/controller.svelte';
  import { AutoScanController } from '../inventory/auto-scan.svelte';
  import { AppIconController } from '../settings/app-icon.svelte';
  import { PresenceController } from '../presence/presence.svelte';
  import ListingReviewModal from '../selling/ListingReviewModal.svelte';
  import MyOrdersPanel from '../orders/MyOrdersPanel.svelte';
  import WatchlistPanel from '../watches/WatchlistPanel.svelte';
  import NotificationInbox from '../notifications/NotificationInbox.svelte';
  
import { ALLOWANCE_CHANGED_EVENT, ANNOUNCEMENTS_CHANGED_EVENT } from '../../contracts/events';
  import LedgerPanel from '../ledger/LedgerPanel.svelte';
  import MarketBrowser from '../../features/market/MarketBrowser.svelte';
  import DesktopUpdateBanner from '../updates/DesktopUpdateBanner.svelte';
  import WatchOfferBanner from '../watches/WatchOfferBanner.svelte';
  import { UpdateController } from '../updates/update-controller.svelte';
  import WfmAuthDialogs from '../account/WfmAuthDialogs.svelte';
  import ExportImportDialogs from '../inventory/ExportImportDialogs.svelte';
  import SellPane from '../selling/SellPane.svelte';
  import TradeSessionPane from '../selling/TradeSessionPane.svelte';
  import RivensPanel from '../rivens/RivensPanel.svelte';
  import ThemeSwitcher from '../../ui/ThemeSwitcher.svelte';
  import UpdateNotes from '../updates/UpdateNotes.svelte';
  import SettingsPanel from '../settings/SettingsPanel.svelte';
  import PriceSharingPrompt from '../settings/PriceSharingPrompt.svelte';
  import SupportPrompt from '../community/SupportPrompt.svelte';
  import { PromptSession } from '../prompts/prompt-session.svelte';
  import { PROMPT_ORDER } from '../prompts/policies';
  import RoutinesPanel from '../routines/RoutinesPanel.svelte';
  import { RoutineController } from '../routines/controller.svelte';
  import { resolveRivens } from '../../domain/rivens';

  import { buildMetaDrift } from '../../domain/meta-drift';

  import { interruptedBatch } from '../../domain/listing-plan';

  const APP_COMMIT = __APP_COMMIT__;
  import type { StateStore } from '../../contracts/state-store';
  import type { ThemeController } from '../../ui/theme';
  import { DisplayController, provideDisplay } from '../../ui/display.svelte';

  import MetaDriftPanel from '../../features/market/MetaDriftPanel.svelte';

  // Desktop (Tauri) vs hosted informational (browser) is decided ONCE at boot.
  // The hosted site is informational only: market data + the desktop showcase,
  // no files. Everything interactive - scan, list, orders, login - lives in
  // the desktop app, driven by the wfm_session commands.
  let updateNotesRef: UpdateNotes;
  // Set only by the inbox's settings link; every other way in opens the top.
  let settingsSection = $state<'notifications' | 'price-sharing' | 'account' | 'scan' | 'overlay' | 'app' | null>(null);
  // Settings people look for by name ("dark mode", "popups") rather than by view.
  const openSettings = (section: NonNullable<typeof settingsSection>) => { settingsSection = section; filters.setView('settings'); };
  const settingsDestinations = [
    { label: 'Settings › Appearance and updates', keywords: 'dark light theme colour color mode updates privacy app icon item pictures visual compact', run: () => openSettings('app') },
    { label: 'Settings › Notifications', keywords: 'popups alerts sounds in game price watches inbox', run: () => openSettings('notifications') },
    { label: 'Settings › Automatic scan', keywords: 'auto scan cadence rescan', run: () => openSettings('scan') },
    { label: 'Settings › Account', keywords: 'warframe.market sign in log out status presence online invisible', run: () => openSettings('account') },
    { label: 'Settings › Relic overlay', keywords: 'reward screen ocr', run: () => openSettings('overlay') },
  ];

  const notesServices = useDesktopServices();
  const transport = new TauriTransport();
  const marketAccess = new WfmAccessController({ desktopAccessStatus, listenForTauriEvent });
  onMount(() => marketAccess.start());
  const updates = new UpdateController(notesServices);
  onMount(() => updates.start());

  // Persistence seam: localStorage in the browser, SQLite-over-IPC in desktop.
  // Selected + primed (scalar settings loaded into cache) in main.ts and passed
  // in, so the scalar-setting `$state` initializers below can read it
  // synchronously with no first-paint flash. Snapshot methods are async.
  // `theme` is the boot-time ThemeController (src/lib/theme.ts) that the mode
  // control in Settings → Appearance (and its footer twin) drives.
  let { store, theme }: { store: StateStore; theme: ThemeController } = $props();
  const filters = untrack(() => new FilterController(store));
  const prompts = untrack(() => new PromptSession(store, PROMPT_ORDER));
  const routines = untrack(() => new RoutineController(store));
  const sidebar = untrack(() => new SidebarLayout(store));
  let shellElement = $state<HTMLElement>();
  function startResize(event: PointerEvent) {
    if (event.button !== 0 || !shellElement) return;
    event.preventDefault();
    const handle = event.currentTarget as HTMLElement;
    handle.setPointerCapture(event.pointerId);
    const left = shellElement.getBoundingClientRect().left;
    const move = (e: PointerEvent) => sidebar.dragTo(e.clientX - left);
    const end = () => { handle.removeEventListener('pointermove', move); sidebar.save(); };
    handle.addEventListener('pointermove', move);
    handle.addEventListener('lostpointercapture', end, { once: true });
  }
  // On the rail the labels are hidden, so the entry under the pointer or
  // keyboard names itself beside the rail. One tip, fixed to the window, so the
  // scrolling view list cannot clip it.
  let tip = $state<{ text: string; top: number; left: number } | null>(null);
  function showTip(event: Event) {
    if (!sidebar.collapsed || matchMedia('(max-width: 900px)').matches) { tip = null; return; }
    const target = (event.target as Element | null)?.closest<HTMLElement>('.nav-item, .project-link, .feedback-trigger, .rail-toggle');
    if (!target) { tip = null; return; }
    const label = target.querySelector('.label')?.textContent?.trim() ?? target.getAttribute('aria-label') ?? target.textContent?.trim() ?? '';
    const badge = target.querySelector('.badge')?.textContent?.trim();
    const box = target.getBoundingClientRect();
    tip = { text: badge && badge !== '—' ? `${label} · ${badge}` : label, top: box.top + box.height / 2, left: box.right + 6 };
  }
  const hideTip = () => { tip = null; };
  const inventory = untrack(() => new InventoryController(store, transport, { loadMarket, loadCatalogs, normalizeInventory: normalizeInventoryNative }));
  const protection = new ProtectionController({ desktopProtectionState, desktopSaveProtectionPlan });
  const listing = new ListingController({ getPendingPlan: () => transport.getPendingPlan(), resumePendingPlan: () => transport.resumePendingPlan(), discardPendingPlan: () => transport.discardPendingPlan(), status: desktopWfmStatus, logout: desktopWfmLogout }, (code, next) => wfmAuthDialogsRef?.open(code, next));
  const activity = new ActivityLog();
  const receipts = new ListingReceipts();
  const workspace = createSellWorkspace({ inventory, filters, protection, listing, transport, services: notesServices, getView: () => effectiveView, getNow: () => session.displayNow });
  let allocationMatches = $derived(workspace.allocationMatches);
  let unknownSlugs = $derived(workspace.unknownSlugs);
  let guidanceUnavailable = $derived(workspace.guidanceUnavailable);
  let listingBlockReason = $derived(workspace.listingBlockReason);
  let listingQuantitiesKnown = $derived(workspace.listingQuantitiesKnown);
  let estimatedGuidance = $derived(workspace.estimatedGuidance);
  let listingActionLabel = $derived(workspace.listingActionLabel);
  let availability = $derived(workspace.availability);
  let guidanceAvailability = $derived(workspace.guidanceAvailability);
  let guidanceOwned = $derived(workspace.guidanceOwned);
  let results = $derived(workspace.results);
  let visibleColumns = $derived(workspace.visibleColumns);
  let presetSort = $derived(workspace.presetSort);
  let ownedQtyForOrders = $derived(workspace.ownedQtyForOrders);
  let filterState = $derived(workspace.filterState);
  let adviceMap = $derived(workspace.adviceMap);
  let calculationError = $derived(workspace.calculationError);
  let calculationPending = $derived(workspace.calculationPending);
  let calculationsReady = $derived(workspace.calculationsReady);
  let listableRows = $derived(workspace.listableRows);
  let setRecos = $derived(workspace.setRecos);
  let availableTags = $derived(workspace.availableTags);
  let availableTypes = $derived(workspace.availableTypes);
  let totalPotential = $derived(workspace.totalPotential);
  let prevSummary = $derived(workspace.prevSummary);
  let sinceScan = $derived(workspace.sinceScan);
  let emptyReason = $derived(workspace.emptyReason);
  let advisorResult = $derived(workspace.advisorResult);
  let defaultFacts = $derived(workspace.defaultFacts);
  let setResult = $derived(workspace.setResult);
  let relicResult = $derived(workspace.relicResult);
  let voidTrader = $derived(workspace.voidTrader);
  let ducatStats = $derived(workspace.ducatStats);
  let showBaroCard = $derived(workspace.showBaroCard);
  let baroState = $derived(workspace.baroState);
  // Automatic scanning. The hold is driven by the two places a new snapshot
  // would invalidate work in progress: an open review (it carries price and
  // quantity edits) and the Trade Session view (a batch is tied to the snapshot
  // it was prepared from).
  const autoScan = new AutoScanController({
    settings: transport,
    listen: listenForTauriEvent,
    parse: parseScanPayload,
    adopt: (data, snapshotId) => inventory.adoptScan(data, snapshotId),
    isInteractive: () => listing.listingOpen || effectiveView === 'session',
  });
  // The user's warframe.market status; the native loop pushes every change.
  const presence = new PresenceController({ native: transport, listen: listenForTauriEvent });
  onMount(() => presence.start());
  // The window and tray icon colour; follows the resolved colour mode for 'match'.
  const appIcon = untrack(() => new AppIconController({ store, theme, native: transport }));
  onMount(() => appIcon.start());
  const display = untrack(() => new DisplayController(store));
  const crackAnyway = untrack(() => new CrackAnyway(store));
  provideDisplay(display);
  onMount(() => {
    const stopProtection = protection.start({
      listen: (event, cb) => listenForTauriEvent(event, cb),
      document,
      setInterval: (fn, ms) => window.setInterval(fn, ms),
      clearInterval: id => window.clearInterval(id),
      now: () => Date.now(),
      reviewing: () => listing.listingOpen || effectiveView === 'session',
    });
    const stopAutoScan = autoScan.start();
    void autoScan.load();
    return () => { stopProtection(); stopAutoScan(); protection.destroy(); };
  });
  $effect(() => {
    const owned = inventory.resolved.owned;
    const snapshotId = inventory.nativeSnapshotId;
    untrack(() => void protection.setInventory(owned, snapshotId));
  });
  // Today's trades left and which items trade in bulk, for the Top picks trade
  // split. A failed read leaves both unknown, which shows no split at all.
  let tradeAllowance = $state<TradeAllowance | null>(null);
  // null = unknown: the list needs an unlocked warframe.market session.
  let bulkSlugs = $state<ReadonlySet<string> | null>(null);
  let allowanceRequest = 0;
  async function refreshAllowance() {
    const current = ++allowanceRequest;
    try {
      const state = await notesServices.desktopTradeSessionState();
      if (current !== allowanceRequest) return;
      tradeAllowance = state?.allowance ?? null;
      bulkSlugs = state?.supported_slugs == null ? null : new Set(state.bulk_slugs);
    } catch {
      if (current === allowanceRequest) { tradeAllowance = null; bulkSlugs = null; }
    }
  }
  onMount(() => listenForTauriEvent(ALLOWANCE_CHANGED_EVENT, () => void refreshAllowance()));
  $effect(() => {
    // A new scan carries a new remaining count; unlocking or locking
    // warframe.market makes bulk status known or unknown.
    void inventory.nativeSnapshotId;
    void listing.wfmStatus?.unlocked;
    untrack(() => void refreshAllowance());
  });
  $effect(() => {
    // The Rust loop keeps the state; this only mirrors whether a listing flow
    // is open, and only when the answer changes.
    const interactive = listing.listingOpen || effectiveView === 'session';
    untrack(() => void autoScan.setInteractive(interactive));
  });
  $effect(() => {
    // A scan the app already has on screen makes an offer redundant - adopting
    // it later would swap newer rows back to older ones.
    const shown = inventory.nativeSnapshotId;
    untrack(() => {
      const offered = autoScan.pending?.snapshotId ?? null;
      if (offered != null && shown != null && offered <= shown) autoScan.dismiss();
    });
  });

  let resolvedRivens = $derived(resolveRivens(inventory.ownedRivens, inventory.market));
  
  // Sidebar nav: if the user's persisted view is unavailable (Baro not
  // visiting, orders on the informational site), fall back to Sell rather than
  // rendering an empty pane. The nav itself hides those entries; this protects
  // against a stale localStorage value.
  let effectiveView = $derived.by<View>(() => {
    if (filters.view === 'baro' && !showBaroCard) return 'sell';
    if (filters.view === 'meta' && !buildMetaDrift(inventory.market)) return 'sell';
    return filters.view;
  });

  // Views visited this session; each stays mounted (hidden) once opened.
  let visited = $state(new Set<View>());
  $effect(() => {
    const view = effectiveView;
    untrack(() => { if (!visited.has(view)) visited = new Set([...visited, view]); });
  });

  const session = createDesktopSession({ inventory, listing, transport, store: untrack(() => store), services: notesServices });

  let relicShowAll = $state(false);
  let relicPlan = $derived(relicResult.value);
  // Total + per-category breakdown of paths no catalog could price-match.
  // The number itself is reassurance ("the app saw these and skipped them,
  // your prime junk isn't missing"), the breakdown is hover detail.

  let unresolvedCount = $derived(
    Object.values(inventory.resolved.unresolved).reduce((s, n) => s + n, 0)
  );

  // Every owned row with ANY market match - no preset/filter applied. The
  // sidebar Sell badge pins to this so it stays stable while filters change.
  let sellableCount = $derived([...defaultFacts.value.values()].filter(row => row.sellable > 0 && (guidanceAvailability.get(row.key) ?? 0) > 0).length);
  let unresolvedSummary = $derived(
    Object.entries(inventory.resolved.unresolved)
      .map(([k, v]) => `${k}: ${v}`)
      .join(', ')
  );

  let exportImportRef = $state<{ openExport(): void; pickImport(): void }>();

  let snapshotStamp = $derived(session.snapshotStamp);
  let marketStaleness = $derived(session.marketStaleness);
  let inventoryStaleness = $derived(session.inventoryStaleness);
  let inventoryTimestamp = $derived(session.inventoryTimestamp);
  let inventoryFreshness = $derived(session.inventoryFreshness);
  let baroSurfaceAge = $derived(session.baroSurfaceAge);
  let relicSurfaceAge = $derived(session.relicSurfaceAge);
  let setSurfaceAge = $derived(session.setSurfaceAge);
  let marketFreshness = $derived(session.marketFreshness);
  let wfmLabel = $derived(session.wfmLabel);
  let notesReady = $derived(session.notesReady);
  let desktopPlatform = $derived(session.desktopPlatform);
  let desktopAppVersion = $derived(session.desktopAppVersion);
  let trayHint = $derived(session.trayHint);
  let unreadNotifications = $derived(session.unreadNotifications);
  let displayNow = $derived(session.displayNow);
  // Bug reports ask for the version, so show it where people look for it.
  let versionLabel = $derived(desktopAppVersion ? `v${desktopAppVersion} · ${APP_COMMIT}` : APP_COMMIT);

  // One classification for the interrupted batch: an item that was never sent
  // and an item whose send outcome is unknown are different work, and the
  // banner has to offer different things for each.
  let batch = $derived(interruptedBatch(listing.pendingPlan));
  let pendingRemaining = $derived(batch?.pending ?? 0);
  let uncertainRemaining = $derived(batch?.uncertain ?? 0);
  let outstanding = $derived(pendingRemaining + uncertainRemaining);
  let ordersToFix = $derived((listing.ordersSummary?.issues ?? 0) + (listing.pendingPlan ? outstanding : 0));

  // Scans report in the strip's activity cell: a user scan from the moment it
  // starts, an automatic one when it lands. Both settle on what changed.
  let scanActivity: number | null = null;
  let lastApplied: number | null = null;
  function scanSummary(): string {
    if (!inventory.previousOwned) return 'Scan applied';
    let added = 0, changed = 0;
    for (const [key, d] of inventory.deltas) {
      if (!inventory.previousOwned.has(key)) added += 1;
      else if (d !== 0) changed += 1;
    }
    return added + changed === 0 ? 'Scan applied · no changes' : `Scan applied · ${changed} changed · ${added} new`;
  }
  $effect(() => {
    const pulling = inventory.pullingInventory;
    untrack(() => {
      if (pulling && scanActivity == null) scanActivity = activity.begin('Scanning game…');
      else if (!pulling && scanActivity != null) {
        const id = scanActivity;
        scanActivity = null;
        if (inventory.pullError) activity.finish(id, inventory.noTradeables ? 'Scan found nothing tradeable' : 'Scan failed', 'bad');
        else if (inventory.lastUpdated !== lastApplied) { lastApplied = inventory.lastUpdated; activity.finish(id, scanSummary()); }
        else activity.finish(id, 'Scan stopped');
      }
    });
  });
  $effect(() => {
    // A scan landing outside a user request: the automatic scanner adopting one.
    const updated = inventory.lastUpdated;
    const fromScan = inventory.source === 'scan';
    untrack(() => {
      if (!fromScan || updated == null || updated === lastApplied || inventory.pullingInventory) return;
      lastApplied = updated;
      activity.finish(activity.begin(''), scanSummary());
    });
  });
  // Make visible from the row that was listed, without reopening the review.
  async function makeReceiptVisible(receipt: { orderId: string | null; slug: string }): Promise<void> {
    if (!receipt.orderId) return;
    try {
      const { results } = await activity.track('Making visible…', () => transport.bulkVisibility([receipt.orderId!], true), (r) => r.results.some((x) => x.status === 'ok') ? 'Now visible to buyers' : 'Visibility not changed', () => 'Visibility not changed');
      receipts.markVisible(results);
    } catch (error) {
      if (error instanceof DesktopCmdError && (error.code === 'needs_login' || error.code === 'needs_unlock')) void wfmAuthDialogsRef?.open(error.code);
    }
  }
  function sendSummary(response: PlanResponse): string {
    const ok = response.results.filter((r) => r.status === 'ok');
    const created = ok.filter((r) => r.action !== 'updated').length;
    const updated = ok.length - created;
    const open = response.results.filter((r) => r.status === 'pending' || r.status === 'uncertain_mutation').length;
    if (open > 0) return `Batch interrupted · ${open} to resume`;
    const parts = [created ? `Listed ${created} · hidden` : '', updated ? `Updated ${updated}` : ''].filter(Boolean);
    return parts.join(' · ') || 'Nothing was listed';
  }

  let wfmAuthDialogsRef = $state<{ open(code: string, next?: string | null): Promise<void> }>();
  // The last sign-in or unlock failure, kept for a bug report after the dialog closes.
  let wfmAuthFailure = $state<unknown>(null);
  async function checkListingRequirements() {
    if (inventory.pullingInventory || protection.loading) return;
    if (listingActionLabel === 'Scan game') {
      await inventory.pullInventory();
      await protection.refresh();
    } else if (listingActionLabel === 'Recheck protection') await protection.refresh();
    else await connectForListings();
  }
  async function connectForListings() {
    try {
      const status = await desktopWfmStatus();
      listing.wfmStatus = status;
      if (status.unlocked) await protection.refresh();
      else await wfmAuthDialogsRef?.open(status.logged_in ? 'needs_unlock' : 'needs_login');
    } catch (error) { protection.error = humanError(error); }
  }

  let feedbackRef: FeedbackDialog;
  let statusStripRef: StatusStrip;
  let feedbackFromMore = false;
  function openFeedback() { feedbackRef.openFeedback(); }
  function captureFeedbackState() {
    const wfmSession: WfmSession = !listing.wfmStatus ? 'unknown' : listing.wfmStatus.unlocked ? 'unlocked' : listing.wfmStatus.logged_in ? 'locked' : 'logged_out';
    const state = {
      capturedAt: new Date().toISOString(), build: APP_COMMIT, appVersion: desktopAppVersion, platform: desktopPlatform,
      view: showWorkspace ? effectiveView : 'landing', phase: inventory.phase,
      scanning: inventory.pullingInventory, scanError: inventory.error ?? inventory.pullError,
      autoScan: autoScan.status, wfmSession, wfmError: wfmAuthFailure,
      marketLoaded: !!inventory.market, marketError: inventory.marketLoadError,
      theme: document.documentElement.dataset.mode ?? 'unknown', width: window.innerWidth, height: window.innerHeight,
    };
    return state;
  }

  let hasInventory = $derived(inventory.resolved.owned.size > 0);
  // Prompts measure use from the first launch with an inventory, and stay out
  // of trading, where they would compete with the task.
  let promptsActive = $derived(hasInventory && !listing.listingOpen && effectiveView !== 'session');
  $effect(() => { if (hasInventory) untrack(() => prompts.launch()); });
  // Until startup has looked for a saved scan, "no inventory" is unknown, not
  // empty: show the workspace with placeholders, never the first-run page.
  let showWorkspace = $derived(hasInventory || inventory.phase === 'done' || effectiveView !== 'sell' || !inventory.restored);
  // Retaining a pane preserves its table scroll, but all panes share the page
  // scroller. Capture it before hiding the old pane and restore after layout.
  let workspaceElement = $state<HTMLElement>();
  const viewScroll = new Map<View, { top: number; left: number }>();
  let scrollingView: View | null = null;
  let scrollGeneration = 0;
  $effect.pre(() => {
    const view = effectiveView;
    const element = workspaceElement;
    const visible = showWorkspace;
    untrack(() => {
      if (!element || !visible) { scrollingView = null; ++scrollGeneration; return; }
      if (scrollingView === view) return;
      const inside = getComputedStyle(element).overflowY === 'auto';
      if (scrollingView) viewScroll.set(scrollingView, {
        top: inside ? element.scrollTop : window.scrollY,
        left: inside ? element.scrollLeft : window.scrollX,
      });
      scrollingView = view;
      const saved = viewScroll.get(view) ?? { top: 0, left: 0 };
      const generation = ++scrollGeneration;
      void tick().then(() => {
        if (generation !== scrollGeneration || !showWorkspace || effectiveView !== view) return;
        const scroller = getComputedStyle(element).overflowY === 'auto' ? element : window;
        scroller.scrollTo({ ...saved, behavior: 'instant' });
      });
    });
  });

  let restoring = $derived(!inventory.restored && !hasInventory);
</script>

<a class="skip-navigation" href="#workspace" onclick={(event) => { event.preventDefault(); workspaceElement?.focus(); }}>Skip to current view</a>

<FeedbackDialog bind:this={feedbackRef} captureState={captureFeedbackState} services={notesServices} onclosed={() => { if (feedbackFromMore) statusStripRef?.focusMore(); feedbackFromMore = false; }} />

<!-- Keep the banner region mounted while navigation or a scan changes the content. -->
<div data-shell data-testid="desktop-mode" class={showWorkspace ? 'shell' : 'desktop-landing'} bind:this={shellElement} data-sidebar={showWorkspace && sidebar.collapsed ? 'rail' : undefined} style:--sidebar={showWorkspace ? (sidebar.collapsed ? 'var(--sidebar-rail)' : `${sidebar.width}px`) : undefined}>
  <StatusStrip keyboardTools={firstRunKeyboard} bind:this={statusStripRef} inShell={showWorkspace} {inventory} {listing} {filters} {unresolvedCount} {unresolvedSummary} {inventoryFreshness} {inventoryStaleness} {inventoryTimestamp} {marketFreshness} {marketStaleness} {baroState} {unreadNotifications} {wfmLabel} {projectLinkAnchors} onexport={() => exportImportRef?.openExport()} onimport={() => exportImportRef?.pickImport()} onclear={() => workspace.clear()} onupdates={() => updates.check({ announce: true })} onfeedback={() => { feedbackFromMore = true; openFeedback(); }} onauth={(code) => wfmAuthDialogsRef?.open(code)} {presence} {activity} onpresencesettings={() => { settingsSection = 'account'; filters.setView('settings'); }} />
  {#if showWorkspace}
  <aside data-shell class="sidebar" onpointerover={showTip} onpointerleave={hideTip} onfocusin={showTip} onfocusout={hideTip}>
    <nav data-shell aria-label="App views">
      <div data-shell class="nav-group">
        <div data-shell class="nav-label">Trade</div>
        <button data-shell type="button" class="nav-item" data-testid="nav-sell" class:active={effectiveView === 'sell'} aria-current={effectiveView === 'sell' ? 'page' : undefined} onclick={() => filters.setView('sell')}>
          <NavIcon name="sell" />
          <span data-shell class="label">{hasInventory ? estimatedGuidance ? 'Opportunities' : 'Sell' : 'Inventory'}</span>
          <!-- Pinned to the unfiltered sellable count: with a narrow preset
               active (Vaulted on a no-vaulted inventory), a filter-driven
               "Sell 0" reads as "your inventory got wiped". -->
          {#if hasInventory}<span data-shell class="badge">{defaultFacts.phase === 'done' && !guidanceUnavailable ? sellableCount : '—'}</span>{/if}
        </button>
        
          <button data-shell type="button" class="nav-item" class:active={effectiveView === 'session'} aria-current={effectiveView === 'session' ? 'page' : undefined} onclick={() => filters.setView('session')}>
          <NavIcon name="session" /><span data-shell class="label">Trade Session</span></button>
        
          <button data-shell type="button" class="nav-item" class:active={effectiveView === 'sets'} aria-current={effectiveView === 'sets' ? 'page' : undefined} onclick={() => filters.setView('sets')}>
          <NavIcon name="sets" />
            <span data-shell class="label">Set picks</span>
            <span data-shell class="badge">{setResult.phase === 'done' ? setRecos.length : '—'}</span>
          </button>
        <!-- Hidden only once the plan is known to be empty, so the nav does not
             shift when relic values arrive after the inventory. -->
        {#if relicPlan.length > 0 || relicResult.phase !== 'done' || relicResult.error}
          <button data-shell type="button" class="nav-item" class:active={effectiveView === 'relics'} aria-current={effectiveView === 'relics' ? 'page' : undefined} onclick={() => filters.setView('relics')}>
          <NavIcon name="relics" />
            <span data-shell class="label">Relics</span>
            <span data-shell class="badge">{relicResult.phase === 'done' ? relicPlan.length : '—'}</span>
          </button>
        {/if}
        <!-- Shown from the scan itself, not once prices arrive, so the nav does
             not shift when the market snapshot lands after the inventory. -->
        {#if resolvedRivens.length > 0 || (inventory.ownedRivens.length > 0 && !inventory.market)}
          <button data-shell type="button" class="nav-item" class:active={effectiveView === 'rivens'} aria-current={effectiveView === 'rivens' ? 'page' : undefined} onclick={() => filters.setView('rivens')}>
          <NavIcon name="rivens" />
            <span data-shell class="label">Rivens</span>
            <span data-shell class="badge">{resolvedRivens.length}</span>
          </button>
        {/if}
        {#if showBaroCard || (hasInventory && !inventory.market && !inventory.marketLoadError)}
          <button data-shell type="button" class="nav-item baro-nav" class:active={effectiveView === 'baro'} aria-current={effectiveView === 'baro' ? 'page' : undefined} onclick={() => filters.setView('baro')}>
          <NavIcon name="baro" />
            <span data-shell class="label">Baro</span>
            {#if baroState?.phase === 'here'}<span data-shell class="badge here">here</span>
            {:else if baroState && baroState.phase !== 'unknown'}<span data-shell class="badge when" title="Baro arrives in {humanWindow(baroState.windowMs)}">{humanWindow(baroState.windowMs).split(' ')[0]}</span>{/if}
          </button>
        {/if}
        <button data-shell type="button" class="nav-item" class:active={effectiveView === 'routines'} aria-current={effectiveView === 'routines' ? 'page' : undefined} onclick={() => filters.setView('routines')}>
          <NavIcon name="routines" />
          <span data-shell class="label">Routines</span>
        </button>
        <button data-shell type="button" class="nav-item" class:active={effectiveView === 'calendar'} aria-current={effectiveView === 'calendar' ? 'page' : undefined} onclick={() => filters.setView('calendar')}>
          <NavIcon name="calendar" />
          <span data-shell class="label">Calendar</span>
        </button>
        {#if buildMetaDrift(inventory.market) || (hasInventory && !inventory.market && !inventory.marketLoadError)}
          <button data-shell type="button" class="nav-item" class:active={effectiveView === 'meta'} aria-current={effectiveView === 'meta' ? 'page' : undefined} onclick={() => filters.setView('meta')}>
          <NavIcon name="meta" />
            <span data-shell class="label">Meta Drift</span>
          </button>
        {/if}
      </div>

      <div data-shell class="nav-group">
        <div data-shell class="nav-label">Manage</div>
        <button data-shell type="button" class="nav-item" class:active={effectiveView === 'orders'} aria-current={effectiveView === 'orders' ? 'page' : undefined} onclick={() => filters.setView('orders')}>
          <NavIcon name="orders" />
          <span data-shell class="label">My orders</span>
          {#if ordersToFix > 0}<span data-shell class="badge warn" title="{ordersToFix} {ordersToFix === 1 ? 'order' : 'orders'} to fix">{ordersToFix}</span>{/if}
        </button>
        <button data-shell type="button" class="nav-item" class:active={effectiveView === 'watches'} aria-current={effectiveView === 'watches' ? 'page' : undefined} onclick={() => filters.setView('watches')}>
          <NavIcon name="watches" />
          <span data-shell class="label">Price watches</span>
        </button>
        <button data-shell type="button" class="nav-item" class:active={effectiveView === 'notifications'} aria-current={effectiveView === 'notifications' ? 'page' : undefined} onclick={() => filters.setView('notifications')}>
          <NavIcon name="notifications" />
          <span data-shell class="label">Notifications</span>{#if unreadNotifications}<span data-shell class="badge unread">{unreadNotifications}</span>{/if}
        </button>
        <button data-shell type="button" class="nav-item" class:active={effectiveView === 'ledger'} aria-current={effectiveView === 'ledger' ? 'page' : undefined} onclick={() => filters.setView('ledger')}>
          <NavIcon name="ledger" />
          <span data-shell class="label">Ledger</span>
        </button>
      </div>

      <div data-shell class="nav-group">
        <div data-shell class="nav-label">Library</div>
        <button data-shell type="button" class="nav-item" class:active={effectiveView === 'install'} aria-current={effectiveView === 'install' ? 'page' : undefined} onclick={() => filters.setView('install')}>
          <NavIcon name="faq" />
          <span data-shell class="label">FAQ</span>
        </button>
        <button data-shell type="button" class="nav-item" class:active={effectiveView === 'settings'} aria-current={effectiveView === 'settings' ? 'page' : undefined} onclick={() => filters.setView('settings')}>
          <NavIcon name="settings" />
          <span data-shell class="label">Settings</span>
        </button>
      </div>
    </nav>
    <!-- After the views in tab order: it is absolutely placed on the edge, and
         Enter collapsing the sidebar was easy to hit on the way to the views. -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div data-shell class="sidebar-resize" role="separator" tabindex="0" aria-orientation="vertical" aria-label="Sidebar width" aria-valuemin={56} aria-valuemax={SIDEBAR_MAX} aria-valuenow={sidebar.collapsed ? 56 : sidebar.width} aria-valuetext={sidebar.collapsed ? 'Icons only' : `${sidebar.width} pixels`} title="Drag to resize · ← → resize · Enter or double-click collapses"
      onpointerdown={startResize} ondblclick={() => sidebar.toggle()} onkeydown={(e) => { if (sidebar.key(e.key)) e.preventDefault(); }}></div>
  </aside>

  {/if}
  <main id="workspace" tabindex="-1" aria-label="Current view" bind:this={workspaceElement} data-shell class={showWorkspace ? 'workspace' : 'landing'} class:reading-view={['install', 'settings'].includes(effectiveView)} data-testid={!showWorkspace ? 'desktop-first-run' : undefined}>
    {@render generalBanners()}
    {#if !showWorkspace}
  {#if !inventory.error && !inventory.pullError}
    <!-- Scanning is the first thing to do here, so it leads; settings, updates
         and feedback live in the header. -->
    <section data-shell class="upsell-lead desktop-hero" aria-labelledby="desktop-hero-title">
      <h2 data-shell id="desktop-hero-title">{t('firstRun.heading')}</h2>
      <p data-shell class="sub">{#each richParts(t('firstRun.pitch')) as part, i (i)}{#if part.em}<em data-shell>{part.text}</em>{:else}{part.text}{/if}{/each}</p>
      <p data-shell class="req">{t('firstRun.requirement')}</p>
      <div data-shell class="desktop-scan-row">
        <button data-shell class="rp-primary" data-testid="desktop-scan" onclick={() => inventory.pullInventory()} disabled={inventory.pullingInventory}>{inventory.pullingInventory ? t('firstRun.scanning') : t('firstRun.scan')}</button>
        <span data-shell class="trust">{t('firstRun.trust')}</span>
      </div>
    </section>
  {/if}
  {@render marketAnnouncement()}
  {#if inventory.market}
    <p data-shell class="lookup-label">{t('firstRun.lookup')}</p>
    <MarketBrowser market={inventory.market} staleness={marketStaleness} freshness={marketFreshness} loadHistory={() => transport.loadHistory()} />
  {/if}
  <Faq />

  <footer data-shell class="sitefoot">
    <span data-shell class="grow">TennoWorth is a fan project, not affiliated with Digital Extremes or warframe.market. Open source · MIT · data from warframe.market and warframestat.us. Item images from the Warframe Wiki, © Digital Extremes.</span>
    {#if inventory.market?.updated_at}<span data-shell title="When the market snapshot was taken">Snapshot {snapshotStamp}</span>{/if}
    <a data-shell href="#trust">Trust &amp; safety</a>
    <span data-shell class="ver" title="build {APP_COMMIT}">{versionLabel}</span>
    <div data-shell class="foot-theme"><ThemeSwitcher {theme} compact label="Colour mode" /></div>
  </footer>
    {:else}
    {@render marketAnnouncement()}
    {#if advisorResult.error && !filterState.adviceOnly && ['sell', 'sets', 'session'].includes(effectiveView)}
      <div class="ui-notice" data-tone="warn" role="status">Hold/sell advice unavailable: {advisorResult.error} <button class="btn" onclick={() => workspace.calculationEpoch += 1}>Retry calculations</button></div>
    {/if}

    <!-- Each view stays mounted once visited and is hidden while another is
         on screen, so a search, a page, a scroll position or a half-written
         draft is still there on return. -->
    {#if effectiveView === 'sell' && restoring}
      <!-- Startup is still looking for the saved scan: hold the Sell layout
           with placeholder rows so the real ones fill in where they will stay. -->
      <section data-shell class="view-header">
        <h2 data-shell>Sell</h2>
        <p data-shell class="lede" role="status">Loading your saved scan…</p>
      </section>
      <div data-shell class="restoring" aria-hidden="true">
        {#each { length: 3 } as _, i (i)}<div data-shell class="ph-row pick"><span data-shell></span><span data-shell></span><span data-shell></span></div>{/each}
        <div data-shell class="ph-gap"></div>
        {#each { length: 8 } as _, i (i)}<div data-shell class="ph-row"><span data-shell></span><span data-shell></span><span data-shell></span></div>{/each}
      </div>
    {/if}
    {#if visited.has('sell') && !restoring}
    <div data-shell class="view-pane" hidden={!(effectiveView === 'sell' && !restoring)}>
      <SellPane keep={keepSection} {receipts} allSellable={defaultFacts.phase === 'done' && !guidanceUnavailable ? sellableCount : null} {tradeAllowance} {bulkSlugs} onmakevisible={makeReceiptVisible} onopenorders={() => filters.setView('orders')}
        bind:minPrice={filters.minPrice} bind:minOwned={filters.minOwned} bind:typeFilter={filters.typeFilter} bind:hideAtLvl={filters.hideAtLvl} bind:activeTags={filters.activeTags}
        bind:tableView={workspace.tableView}
        resolved={inventory.resolved} allocation={allocationMatches ? protection.state : null} {results} deltas={inventory.deltas} {totalPotential}
        prevSummary={estimatedGuidance ? null : prevSummary} {sinceScan} ordersSummary={estimatedGuidance ? null : listing.ordersSummary}
        {marketFreshness} {marketStaleness} marketLoadError={inventory.marketLoadError}
        {listableRows} {availableTags} {availableTypes}
        {visibleColumns} {presetSort} {emptyReason}
        columnsCustomized={filters.columnKey in filters.columnChoice} oncolumnschange={(columns) => filters.setColumns(columns)}
        activePreset={filters.activePreset} reserveCopies={filters.reserveCopies} filtersOpen={filters.filtersOpen}
        sellOnboardingDismissed={filters.sellOnboardingDismissed} keepCopiesNudgeDismissed={filters.keepCopiesNudgeDismissed}
        applyPreset={(name) => filters.applyPreset(name)} setReserveCopies={(value) => filters.setReserveCopies(value)} toggleFiltersOpen={(event) => filters.toggleFiltersOpen(event)}
        dismissSellOnboarding={() => filters.dismissSellOnboarding()} dismissKeepCopiesNudge={() => filters.dismissKeepCopiesNudge()}
        openListingFlow={(rows) => { if (calculationsReady && !estimatedGuidance) listing.openListingFlow((Array.isArray(rows) ? rows : rows ? [rows] : listableRows).map(row => ({ ...row, inventory_snapshot_id: inventory.nativeSnapshotId ?? undefined }))); }}
        {estimatedGuidance} oncheckListings={checkListingRequirements} canList={listingQuantitiesKnown} {listingActionLabel} unavailableCount={unknownSlugs.size}
        {pendingBanner}
        calculationPending={calculationPending || (hasInventory && !inventory.market && !inventory.marketLoadError)} {calculationError} calculationErrorShown={guidanceUnavailable} onretryCalculation={() => workspace.calculationEpoch += 1}
      />
    </div>
    {/if}
    {#if visited.has('session')}
    <div data-shell class="view-pane" hidden={!(effectiveView === 'session')}>
      {#if defaultFacts.phase === 'loading'}
        <div class="ui-notice" role="status">Calculating safe quantities and sale values…</div>
      {:else if defaultFacts.error}
        <div class="ui-notice" data-tone="bad" role="alert">Sale calculations unavailable: {defaultFacts.error} <button class="btn" onclick={() => workspace.calculationEpoch += 1}>Retry calculations</button></div>
      {/if}
      <TradeSessionPane active={effectiveView === 'session'} sessionEpoch={listing.sessionEpoch} keep={keepSection} {listingBlockReason} onrecheck={checkListingRequirements} {listingActionLabel} owned={inventory.resolved.owned} market={inventory.market} reserveCopies={filters.reserveCopies} advice={adviceMap} nativeFacts={defaultFacts.value} {availability}
        scanning={inventory.pullingInventory} onscan={async () => { await inventory.pullInventory(); await protection.refresh(); }} onreview={(rows, budget, state) => { if (!listingQuantitiesKnown) return; listing.openListingFlow(rows.map(r => ({
          ...r, inventory_snapshot_id: inventory.nativeSnapshotId ?? undefined, proposed_quantity: r.quantity, clearing_price: r.platinum, low_sell: r.platinum,
          avg_price: r.market.avg, session: { snapshot_id: state.allowance.snapshot_id!, utc_day: state.allowance.utc_day, budget },
        }))); }} />
    </div>
    {/if}
    {#if visited.has('sets')}
    <div data-shell class="view-pane" hidden={!(effectiveView === 'sets')}>
      <SetPicksView {inventory} {setSurfaceAge} {keepSection} {guidanceUnavailable} {setResult} {setRecos} {adviceMap} onretry={() => workspace.calculationEpoch += 1} />
    </div>
    {/if}
    {#if visited.has('relics')}
    <div data-shell class="view-pane" hidden={!(effectiveView === 'relics')}>
      <RelicPlannerView bind:relicShowAll {relicResult} {relicSurfaceAge} {relicPlan} {crackAnyway} onretry={() => workspace.calculationEpoch += 1} />
    </div>
    {/if}
    {#if visited.has('rivens')}
    <div data-shell class="view-pane" hidden={!(effectiveView === 'rivens')}>
      <RivensPanel market={inventory.market} rivens={resolvedRivens} />
    </div>
    {/if}
    {#if visited.has('baro')}
    <div data-shell class="view-pane" hidden={!(effectiveView === 'baro')}>
      <BaroView {inventory} {baroSurfaceAge} {keepSection} {guidanceUnavailable} {voidTrader} {ducatStats} {baroState} {guidanceOwned} {guidanceAvailability} {unknownSlugs} onducats={() => { filters.setView('sell'); filters.applyPreset('ducats'); }} />
    </div>
    {/if}
    {#if visited.has('routines')}
    <div data-shell class="view-pane" hidden={!(effectiveView === 'routines')}>
      <RoutinesPanel routine={routines} market={inventory.market} owned={inventory.resolved.owned} now={displayNow} />
    </div>
    {/if}
    {#if visited.has('calendar')}
    <div data-shell class="view-pane" hidden={!(effectiveView === 'calendar')}>
      <CalendarView market={inventory.market} owned={inventory.resolved.owned} now={displayNow} loadAnnouncements={notesServices.desktopAnnouncements} {store} surface="desktop" />
    </div>
    {/if}
    {#if visited.has('meta')}
    <div data-shell class="view-pane" hidden={!(effectiveView === 'meta')}>
      <MetaDriftPanel market={inventory.market} />
    </div>
    {/if}
    {#if visited.has('orders')}
    <div data-shell class="view-pane" hidden={!(effectiveView === 'orders')}>
      <MyOrdersPanel
        banner={pendingBanner}
        active={effectiveView === 'orders'}
        {activity}
        {transport}
        market={inventory.market}
        sessionEpoch={listing.sessionEpoch}
        ownedQty={ownedQtyForOrders}
        {marketStaleness}
        onauthrequired={(code) => wfmAuthDialogsRef?.open(code)}
        onsummary={(s) => (listing.ordersSummary = s)}
      />
    </div>
    {/if}
    {#if visited.has('watches')}
    <div data-shell class="view-pane" hidden={!(effectiveView === 'watches')}>
      <WatchlistPanel market={inventory.market} />
    </div>
    {/if}
    {#if visited.has('notifications')}
    <div data-shell class="view-pane" hidden={!(effectiveView === 'notifications')}>
      <!-- Calendar reminders carry the 'routines' target from before Calendar was its own view; their button says Open calendar. -->
      <NotificationInbox onopen={(target) => filters.setView(target === 'routines' ? 'calendar' : target)} onsettings={() => { settingsSection = 'notifications'; filters.setView('settings'); }} />
    </div>
    {/if}
    {#if visited.has('ledger')}
    <div data-shell class="view-pane" hidden={!(effectiveView === 'ledger')}>
      <LedgerPanel onsetautoclose={(on) => store.setSetting('auto-close-sold', on ? 'on' : 'off')} />
    </div>
    {/if}
    {#if visited.has('install')}
    <div data-shell class="view-pane" hidden={!(effectiveView === 'install')}>
      <section data-shell class="view-header">
        <h2 data-shell>FAQ</h2>
        <p data-shell class="lede">Answers to common questions.</p>
      </section>
      <Faq />
    </div>
    {/if}
    {#if visited.has('settings')}
    <div data-shell class="view-pane" hidden={!(effectiveView === 'settings')}>
      <SettingsPanel active={effectiveView === 'settings'} {updates} onwhatsnew={() => updateNotesRef?.open()} {theme} {transport} {autoScan} {appIcon} {presence} wfmStatus={listing.wfmStatus} onwfmlogout={() => listing.handleWfmLogout()} section={settingsSection} onsectionshown={() => (settingsSection = null)} />
    </div>
    {/if}

    {/if}
    {#if showWorkspace}<KeyLegend search={['sell', 'meta', 'orders', 'watches'].includes(effectiveView)} />{/if}
  </main>
  {#if showWorkspace}
  <!-- After main in source so a narrow window reaches its links at the page
       foot instead of above the first decision; wide windows place it under
       the sidebar. -->
  <!-- The rail tip only repeats each control's own name; the controls inside stay the interaction. -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div data-shell class="sfoot" onpointerover={showTip} onpointerleave={hideTip} onfocusin={showTip} onfocusout={hideTip}>
    <button data-shell type="button" class="rail-toggle" aria-label={sidebar.collapsed ? 'Expand sidebar' : 'Collapse sidebar'} aria-expanded={!sidebar.collapsed} onclick={() => { sidebar.toggle(); tip = null; }}>
      <NavIcon name={sidebar.collapsed ? 'expand' : 'collapse'} /><span data-shell class="label">Collapse</span>
    </button>
    <KeyboardNavigation desktop compact {store} extra={settingsDestinations} />
    <nav data-shell class="project-links" aria-label="Project links">
      {@render projectLinkAnchors()}
    </nav>
    <button data-shell type="button" class="feedback-trigger" onclick={openFeedback}>
      <svg data-shell viewBox="0 0 24 24" aria-hidden="true"><path data-shell d="M4 4h16v12H9l-5 4V4Z" /><path data-shell d="M8 8h8M8 12h5" /></svg>
      <span data-shell class="label">Send feedback</span>
    </button>
    <div data-shell class="ver" title="build {APP_COMMIT}" data-testid="app-version">{versionLabel}</div>
  </div>
  {#if tip}<div data-shell class="rail-tip" aria-hidden="true" style:top="{tip.top}px" style:left="{tip.left}px">{tip.text}</div>{/if}
  {/if}
</div>

{#snippet keepSection()}
  <ProtectedPlan controller={protection} owned={inventory.resolved.owned} market={inventory.market} onconnect={connectForListings}
    reserveCopies={filters.reserveCopies} onsetKeep={async (value) => { await store.setSetting('reserve-copies', String(value)); filters.reserveCopies = value; }}
    unavailableCount={unknownSlugs.size} unavailable={guidanceUnavailable} scanning={inventory.pullingInventory} onscan={() => inventory.pullInventory()} />
  {#if estimatedGuidance && !guidanceUnavailable}
    <p class="ui-notice" role="status" aria-label="Estimated guidance">Estimates apply your keep rules. Existing WFM listings are not subtracted. Check WFM listings before posting.</p>
  {/if}
{/snippet}

{#snippet projectLinkAnchors()}
  <a data-shell class="project-link" href="https://github.com/tennoworth/tennoworth" target="_blank" rel="noopener noreferrer">
    <svg data-shell class="github-mark" viewBox="0 0 24 24" aria-hidden="true">
      <path data-shell d="M12 2.7a9.5 9.5 0 0 0-3 18.5c.5.1.7-.2.7-.5v-1.9c-2.8.6-3.4-1.2-3.4-1.2-.5-1.2-1.1-1.5-1.1-1.5-.9-.6.1-.6.1-.6 1 0 1.6 1 1.6 1 .9 1.6 2.4 1.1 3 .8.1-.7.4-1.1.7-1.3-2.2-.3-4.6-1.1-4.6-4.7 0-1 .4-1.9 1-2.6-.1-.3-.4-1.3.1-2.6 0 0 .8-.3 2.7 1a9.2 9.2 0 0 1 4.9 0c1.9-1.3 2.7-1 2.7-1 .5 1.3.2 2.3.1 2.6.6.7 1 1.6 1 2.6 0 3.7-2.4 4.5-4.6 4.7.4.3.7.9.7 1.8v2.8c0 .4.2.6.7.5A9.5 9.5 0 0 0 12 2.7Z" />
    </svg>
    <span data-shell class="label">GitHub</span>
  </a>
  <a data-shell class="project-link" href="https://ko-fi.com/prowly" target="_blank" rel="noopener noreferrer">
    <svg data-shell viewBox="0 0 24 24" aria-hidden="true">
      <path data-shell d="M4 7 H17 V16 H6 L4 14 Z M17 9 H19 L21 11 V13 L19 15 H17 M7 10 L10 13 L14 9" />
    </svg>
    <span data-shell class="label">Buy me a coffee</span>
  </a>
{/snippet}

{#snippet generalBanners()}
  {#if marketAccess.message}
    <div class="ui-notice" data-tone="warn" role="status">{marketAccess.message}</div>
  {/if}
  {#if inventory.noTradeables && inventory.pullError}
    <section data-shell class="ui-notice ui-stack" data-tone="warn" role="status" aria-label="No tradeable items found">
      <strong data-shell>No tradeable items found</strong>
      <p data-shell>{inventory.pullError}</p>
      {#if hasInventory}<p data-shell>Showing your saved inventory. This scan did not replace its quantities.</p>{/if}
      <div data-shell class="ui-toolbar">
        <button data-shell class="btn" onclick={() => inventory.pullInventory()} disabled={inventory.pullingInventory}>Scan again</button>
        <button data-shell class="btn ghost" onclick={() => { inventory.pullError = null; }}>Dismiss scan notice</button>
      </div>
    </section>
  {:else if inventory.error || inventory.pullError}
    <section data-shell class="ui-notice ui-stack" data-tone="bad" role="alert" aria-label="Inventory unavailable">
      <strong data-shell>Your inventory couldn’t be refreshed</strong>
      <p data-shell>{hasInventory ? 'Showing your last successful inventory. Its quantities have not been refreshed.' : 'Try scanning again, or check for an app update. Settings and help are still available.'}</p>
      <details data-shell>
        <summary data-shell>Technical details</summary>
        <p data-shell class="recovery-details">{inventory.pullError ?? inventory.error}</p>
      </details>
      <div data-shell class="ui-toolbar">
        <button data-shell class="btn primary" onclick={() => inventory.pullInventory()} disabled={inventory.pullingInventory}>{inventory.pullingInventory ? 'Scanning game…' : 'Retry scan'}</button>
        <button data-shell class="btn" onclick={() => updates.check({ announce: true })}>Check for updates</button>
        <button data-shell class="btn" onclick={() => filters.setView('settings')}>Settings</button>
        <button data-shell class="btn" onclick={openFeedback}>Report a bug</button>
        <button data-shell class="btn ghost" onclick={() => { inventory.error = null; inventory.pullError = null; }}>Dismiss scan error</button>
      </div>
    </section>
  {/if}
  {#if trayHint}
    <div data-shell class="card ui-panel warn-banner general-banner" role="status">
      <div data-shell class="gb-body">
        Still running in your tray. Closing the window keeps TennoWorth in the
        background - use the tray icon's Quit to exit.
      </div>
      <div data-shell class="gb-actions">
        <button data-shell class="gb-dismiss" aria-label="Dismiss" onclick={() => (session.trayHint = false)}>×</button>
      </div>
    </div>
  {/if}
  {#if autoScan.pending}
    <div data-shell class="card ui-panel warn-banner general-banner" role="status">
      <div data-shell class="gb-body">
        An automatic scan finished at {new Date(autoScan.pending.at).toLocaleTimeString()}.
        Load it to update quantities in the tables.
      </div>
      <div data-shell class="gb-actions">
        <button data-shell class="btn" onclick={() => void autoScan.loadPending()}>Load new scan</button>
        <button data-shell class="gb-dismiss" aria-label="Dismiss" onclick={() => autoScan.dismiss()}>×</button>
      </div>
    </div>
  {/if}
  <WatchOfferBanner onwatches={() => filters.setView('watches')} />
  <!-- Kept out of first run and of trading, where it would compete with the task. -->
  <PriceSharingPrompt {transport} session={prompts} active={promptsActive && effectiveView !== 'settings'} onsettings={() => { settingsSection = 'price-sharing'; filters.setView('settings'); }} />
  <SupportPrompt session={prompts} active={promptsActive} />
    <DesktopUpdateBanner {updates} />
  
{/snippet}

{#snippet marketAnnouncement()}
  <MarketAnnouncement now={displayNow} {store} load={notesServices.desktopAnnouncements} subscribe={(refresh) => listenForTauriEvent(ANNOUNCEMENTS_CHANGED_EVENT, refresh)} />
{/snippet}

{#snippet pendingBanner()}
  <PendingBatchBanner {listing} {marketAccess} onorders={() => filters.setView('orders')} />
{/snippet}

<!-- Desktop only (listing needs wfm-trading's session). onauthrequired fires on
     typed needs_login/needs_unlock rejections. -->
<ListingReviewModal
    {listingBlockReason} currentSnapshotId={inventory.nativeSnapshotId} onrecheck={checkListingRequirements} {listingActionLabel}
  bind:open={listing.listingOpen}
  sessionEpoch={listing.sessionEpoch}
  rows={listing.reviewRowsOverride ?? listableRows.slice(0, 50).map(row => ({ ...row, inventory_snapshot_id: inventory.nativeSnapshotId ?? undefined }))}
  {transport}
  onauthrequired={(code) => wfmAuthDialogsRef?.open(code, 'list')}
  sendThrough={(send) => activity.track('Sending listings…', () => listing.trackSend(send), (response) => sendSummary(response as PlanResponse), () => 'Listing failed')}
  onsent={(sent, results) => receipts.record(sent, results)}
  onvisible={(results) => { receipts.markVisible(results); const shown = results.filter((r) => r.status === 'ok').length; if (shown) activity.finish(activity.begin(''), `${shown} now visible`); }}
  onclose={() => { listing.reviewRowsOverride = null; void protection.refresh(); void listing.refreshPendingPlan(); }}
/>

  <WfmAuthDialogs bind:this={wfmAuthDialogsRef} onunlocked={(next) => { listing.handleWfmUnlocked(next); void protection.refresh(); }} onautherror={(error) => (wfmAuthFailure = error)} onreport={openFeedback} />

<ExportImportDialogs
  bind:this={exportImportRef}
  owned={inventory.resolved.owned}
  inventoryName={inventory.inventoryName}
  lastUpdated={inventory.lastUpdated}
  onimport={(result) => inventory.handleImported(result)}
/>

<UpdateNotes bind:this={updateNotesRef} services={notesServices} ready={notesReady} blocked={outstanding > 0 || listing.resumePhase !== 'idle'} onautomatic={(state) => prompts.notes(state)} />

{#snippet firstRunKeyboard()}<KeyboardNavigation desktop {store} />{/snippet}
