<script lang="ts">
import { createDesktopSession } from './desktop-session.svelte';
import { createSellWorkspace } from '../features/selling/sell-workspace.svelte';
import StatusStrip from './StatusStrip.svelte';
import FeedbackDialog from '../features/settings/FeedbackDialog.svelte';
import PendingBatchBanner from '../features/selling/PendingBatchBanner.svelte';
import BaroView from '../features/market-context/BaroView.svelte';
import RelicPlannerView from '../features/relics/RelicPlannerView.svelte';
import SetPicksView from '../features/selling/SetPicksView.svelte';

  import { loadMarket } from '../adapters/market';
  import { loadCatalogs } from '../adapters/catalogs';
  import { TauriTransport, parseScanPayload } from '../adapters/desktop';
  import { useDesktopServices } from '../ui/desktop-context';
  const { desktopAccessStatus, desktopWfmStatus, desktopWfmLogout, listenForTauriEvent, desktopProtectionState, desktopSaveProtectionPlan, normalizeInventoryNative } = useDesktopServices();
  import { ProtectionController } from '../features/selling/protection.svelte';
  import ProtectedPlan from '../features/selling/ProtectedPlan.svelte';
  import { type WfmSession } from '../features/settings/feedback';
  import { humanError } from '../contracts/errors';
  
  import { onMount, untrack } from 'svelte';
  import Faq from './Faq.svelte';
  import { FilterController, type View } from '../features/selling/filters.svelte';
  import { ListingController, WfmAccessController } from '../features/selling/controller.svelte';
  import { InventoryController } from '../features/inventory/controller.svelte';
  import { AutoScanController } from '../features/inventory/auto-scan.svelte';
  import ListingReviewModal from '../features/selling/ListingReviewModal.svelte';
  import MyOrdersPanel from '../features/orders/MyOrdersPanel.svelte';
  import WatchlistPanel from '../features/watches/WatchlistPanel.svelte';
  import NotificationInbox from '../features/settings/NotificationInbox.svelte';
  
import { ALLOWANCE_CHANGED_EVENT } from '../contracts/events';
  import LedgerPanel from '../features/ledger/LedgerPanel.svelte';
  import MarketBrowser from '../features/market-context/MarketBrowser.svelte';
  import DesktopUpdateBanner from '../ui/DesktopUpdateBanner.svelte';
  import WfmAuthDialogs from '../features/settings/WfmAuthDialogs.svelte';
  import ExportImportDialogs from '../features/inventory/ExportImportDialogs.svelte';
  import SellPane from '../features/selling/SellPane.svelte';
  import TradeSessionPane from '../features/selling/TradeSessionPane.svelte';
  import RivensPanel from '../features/rivens/RivensPanel.svelte';
  import ThemeSwitcher from '../ui/ThemeSwitcher.svelte';
  import UpdateNotes from '../ui/UpdateNotes.svelte';
  import SettingsPanel from '../features/settings/SettingsPanel.svelte';
  import PriceSharingPrompt from '../features/settings/PriceSharingPrompt.svelte';
  import RoutinesPanel from '../features/routines/RoutinesPanel.svelte';
  import { RoutineController } from '../features/routines/controller.svelte';
  import { resolveRivens } from '../domain/rivens';

  import { buildMetaDrift } from '../domain/meta-drift';

  import { interruptedBatch } from '../domain/listing-plan';

  const APP_COMMIT = __APP_COMMIT__;
  import type { StateStore } from '../contracts/state-store';
  import type { ThemeController } from '../ui/theme';

  import MetaDriftPanel from '../features/market-context/MetaDriftPanel.svelte';

  // Desktop (Tauri) vs hosted informational (browser) is decided ONCE at boot.
  // The hosted site is informational only: market data + the desktop showcase,
  // no files. Everything interactive - scan, list, orders, login - lives in
  // the desktop app, driven by the wfm_session commands.
  let updateNotesRef: UpdateNotes;
  // Set only by the inbox's settings link; every other way in opens the top.
  let settingsSection = $state<'notifications' | 'price-sharing' | null>(null);

  const notesServices = useDesktopServices();
  const transport = new TauriTransport();
  const marketAccess = new WfmAccessController({ desktopAccessStatus, listenForTauriEvent });
  onMount(() => marketAccess.start());

  // Persistence seam: localStorage in the browser, SQLite-over-IPC in desktop.
  // Selected + primed (scalar settings loaded into cache) in main.ts and passed
  // in, so the scalar-setting `$state` initializers below can read it
  // synchronously with no first-paint flash. Snapshot methods are async.
  // `theme` is the boot-time ThemeController (src/lib/theme.ts) that the mode
  // control in Settings → Appearance (and its footer twin) drives.
  let { store, theme }: { store: StateStore; theme: ThemeController } = $props();
  const filters = untrack(() => new FilterController(store));
  const routines = untrack(() => new RoutineController(store));
  const inventory = untrack(() => new InventoryController(store, transport, { loadMarket, loadCatalogs, normalizeInventory: normalizeInventoryNative }));
  const protection = new ProtectionController({ desktopProtectionState, desktopSaveProtectionPlan });
  const listing = new ListingController({ getPendingPlan: () => transport.getPendingPlan(), resumePendingPlan: () => transport.resumePendingPlan(), discardPendingPlan: () => transport.discardPendingPlan(), status: desktopWfmStatus, logout: desktopWfmLogout }, (code, next) => wfmAuthDialogsRef?.open(code, next));
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
  onMount(() => {
    const timer = setInterval(() => { if (!protection.loading && !protection.saving) void protection.refresh(); }, 30_000);
    const stop = listenForTauriEvent(ALLOWANCE_CHANGED_EVENT, () => void protection.refresh());
    const stopAutoScan = autoScan.start();
    void autoScan.load();
    return () => { clearInterval(timer); stop(); stopAutoScan(); protection.destroy(); };
  });
  $effect(() => {
    const owned = inventory.resolved.owned;
    const snapshotId = inventory.nativeSnapshotId;
    untrack(() => void protection.setInventory(owned, snapshotId));
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
  let showWorkspace = $derived(hasInventory || inventory.phase === 'done' || effectiveView !== 'sell');
  let updateBanner: DesktopUpdateBanner;
</script>

<FeedbackDialog bind:this={feedbackRef} captureState={captureFeedbackState} services={notesServices} onclosed={() => { if (feedbackFromMore) statusStripRef?.focusMore(); feedbackFromMore = false; }} />

<!-- Keep the banner region mounted while navigation or a scan changes the content. -->
<div data-shell class={showWorkspace ? 'shell' : 'desktop-landing'}>
  <StatusStrip bind:this={statusStripRef} inShell={showWorkspace} {inventory} {listing} {filters} {unresolvedCount} {unresolvedSummary} {inventoryFreshness} {inventoryStaleness} {inventoryTimestamp} {marketFreshness} {marketStaleness} {ordersToFix} {baroState} {unreadNotifications} {wfmLabel} {projectLinkAnchors} onexport={() => exportImportRef?.openExport()} onimport={() => exportImportRef?.pickImport()} onclear={() => workspace.clear()} onupdates={() => updateBanner.checkForUpdates()} onfeedback={() => { feedbackFromMore = true; openFeedback(); }} onauth={(code) => wfmAuthDialogsRef?.open(code)} />
  {#if showWorkspace}
  <aside data-shell class="sidebar">
    <nav data-shell>
      <div data-shell class="nav-group">
        <div data-shell class="nav-label">Trade</div>
        <button data-shell type="button" class="nav-item" data-testid="nav-sell" class:active={effectiveView === 'sell'} onclick={() => filters.setView('sell')}>
          <span data-shell>{hasInventory ? estimatedGuidance ? 'Opportunities' : 'Sell' : 'Inventory'}</span>
          <!-- Pinned to the unfiltered sellable count: with a narrow preset
               active (Vaulted on a no-vaulted inventory), a filter-driven
               "Sell 0" reads as "your inventory got wiped". -->
          {#if hasInventory}<span data-shell class="badge">{defaultFacts.phase === 'done' && !guidanceUnavailable ? sellableCount : '—'}</span>{/if}
        </button>
        
          <button data-shell type="button" class="nav-item" class:active={effectiveView === 'session'} onclick={() => filters.setView('session')}><span data-shell>Trade Session</span></button>
        
          <button data-shell type="button" class="nav-item" class:active={effectiveView === 'sets'} onclick={() => filters.setView('sets')}>
            <span data-shell>Set picks</span>
            <span data-shell class="badge">{setResult.phase === 'done' ? setRecos.length : '—'}</span>
          </button>
        {#if relicPlan.length > 0 || relicResult.phase === 'loading' || relicResult.error}
          <button data-shell type="button" class="nav-item" class:active={effectiveView === 'relics'} onclick={() => filters.setView('relics')}>
            <span data-shell>Relics</span>
            <span data-shell class="badge">{relicResult.phase === 'done' ? relicPlan.length : '—'}</span>
          </button>
        {/if}
        {#if resolvedRivens.length > 0}
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
          {#if listing.pendingPlan && outstanding > 0}<span data-shell class="badge warn">{outstanding}</span>{/if}
        </button>
        <button data-shell type="button" class="nav-item" class:active={effectiveView === 'watches'} onclick={() => filters.setView('watches')}>
          <span data-shell>Price watches</span>
        </button>
        <button data-shell type="button" class="nav-item" class:active={effectiveView === 'notifications'} onclick={() => filters.setView('notifications')}>
          <span data-shell>Notifications</span>{#if unreadNotifications}<span data-shell class="badge unread">{unreadNotifications}</span>{/if}
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

  </aside>

  {/if}
  <main data-shell class={showWorkspace ? 'workspace' : 'landing'} class:reading-view={['sets', 'relics', 'routines', 'install', 'settings'].includes(effectiveView)} data-testid={!showWorkspace ? 'desktop-mode' : undefined}>
    {@render generalBanners()}
    {#if !showWorkspace}
  {#if !inventory.error && !inventory.pullError}
    <!-- Scanning is the first thing to do here, so it leads; settings, updates
         and feedback live in the header. -->
    <section data-shell class="upsell-lead desktop-hero" aria-labelledby="desktop-hero-title">
      <h2 data-shell id="desktop-hero-title">Get your personal sell list</h2>
      <p data-shell class="sub">Scan once and TennoWorth ranks <em data-shell>your</em> inventory by what to sell right now.</p>
      <p data-shell class="req">Warframe must be open and past the login screen.</p>
      <div data-shell class="desktop-scan-row">
        <button data-shell class="rp-primary" data-testid="desktop-scan" onclick={() => inventory.pullInventory()} disabled={inventory.pullingInventory}>{inventory.pullingInventory ? 'Scanning game…' : 'Scan inventory'}</button>
        <span data-shell class="trust">Reads the running game's memory only - nothing leaves your machine.</span>
      </div>
    </section>
  {/if}
  {#if inventory.market}
    <p data-shell class="lookup-label">Or look anything up</p>
    <MarketBrowser market={inventory.market} staleness={marketStaleness} freshness={marketFreshness} loadHistory={() => transport.loadHistory()} />
  {/if}
  <Faq desktop />

  <footer data-shell class="sitefoot">
    <span data-shell class="grow">TennoWorth is a fan project, not affiliated with Digital Extremes or warframe.market. Open source · MIT · data from warframe.market and warframestat.us.</span>
    {#if inventory.market?.updated_at}<span data-shell title="When the market snapshot was taken">Snapshot {snapshotStamp}</span>{/if}
    <a data-shell href="#trust">Trust &amp; safety</a>
    <span data-shell class="ver" title="build {APP_COMMIT}">{versionLabel}</span>
    <div data-shell class="foot-theme"><ThemeSwitcher {theme} compact label="Colour mode" /></div>
  </footer>
    {:else}
    {#if advisorResult.error && !filterState.adviceOnly && ['sell', 'sets', 'session'].includes(effectiveView)}
      <div class="ui-notice" data-tone="warn" role="status">Hold/sell advice unavailable: {advisorResult.error} <button class="btn" onclick={() => workspace.calculationEpoch += 1}>Retry calculations</button></div>
    {/if}

    {#if effectiveView === 'sell'}
      <SellPane keep={keepSection}
        bind:minPrice={filters.minPrice} bind:minOwned={filters.minOwned} bind:typeFilter={filters.typeFilter} bind:hideAtLvl={filters.hideAtLvl} bind:activeTags={filters.activeTags}
        bind:tableView={workspace.tableView}
        resolved={inventory.resolved} allocation={allocationMatches ? protection.state : null} {results} deltas={inventory.deltas} {totalPotential}
        prevSummary={estimatedGuidance ? null : prevSummary} {sinceScan} ordersSummary={estimatedGuidance ? null : listing.ordersSummary}
        {marketFreshness} {marketStaleness} marketLoadError={inventory.marketLoadError}
        {listableRows} {availableTags} {availableTypes}
        {visibleColumns} {presetSort} {emptyReason}
        columnsCustomized={filters.columnKey in filters.columnChoice} oncolumnschange={(columns) => filters.setColumns(columns)}
        activePreset={filters.activePreset} reserveCopies={filters.reserveCopies} filtersOpen={filters.filtersOpen} scoreExplainerDismissed={filters.scoreExplainerDismissed}
        sellOnboardingDismissed={filters.sellOnboardingDismissed} keepCopiesNudgeDismissed={filters.keepCopiesNudgeDismissed}
        applyPreset={(name) => filters.applyPreset(name)} setReserveCopies={(value) => filters.setReserveCopies(value)} toggleFiltersOpen={(event) => filters.toggleFiltersOpen(event)}
        dismissSellOnboarding={() => filters.dismissSellOnboarding()} dismissKeepCopiesNudge={() => filters.dismissKeepCopiesNudge()}
        openListingFlow={(rows) => { if (calculationsReady && !estimatedGuidance) listing.openListingFlow((Array.isArray(rows) ? rows : rows ? [rows] : listableRows).map(row => ({ ...row, inventory_snapshot_id: inventory.nativeSnapshotId ?? undefined }))); }}
        {estimatedGuidance} oncheckListings={checkListingRequirements} canList={listingQuantitiesKnown} {listingActionLabel} unavailableCount={unknownSlugs.size}
        {pendingBanner}
        {calculationPending} {calculationError} calculationErrorShown={guidanceUnavailable} onretryCalculation={() => workspace.calculationEpoch += 1}
      />
    {:else if effectiveView === 'session'}
      {#if defaultFacts.phase === 'loading'}
        <div class="ui-notice" role="status">Calculating safe quantities and sale values…</div>
      {:else if defaultFacts.error}
        <div class="ui-notice" data-tone="bad" role="alert">Sale calculations unavailable: {defaultFacts.error} <button class="btn" onclick={() => workspace.calculationEpoch += 1}>Retry calculations</button></div>
      {/if}
      <TradeSessionPane keep={keepSection} {listingBlockReason} onrecheck={checkListingRequirements} {listingActionLabel} owned={inventory.resolved.owned} market={inventory.market} reserveCopies={filters.reserveCopies} advice={adviceMap} nativeFacts={defaultFacts.value} {availability}
        scanning={inventory.pullingInventory} onscan={async () => { await inventory.pullInventory(); await protection.refresh(); }} onreview={(rows, budget, state) => { if (!listingQuantitiesKnown) return; listing.openListingFlow(rows.map(r => ({
          ...r, inventory_snapshot_id: inventory.nativeSnapshotId ?? undefined, proposed_quantity: r.quantity, clearing_price: r.platinum, low_sell: r.platinum,
          avg_price: r.market.avg, session: { snapshot_id: state.allowance.snapshot_id!, utc_day: state.allowance.utc_day, budget },
        }))); }} />
    {:else if effectiveView === 'sets'}
      <SetPicksView {inventory} {setSurfaceAge} {keepSection} {guidanceUnavailable} {setResult} {setRecos} {adviceMap} onretry={() => workspace.calculationEpoch += 1} />
    {:else if effectiveView === 'relics'}
      <RelicPlannerView bind:relicShowAll {relicResult} {relicSurfaceAge} {relicPlan} onretry={() => workspace.calculationEpoch += 1} />
    {:else if effectiveView === 'rivens'}
      <RivensPanel market={inventory.market} rivens={resolvedRivens} />
    {:else if effectiveView === 'baro'}
      <BaroView {inventory} {baroSurfaceAge} {keepSection} {guidanceUnavailable} {voidTrader} {ducatStats} {baroState} {guidanceOwned} {guidanceAvailability} {unknownSlugs} onducats={() => { filters.setView('sell'); filters.applyPreset('ducats'); }} />
    {:else if effectiveView === 'routines'}
      <RoutinesPanel routine={routines} market={inventory.market} owned={inventory.resolved.owned} now={displayNow} />

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
        {marketStaleness}
        onauthrequired={(code) => wfmAuthDialogsRef?.open(code)}
        onsummary={(s) => (listing.ordersSummary = s)}
      />

    {:else if effectiveView === 'watches'}
      <WatchlistPanel market={inventory.market} />

    {:else if effectiveView === 'notifications'}
      <NotificationInbox onopen={(target) => filters.setView(target)} onsettings={() => { settingsSection = 'notifications'; filters.setView('settings'); }} />

    {:else if effectiveView === 'ledger'}
      <LedgerPanel onsetautoclose={(on) => store.setSetting('auto-close-sold', on ? 'on' : 'off')} />

    {:else if effectiveView === 'install'}
      <section data-shell class="view-header">
        <h2 data-shell>FAQ</h2>
        <p data-shell class="lede">Answers to common questions.</p>
      </section>
      <Faq desktop />

    {:else if effectiveView === 'settings'}
      <SettingsPanel onwhatsnew={() => updateNotesRef?.open()} {theme} {transport} {autoScan} wfmStatus={listing.wfmStatus} onwfmlogout={() => listing.handleWfmLogout()} section={settingsSection} onsectionshown={() => (settingsSection = null)} />
    {/if}

    {/if}
  </main>
  {#if showWorkspace}
  <!-- After main in source so a narrow window reaches its links at the page
       foot instead of above the first decision; wide windows place it under
       the sidebar. -->
  <div data-shell class="sfoot">
    <nav data-shell class="project-links" aria-label="Project links">
      {@render projectLinkAnchors()}
    </nav>
    <button data-shell type="button" class="feedback-trigger" onclick={openFeedback}>
      <svg data-shell viewBox="0 0 24 24" aria-hidden="true"><path data-shell d="M4 4h16v12H9l-5 4V4Z" /><path data-shell d="M8 8h8M8 12h5" /></svg>
      Send feedback
    </button>
    <div data-shell class="ver" title="build {APP_COMMIT}" data-testid="app-version">{versionLabel}</div>
  </div>
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
    <span data-shell>GitHub</span>
  </a>
  <a data-shell class="project-link" href="https://ko-fi.com/prowly" target="_blank" rel="noopener noreferrer">
    <svg data-shell viewBox="0 0 24 24" aria-hidden="true">
      <path data-shell d="M4 7 H17 V16 H6 L4 14 Z M17 9 H19 L21 11 V13 L19 15 H17 M7 10 L10 13 L14 9" />
    </svg>
    <span data-shell>Buy me a coffee</span>
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
        <button data-shell class="btn" onclick={() => updateBanner.checkForUpdates()}>Check for updates</button>
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
  <!-- Kept out of first run and of trading, where it would compete with the task. -->
  <PriceSharingPrompt {transport} {store} active={hasInventory && !listing.listingOpen && effectiveView !== 'session' && effectiveView !== 'settings'} onsettings={() => { settingsSection = 'price-sharing'; filters.setView('settings'); }} />
    <DesktopUpdateBanner bind:this={updateBanner} />
  
{/snippet}

{#snippet pendingBanner()}
  <PendingBatchBanner {listing} {marketAccess} onorders={() => filters.setView('orders')} />
{/snippet}

<!-- Desktop only (listing needs wfm-core's session). onauthrequired fires on
     typed needs_login/needs_unlock rejections. -->
<ListingReviewModal
    {listingBlockReason} currentSnapshotId={inventory.nativeSnapshotId} onrecheck={checkListingRequirements} {listingActionLabel}
  bind:open={listing.listingOpen}
  rows={listing.reviewRowsOverride ?? listableRows.slice(0, 50).map(row => ({ ...row, inventory_snapshot_id: inventory.nativeSnapshotId ?? undefined }))}
  {transport}
  onauthrequired={(code) => wfmAuthDialogsRef?.open(code, 'list')}
  sendThrough={(send) => listing.trackSend(send)}
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

<UpdateNotes bind:this={updateNotesRef} services={notesServices} ready={notesReady} blocked={outstanding > 0 || listing.resumePhase !== 'idle'} />
