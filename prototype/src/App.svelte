<script lang="ts">
  // @ts-nocheck — App.svelte is presentation glue: dialog refs, catch
  // blocks, event handlers. The high-value typing already lives at the
  // lib/ boundary (Market, Inventory, OwnedRecord etc.). Annotating
  // every `catch (e: unknown)` and `dialog: HTMLDialogElement | undefined`
  // here would be busy-work that catches no real bugs. Revisit if a
  // refactor extracts state into a typed store.
  import { onMount, untrack } from 'svelte';
  import ListingReviewModal from './components/ListingReviewModal.svelte';
  import MyOrdersPanel from './components/MyOrdersPanel.svelte';
  import WatchlistPanel from './components/WatchlistPanel.svelte';
  import LedgerPanel from './components/LedgerPanel.svelte';
  import MarketBrowser from './components/MarketBrowser.svelte';
  import DesktopUpdateBanner from './components/DesktopUpdateBanner.svelte';
  import WfmAuthDialogs from './components/WfmAuthDialogs.svelte';
  import ExportImportDialogs from './components/ExportImportDialogs.svelte';
  import SellPane from './components/SellPane.svelte';
  import DesktopShowcase from './components/DesktopShowcase.svelte';
  import ThemeSwitcher from './components/ThemeSwitcher.svelte';
  import { flattenInventory, extractKeptLvls } from './lib/inventory';
  import { loadCatalogs, resolvePath, type Catalogs } from './lib/resolver';
  import { loadMarket, lookup } from './lib/market';
  import { sellableQty } from './lib/sell-priority';
  import { computeResults as computeFilteredResults, computeAvailableTags, computeEmptyReason, type FilterState } from './lib/filter-engine';
  import { PRESETS, presetFilterValues, presetStillMatches } from './lib/presets';

  const APP_COMMIT = __APP_COMMIT__;
  import { deriveSetRecos } from './lib/set-recos';
  import { deriveRelicPlan } from './lib/relic-planner';
  import { diffOwned } from './lib/storage';
  import type { StateStore } from './lib/state-store';
  import type { ThemeController } from './lib/theme';
  import {
    createTransport, isDesktopRuntime,
    desktopWfmStatus, DesktopCmdError,
  } from './lib/transport';
  import type { Market, OwnedRecord } from './lib/types';
  import { wfmItemUrl, baroLocation, humanWindow } from './lib/format';
  import { listenForTauriEvent, TRAY_HINT_EVENT } from './lib/desktop-update';

  // Desktop (Tauri) vs hosted informational (browser) is decided ONCE at boot.
  // The hosted site is informational only: market data + the desktop showcase,
  // no files. Everything interactive — scan, list, orders, login — lives in
  // the desktop app, driven by the wfm_session commands.
  const isDesktop = isDesktopRuntime();
  const transport = createTransport();

  // Persistence seam: localStorage in the browser, SQLite-over-IPC in desktop.
  // Selected + primed (scalar settings loaded into cache) in main.ts and passed
  // in, so the scalar-setting `$state` initializers below can read it
  // synchronously with no first-paint flash. Snapshot methods are async.
  // `theme` is the boot-time ThemeController (src/lib/theme.ts) the switchers drive.
  let { store, theme }: { store: StateStore; theme: ThemeController } = $props();

  type Phase = 'idle' | 'loading' | 'done' | 'error';
  let phase = $state<Phase>('idle');
  let error = $state<string | null>(null);
  // Non-fatal: a price-snapshot load failure (offline, transient market.json
  // 404 during the cron push) when we already have a restored inventory. We
  // render the snapshot and flag prices unavailable instead of throwing the
  // user back to the cold-start landing.
  let marketLoadError = $state<string | null>(null);
  let inventoryName = $state<string | null>(null);
  let lastUpdated = $state<number | null>(null);

  let catalogs = $state<Catalogs | null>(null);
  let market = $state<Market | null>(null);
  let resolved = $state<{ owned: Map<string, OwnedRecord>; unresolved: Record<string, number> }>({
    owned: new Map(),
    unresolved: {},
  });
  let deltas = $state<Map<string, number>>(new Map());
  // The snapshot the current scan was diffed against — kept for the session
  // (same lifetime as `deltas`) so the Sell summary can say what changed since
  // the last scan in the same units the table shows (sellable rows, potential
  // plat), not just per-row counts. Null after a restore-from-storage boot,
  // exactly like `deltas` is empty then.
  let previousOwned = $state<Map<string, OwnedRecord> | null>(null);
  let results = $state<any[]>([]);
  // The Sell table pushes its filtered+sorted rows up here so the "List on WFM"
  // CTA stages exactly what the user sees (name filter + badge chips), not the
  // unfiltered preset results.
  let tableView = $state<{ rows: any[]; active: boolean }>({ rows: [], active: false });
  // Rows eligible for the bulk "List on WFM" action: the table-filtered set when
  // a table filter is active, else all results — minus relics (subtyped rows),
  // since selling an intact relic at a few plat usually loses to cracking it
  // (the Relic planner ranks those), so they shouldn't be staged by default.
  let listableRows = $derived(
    (tableView.active ? tableView.rows : results).filter((r) => !r.subtype && r.sellable > 0)
  );
  let minPrice = $state(5);
  let minOwned = $state(1);
  // "Keep copies" reserve — holds back N copies of every owned item from
  // being listed (sellable = owned − N). Distinct from minOwned, which only
  // hides rows from view; this changes what's actually sellable, so unlike
  // minOwned/minPrice it's worth persisting — losing your only copy of
  // something to an underpriced snipe is the mistake this exists to prevent.
  let reserveCopies = $state(
    (() => {
      const n = parseInt(store.getSetting('reserve-copies'), 10);
      return Number.isFinite(n) && n >= 0 ? n : 0;
    })()
  );
  // Takes the number input's event, or a plain number (the row-B "keep N
  // copies ×" chip clears it without a synthetic event).
  function setReserveCopies(e) {
    const raw = typeof e === 'number' ? e : e.currentTarget.value;
    const n = Math.max(0, parseInt(raw, 10) || 0);
    reserveCopies = n;
    void store.setSetting('reserve-copies', String(n));
  }
  let typeFilter = $state('all');
  // Hide rows when the user has a copy of the mod ranked to ≥ `hideAtLvl`
  // in `Upgrades`. Per-record `kept_lvl` is `null` for items the user has
  // no leveled copy of, so those always show. Threshold of 5 catches
  // regular maxed mods (most cap at lvl 5); 10 catches only
  // primed/galvanized maxed; 0 catches any individualised instance
  // including lvl 0 rivens; 11 effectively disables the filter.
  let hideAtLvl = $state(5);
  // Tag-chip filter. WFM /v2/items returns tags like `prime`, `mod`,
  // `relic`, `arcane_enhancement`, `syndicate`, … per slug. Chips OR
  // within themselves (selecting `prime` + `mod` shows rows with EITHER
  // tag) and AND with the price/owned/type/kept filters. Empty set =
  // no tag restriction.
  let activeTags = $state<Set<string>>(new Set());

  // Filter rail is collapsed by default — user feedback (casual flipper)
  // found a visible "tax form" intimidating; power users open it once and
  // it stays open via localStorage. Tag chips + table search stay visible.
  let filtersOpen = $state((() => store.getSetting('filters-open') === '1')());
  function toggleFiltersOpen(e: Event): void {
    const isOpen = (e.currentTarget as HTMLDetailsElement).open;
    filtersOpen = isOpen;
    void store.setSetting('filters-open', isOpen ? '1' : '0');
  }

  // Sidebar nav view — which workspace pane is active. Persists so a
  // reload lands the user back where they left off. Falls through to
  // 'sell' if the persisted view's data isn't available (Baro not
  // visiting; 'orders' is desktop-only — the hosted site is informational).
  type View = 'sell' | 'sets' | 'relics' | 'baro' | 'routines' | 'orders' | 'watches' | 'ledger' | 'install';
  const VALID_VIEWS: ReadonlySet<View> = new Set([
    'sell', 'sets', 'relics', 'baro', 'routines', 'orders', 'watches', 'ledger', 'install',
  ]);
  let view = $state<View>(
    (() => {
      const saved = store.getSetting('view') as View | null;
      return saved && VALID_VIEWS.has(saved) ? saved : 'sell';
    })()
  );
  function setView(v: View): void {
    view = v;
    void store.setSetting('view', v);
  }

  // Sidebar nav: if the user's persisted view is unavailable (Baro not
  // visiting, orders on the informational site), fall back to Sell rather than
  // rendering an empty pane. The nav itself hides those entries; this protects
  // against a stale localStorage value.
  let effectiveView = $derived.by<View>(() => {
    if (view === 'baro' && !showBaroCard) return 'sell';
    if ((view === 'orders' || view === 'watches' || view === 'ledger') && !isDesktop) return 'sell';
    return view;
  });

  // First-session Score explainer — dismissable, one-time, persists.
  let scoreExplainerDismissed = $state((() => store.getSetting('score-explainer-dismissed') === '1')());
  function dismissScoreExplainer(): void {
    scoreExplainerDismissed = true;
    void store.setSetting('score-explainer-dismissed', '1');
  }

  // First-session sell onboarding — dismissable, one-time, persists (same
  // pattern as the score explainer; keeps the keep-copies nudge from stacking
  // on top of it on a fresh install).
  let sellOnboardingDismissed = $state((() => store.getSetting('sell-onboarding-dismissed') === '1')());
  function dismissSellOnboarding(): void {
    sellOnboardingDismissed = true;
    void store.setSetting('sell-onboarding-dismissed', '1');
  }

  // Keep-copies nudge — permanent one-time dismissal (education, not a
  // recurring nag).
  let keepCopiesNudgeDismissed = $state((() => store.getSetting('keep-copies-nudge-dismissed') === '1')());
  function dismissKeepCopiesNudge(): void {
    keepCopiesNudgeDismissed = true;
    void store.setSetting('keep-copies-nudge-dismissed', '1');
  }

  // Tray hint banner — set by the Rust tray-hint event when the user closes the
  // window while the tray still exists. Once-ever, persisted when SHOWN (not on
  // dismiss); the Rust side caps within-session duplicates with an AtomicBool.
  let trayHint = $state(false);

  // Preset pills above the rec cards. Each preset is one click that
  // updates the filter state to a known-useful combination. Currently
  // selected preset is tracked so the pill can show as active. Custom
  // edits null out the selection (you're no longer "on" a preset).
  let activePreset = $state<string | null>('default');
  // PRESETS itself, plus the pure lookup/matching logic, live in
  // lib/presets.ts. `columns` is the ordered visible-column list;
  // missing = all columns (Default).
  let visibleColumns = $derived<string[] | null>(activePreset ? PRESETS[activePreset]?.columns ?? null : null);
  // A preset's optional default sort, handed to ResultsTable. Stable object
  // identity per preset → switching presets re-applies it; header clicks don't.
  // Spread a fresh object so the derived's identity changes whenever it
  // recomputes — re-selecting a preset then re-applies its sort.
  let presetSort = $derived(
    activePreset && PRESETS[activePreset]?.defaultSort
      ? { ...PRESETS[activePreset].defaultSort }
      : null,
  );
  // The filter cascade's inputs, bundled for lib/filter-engine.ts — the
  // hand-set sliders/chips plus whatever the active preset restricts
  // (vault-only, ducats-only, min-volume, min-median).
  // Tradeable copies per slug|refinement from the latest scan — the My orders
  // panel's "you list ×5 but own 2" / "not owned" checks. Null without a scan
  // so those checks stay silent rather than calling every listing a ghost.
  let ownedQtyForOrders = $derived.by((): Map<string, number> | null => {
    if (!resolved.owned.size) return null;
    const m = new Map<string, number>();
    for (const rec of resolved.owned.values()) {
      m.set(`${rec.slug}|${rec.subtype ?? ''}`, Math.max(0, rec.count - (rec.leveled ?? 0)));
    }
    return m;
  });

  let filterState = $derived<FilterState>({
    minPrice, minOwned, typeFilter, hideAtLvl, activeTags,
    vaultOnly: !!PRESETS[activePreset]?.vaultOnly,
    ducatsOnly: !!PRESETS[activePreset]?.ducatsOnly,
    minVol: PRESETS[activePreset]?.minVol ?? 0,
    minMedian: PRESETS[activePreset]?.minMedian ?? 0,
    typesAny: PRESETS[activePreset]?.typesAny ?? [],
    sparesOnly: !!PRESETS[activePreset]?.sparesOnly,
  });
  function applyPreset(name: string): void {
    const values = presetFilterValues(name);
    if (!values) return;
    minPrice = values.minPrice;
    hideAtLvl = values.hideAtLvl;
    typeFilter = values.typeFilter;
    activeTags = values.activeTags;
    activePreset = name;
  }
  $effect(() => {
    // Depend ONLY on the filter primitives that define a preset (the void reads
    // below). Read/write activePreset inside untrack() so nulling the selection
    // when the user hand-edits a filter can't re-trigger this effect — the old
    // version read AND wrote activePreset in the same body, which re-fired it
    // (flagged in the audit).
    void minPrice; void minOwned; void hideAtLvl; void typeFilter; void activeTags.size;
    untrack(() => {
      if (activePreset === null) return;
      if (!presetStillMatches(activePreset, { minPrice, hideAtLvl, typeFilter, activeTags })) {
        activePreset = null;
      }
    });
  });

  // Best available market snapshot for this runtime. Desktop: the app-data
  // cache (last known-good from the live server) beats the compile-time bundled
  // floor; browser: loadCachedMarket() is a no-op null, so this collapses to the
  // exact same one same-origin fetch the hosted app has always done. Can throw
  // (loadMarket rejects on a missing snapshot) — callers keep their try/catch.
  async function loadBestMarket(): Promise<Market> {
    const cached = await transport.loadCachedMarket();
    return cached ?? (await loadMarket());
  }

  // Desktop-only: after the bundled/cached copy is on screen, top it up from
  // tennoworth.app in the background and swap in a strictly-newer snapshot. The
  // Rust command swallows offline/timeout/HTTP failures (returns updated:false),
  // so this never blocks or breaks boot — a failure just leaves the current copy.
  async function refreshMarketInBackground(): Promise<void> {
    try {
      const res = await transport.refreshMarket();
      if (!res.updated || !res.market) return;
      const cur = market?.updated_at ? Date.parse(market.updated_at) : NaN;
      const next = res.market.updated_at ? Date.parse(res.market.updated_at) : NaN;
      // Swap when we have nothing yet, the current stamp is unusable, or the
      // fetched snapshot is strictly newer (guards a server rollback serving an
      // older snapshot than the cache we already show).
      if (!market || !Number.isFinite(cur) || (Number.isFinite(next) && next > cur)) {
        market = res.market;
        marketLoadError = null;
      }
    } catch (e) {
      console.error('market refresh failed', e);
    }
  }

  // Restore the last snapshot exactly once after mount. Using onMount (not
  // $effect) is critical: $effect tracks any state read inside its body as
  // a dependency, so writing `resolved` here and then reading it via
  // recomputeResults caused an infinite re-run loop.
  onMount(async () => {
    // Desktop mode: a best-effort `health` invoke confirms wfm-core is linked
    // and records the platform for display; failure is non-fatal (the dashboard
    // still works). The hosted site is informational — it has no account
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
      // no unlock. Best-effort — a failure just hides the Resume banner.
      try {
        pendingPlan = await transport.getPendingPlan();
      } catch (e) {
        console.error('desktop pending-plan check failed', e);
      }
      // Tray hint: when the window is closed to the tray, surface the once-ever
      // "still running in the tray" banner. Listeners are best-effort in the
      // hosted build (no event API), so this is a no-op there.
      listenForTauriEvent(TRAY_HINT_EVENT, () => {
        if (store.getSetting('tray-toast-seen') !== '1') {
          trayHint = true;
          void store.setSetting('tray-toast-seen', '1');
        }
      });
    }

    // Desktop only: restore the last inventory snapshot. The hosted site is
    // informational — it never holds an inventory, so it stays on the landing.
    if (isDesktop) {
      const snap = await store.loadSnapshot();
      if (snap) {
        try {
          inventoryName = snap.invName;
          lastUpdated = snap.ts;
          resolved = { owned: snap.owned, unresolved: {} };
          if (!market) {
            try {
              market = await loadBestMarket();
            } catch (e) {
              // We already have the restored inventory in hand — a failed price
              // refresh must NOT throw us back to cold-start and hide the user's
              // data. Render from the snapshot; flag prices unavailable.
              console.error(e);
              marketLoadError = 'Couldn’t load the price snapshot — you may be offline. Your saved inventory is shown below; prices and rankings will be unavailable until it loads. Reload to retry.';
            }
          }
          // No explicit recompute: the results $effect tracks resolved/market and
          // flushes before paint — the old call here just computed everything twice.
          phase = 'done';
        } catch (e) {
          console.error(e);
          error = e.message || String(e);
          phase = 'error';
        }
      }
    }

    // Cold landing (no saved inventory): preload the snapshot so the no-install
    // MarketBrowser has data to show. Best-effort — a failure just hides the
    // browser; the install steps below it still work.
    if (phase === 'idle' && !market) {
      try {
        market = await loadBestMarket();
      } catch (e) {
        console.error(e);
      }
    }

    // Desktop only: keep the bundled/cached snapshot fresh from tennoworth.app.
    // Fire-and-forget AFTER the copy above is on screen — the refresh must never
    // block app start (the hosted build skips this: it already gets fresh data
    // same-origin from the box). Runs even if the loads above failed, so an
    // offline-first-launch can still recover once the network returns.
    if (isDesktop) void refreshMarketInBackground();
  });

  function handleClear() {
    void store.clearSnapshot();
    inventoryName = null;
    lastUpdated = null;
    resolved = { owned: new Map(), unresolved: {} };
    deltas = new Map();
    previousOwned = null;
    results = [];
    tableView = { rows: [], active: false };
    phase = 'idle';
  }

  // The single inventory path: a memory scan of the running game. (The hosted
  // site takes no files, and the desktop app scans directly — there is no
  // file-drop path.) The scan command already records a source='memory'
  // snapshot inside scan_inventory, so nothing is recorded here.
  async function handleInventory({ name, data }) {
    inventoryName = name;
    phase = 'loading';
    error = null;
    pullError = null;
    try {
      if (!catalogs || !market) {
        [catalogs, market] = await Promise.all([
          catalogs ?? loadCatalogs(),
          market ?? loadBestMarket(),
        ]);
      }

      const keptLvls = extractKeptLvls(data);  // /Lotus/... → max lvl in Upgrades
      const owned = new Map();
      const unresolved = {};
      let flatCount = 0;
      for (const { category: invCat, path, count, xp } of flattenInventory(data)) {
        flatCount++;
        const { name: itemName, slug, category: itemType, subtype } =
          resolvePath(path, catalogs, market);
        if (!slug) {
          unresolved[invCat] = (unresolved[invCat] || 0) + 1;
          continue;
        }
        const type = itemType || invCat;
        const key = `${slug}|${subtype ?? ''}`;
        const keptLvl = keptLvls.get(path);
        const rec = owned.get(key) || {
          count: 0, name: itemName, type, slug, subtype: subtype ?? null,
          kept_lvl: null, leveled: 0,
        };
        rec.count += count;
        // XP > 0 on an instance category entry means DE has flagged that
        // specific copy untradeable in-game (only unranked gear can trade).
        // Stack categories never carry XP, so this stays 0 for them.
        if (xp > 0) rec.leveled += count;
        // Carry forward the highest kept lvl across any inventory path
        // that resolved to the same slug+subtype (rare for mods — one path
        // per slug — but harmless for the relic refinement case).
        if (typeof keptLvl === 'number' && (rec.kept_lvl === null || keptLvl > rec.kept_lvl)) {
          rec.kept_lvl = keptLvl;
        }
        owned.set(key, rec);
      }
      // Nothing resolved to a tradeable item — the scan returned an inventory
      // with no recognizable tradeable content. Surface it as a pull error so
      // the user sees why the scan didn't produce a sell list, and don't
      // overwrite their snapshot.
      if (owned.size === 0) {
        pullError = flatCount === 0
          ? "The scan didn't find a recognizable inventory in the game's memory. Make sure Warframe is running and you're past the login screen, then try again."
          : 'The scan found items, but nothing in them is tradeable on warframe.market (quest items, resources, and brand-new content have no listings).';
        inventoryName = null;
        phase = 'idle';
        return;
      }
      // Diff against the previously-saved snapshot before overwriting it.
      const previous = await store.loadSnapshot();
      deltas = diffOwned(previous?.owned, owned);
      previousOwned = previous?.owned ?? null;
      resolved = { owned, unresolved };
      await store.saveSnapshot({ invName: name, owned });
      lastUpdated = Date.now();

      // No explicit recompute: the results $effect below tracks resolved +
      // market and flushes before paint. The call that was here computed the
      // identical array a second time — the same fix the restore path above
      // already got. Its only distinct case, "market not loaded yet", is
      // covered by that effect's own `market` guard.
      phase = 'done';
    } catch (e) {
      console.error(e);
      error = e.message || String(e);
      phase = 'error';
    }
  }

  // Re-derive results whenever any filter input or the owned set changes.
  // We deliberately read the filter state inside the effect (so they're
  // tracked) but write only to `results`, which the effect doesn't read —
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
    filterState; reserveCopies;                   // track filter changes
    if (resolved.owned.size && market) {          // track owned + market readiness
      results = computeFilteredResults(resolved.owned, market, filterState, reserveCopies);
    }
  });

  // Set-completion recommendations. Pure derivation from owned × market —
  // see lib/set-recos.js for the three reco kinds (near-complete /
  // complete-with-extras / extras). Computed lazily; cheap (one walk per
  // set in the catalog).
  let setRecos = $derived.by(() => {
    if (!resolved.owned.size || !market?.set_to_parts) return [];
    return deriveSetRecos(resolved.owned, market);
  });
  let setRecosExpanded = $state(false);

  // Baro Ki'Teer schedule, baked into market.json at build time (mirrors
  // relic_rewards / vault_status). No runtime warframestat fetch — that
  // broke the resolver-only rule and vanished during warframestat
  // outages. Null until market loads, or when the bake came back empty.
  // Footer stamp: the snapshot's own timestamp, in UTC so it matches the
  // scraper's log lines.
  let snapshotStamp = $derived.by(() => {
    const t = Date.parse(market?.updated_at ?? '');
    if (!Number.isFinite(t)) return '';
    return new Date(t).toISOString().slice(0, 16).replace('T', ' ') + ' UTC';
  });

  let voidTrader = $derived.by(() => {
    const b = market?.baro;
    if (!b) return null;
    return { ...b, location: baroLocation(b.location) };
  });

  // Total ducats across the user's currently-sellable inventory.
  // Only count rows that resolved to a market entry with ducats > 0;
  // skip relic refinements (subtype set) since those aren't a ducat
  // trade. Cap presented as `count_owned × ducats`.
  let ducatStats = $derived.by(() => {
    if (!resolved.owned.size || !market) return { count: 0, total: 0 };
    let count = 0, total = 0;
    for (const rec of resolved.owned.values()) {
      if (rec.subtype) continue;
      const m = market.items?.[rec.slug];
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
  let showBaroCard = $derived(voidTrader != null && ducatStats.total >= 500);

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
  // these recompute on load / view change — the same non-ticking model as the
  // Baro card, which is fine for a "next reset in ~Xh" reminder.
  let routinesState = $derived.by(() => {
    const now = Date.now();
    const d = new Date(now);
    const nextDaily = Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate() + 1);
    const daysToMon = ((8 - d.getUTCDay()) % 7) || 7; // 0=Sun..6=Sat → next Mon
    const nextWeekly = Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate() + daysToMon);
    return { dailyMs: nextDaily - now, weeklyMs: nextWeekly - now };
  });

  // Relic planner — top 3 owned (Intact) relics by expected-plat-per-crack.
  let relicPlan = $derived.by(() => {
    if (!resolved.owned.size || !market?.relic_rewards) return [];
    return deriveRelicPlan(resolved.owned, market, Infinity);
  });
  const RELIC_PREVIEW = 6;
  let relicShowAll = $state(false);
  let relicVisible = $derived(relicShowAll ? relicPlan : relicPlan.slice(0, RELIC_PREVIEW));

  // Routine checklist is collapsed by default — the clocks carry the daily
  // urgency, the three routine cards are a long-read. Not persisted
  // (collapsed-by-default is the intent).
  let routineChecklistOpen = $state(false);

  // Available tags = every tag that appears on a row surviving the OTHER
  // filters (price/owned/type/kept), with its live count. Empty chips
  // (count 0) are still rendered (strikethrough) so the user can see what
  // categories exist in their inventory rather than wondering where they
  // went. Sorted by count desc, then alphabetical.
  let availableTags = $derived.by(() => {
    if (!resolved.owned.size || !market) return [];
    return computeAvailableTags(resolved.owned, market, filterState);
  });

  // Auto-derived options for the type dropdown: every category that has at
  // least one sellable item. Built off owned + market (not `results`), so it
  // doesn't shrink when the user narrows by min-price / min-owned.
  let availableTypes = $derived.by(() => {
    if (!resolved.owned.size || !market) return [];
    const set = new Set();
    for (const rec of resolved.owned.values()) {
      if (lookup(market, rec.slug)) set.add(rec.type || 'Unknown');
    }
    return [...set].sort();
  });

  // Total + per-category breakdown of paths no catalog could price-match.
  // The number itself is reassurance ("the app saw these and skipped them,
  // your prime junk isn't missing"), the breakdown is hover detail.

  let unresolvedCount = $derived(
    Object.values(resolved.unresolved).reduce((s, n) => s + n, 0)
  );

  // Every owned row with ANY market match — no preset/filter applied. The
  // sidebar Sell badge pins to this so it stays stable while filters change.
  let sellableCount = $derived.by(() => {
    if (!market) return 0;
    let n = 0;
    // Count an item only if at least one copy is actually listable after the
    // keep-copies reserve and leveled (untradeable) copies — otherwise the
    // headline tile would claim items are sellable that the reserve holds back,
    // contradicting the dimmed rows and zeroed potential.
    for (const rec of resolved.owned.values())
      if (lookup(market, rec.slug) && sellableQty(rec.count, reserveCopies, rec.leveled ?? 0) > 0) n += 1;
    return n;
  });
  let unresolvedSummary = $derived(
    Object.entries(resolved.unresolved)
      .map(([k, v]) => `${k}: ${v}`)
      .join(', ')
  );

  function ago(ts) {
    if (!ts) return null;
    // Clamp at 0 — a cron runner with skewed clock can produce
    // `updated_at` in the future, which used to render "-120 min ago".
    const minutes = Math.max(0, Math.round((Date.now() - new Date(ts)) / 60000));
    if (minutes < 1) return 'just now';
    if (minutes < 60) return `${minutes} min ago`;
    if (minutes < 60 * 24) return `${Math.round(minutes / 60)} h ago`;
    return `${Math.round(minutes / 1440)} d ago`;
  }

  let marketStaleness = $derived(ago(market?.updated_at));
  let inventoryStaleness = $derived(ago(lastUpdated));
  // Same buckets as the market dot, on the inventory's own clock: a scan is
  // "fresh" for a day (inventories move slower than the order book).
  let inventoryFreshness = $derived.by(() => {
    if (!lastUpdated) return 'unknown';
    const h = (Date.now() - lastUpdated) / 3.6e6;
    if (h <= 24) return 'fresh';
    if (h <= 24 * 7) return 'aging';
    return 'stale';
  });

  // A vendor surface (Baro schedule, relic drops, vault status, set maps) is
  // refreshed on a full scrape but NOT on a CSV-only rebuild — so it can lag
  // the price `updated_at`. Flag a surface only when it's meaningfully old
  // (these rotate on the order of days/weeks), so we don't nag on a healthy
  // snapshot where every stamp matches.
  function surfaceAge(key) {
    const ts = market?.surface_fetched_at?.[key];
    if (!ts) return null;
    const days = (Date.now() - new Date(ts).getTime()) / 8.64e7;
    return days >= 3 ? ago(ts) : null;
  }
  let baroSurfaceAge = $derived(surfaceAge('baro'));
  let relicSurfaceAge = $derived(surfaceAge('relic_rewards'));
  let setSurfaceAge = $derived(surfaceAge('set_to_parts'));

  // Coarse freshness bucket for the small status dot next to "market Xh ago".
  // The scrape cron runs every 2h, so a healthy snapshot is under 3h old —
  // "fresh" means exactly that. Calling a 5h-old book "fresh" during an
  // event-week price spike would be generous to the point of misleading.
  let marketFreshness = $derived.by(() => {
    if (!market?.updated_at) return 'unknown';
    const h = (Date.now() - new Date(market.updated_at)) / 3.6e6;
    if (h <= 3) return 'fresh';
    if (h <= 24) return 'aging';
    return 'stale';
  });

  // Total theoretical plat across visible results — for the stats strip.
  let totalPotential = $derived(
    results.reduce((s, r) => s + r.potential_plat, 0)
  );

  // Since-last-scan deltas for the Sell summary cells. The previous snapshot is
  // pushed through the SAME filter cascade as the live one, so "Sellable ▲12"
  // means twelve more rows under the current preset/filters — not a different
  // definition of sellable. One extra computeResults per filter change; the
  // cascade costs ~0.1 ms on a 2k-item inventory (measured 2026-08-01).
  let prevSummary = $derived.by(() => {
    if (!previousOwned || !market) return null;
    const rows = computeFilteredResults(previousOwned, market, filterState, reserveCopies);
    return {
      owned: previousOwned.size,
      sellable: rows.filter((r) => r.sellable > 0).length,
      potential: rows.reduce((s, r) => s + r.potential_plat, 0),
    };
  });
  // Row-level "what changed": keys new since the last scan, keys gone, keys
  // whose count moved. `deltas` only covers keys present now (diffOwned walks
  // the current map), so removals come from the previous map directly.
  let sinceScan = $derived.by(() => {
    if (!previousOwned) return null;
    let added = 0, removed = 0, changed = 0;
    for (const [key, d] of deltas) {
      if (!previousOwned.has(key)) added += 1;
      else if (d !== 0) changed += 1;
    }
    for (const key of previousOwned.keys()) if (!resolved.owned.has(key)) removed += 1;
    return { added, removed, changed };
  });

  // Live-orders summary reported up by MyOrdersPanel once it has fetched
  // (desktop only; null until the user has opened My orders this session, so
  // the strip never triggers an auth prompt on its own). Feeds the strip's
  // "N orders to fix →" cell and the Sell summary's Listed / Needs fixing.
  let ordersSummary = $state<{ live: number; issues: number } | null>(null);
  let ordersToFix = $derived((ordersSummary?.issues ?? 0) + (pendingPlan && pendingRemaining > 0 ? pendingRemaining : 0));

  // WFM session state for the strip's end cell. Re-probed whenever the session
  // epoch bumps (login/unlock) so the label follows the dialogs.
  let wfmStatus = $state<{ logged_in: boolean; unlocked: boolean } | null>(null);
  $effect(() => {
    if (!isDesktop) return;
    void sessionEpoch;
    desktopWfmStatus().then((s) => { wfmStatus = s; }).catch(() => { wfmStatus = null; });
  });
  let wfmLabel = $derived(
    !wfmStatus ? '—' : wfmStatus.unlocked ? 'session live' : wfmStatus.logged_in ? 'locked' : 'logged out',
  );

  // Friendly diagnosis of WHY the table is empty so we don't just shrug.
  let emptyReason = $derived.by(() =>
    computeEmptyReason(resolved.owned, market, filterState, results.length, activePreset)
  );

  // Scan is the only inventory source — the refresh pop is a single action.
  let refreshOpen = $state(false);
  async function refreshFromGame() {
    refreshOpen = false;
    await pullInventory();
  }
  $effect(() => {
    if (!refreshOpen) return;
    const handler = (e) => {
      const t = e.target;
      if (!t?.closest?.('.refresh-pop, .refresh-trigger')) refreshOpen = false;
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
  let exportImportRef = $state();
  async function handleImported({ invName, ts, ownedMap }) {
    inventoryName = invName;
    lastUpdated = ts;
    const previous = await store.loadSnapshot();
    deltas = diffOwned(previous?.owned, ownedMap);
    previousOwned = previous?.owned ?? null;
    resolved = { owned: ownedMap, unresolved: {} };
    if (!market) market = await loadBestMarket();
    await store.saveSnapshot({ invName: inventoryName, owned: ownedMap });
    phase = 'done';
  }

  // ---- Pending-plan recovery ----
  let pendingPlan = $state(null);          // {plan_id, started_at, items[]} | null
  let resumePhase = $state('idle');        // idle | running | done | error
  let resumeError = $state(null);
  let resumeResults = $state([]);
  // Platform the desktop session reports (from /health), for display.
  let companionPlatform = $state(null);

  let pendingRemaining = $derived(
    pendingPlan?.items?.filter((i) => i.status === 'pending').length ?? 0
  );
  let pendingDone = $derived(
    pendingPlan?.items?.filter((i) => i.status === 'ok').length ?? 0
  );

  async function doResume() {
    if (!isDesktop) return;
    resumePhase = 'running';
    resumeError = null;
    try {
      const resp = await transport.resumePendingPlan();
      resumeResults = resp?.results ?? [];
      resumePhase = 'done';
      pendingPlan = null;
    } catch (e) {
      // Desktop locked-session rejection: keep the banner (the plan is still
      // pending) and open the matching auth dialog — Resume works after that.
      if (e instanceof DesktopCmdError && (e.code === 'needs_login' || e.code === 'needs_unlock')) {
        resumePhase = 'idle';
        wfmAuthDialogsRef.open(e.code);
        return;
      }
      resumePhase = 'error';
      resumeError = e.message || String(e);
    }
  }

  async function doDiscard() {
    if (!isDesktop) return;
    try { await transport.discardPendingPlan(); } catch { /* ignore */ }
    pendingPlan = null;
    resumePhase = 'idle';
    resumeResults = [];
  }

  let resumeOk = $derived(resumeResults.filter((r) => r.status === 'ok').length);
  let resumeErr = $derived(resumeResults.filter((r) => r.status !== 'ok').length);

  // Scan inventory straight from the running game and run it through the same
  // resolution pipeline — no file, no drag-in. Desktop only (the hosted site
  // is informational); the `scan_inventory` IPC command's rejection carries
  // the scanner's exact actionable message.
  let pullingInventory = $state(false);   // scan in flight
  let pullError = $state<string | null>(null);
  // Set only when opening a browser FAILED, so the user can copy the URL.
  let reportUrl = $state<string | null>(null);
  let reportingScan = $state(false);

  async function reportScanBroke(): Promise<void> {
    reportingScan = true;
    reportUrl = null;
    try {
      // A failed open is not a rejection — the command reports it so we can
      // fall back to a copyable link rather than a dead button.
      const r = await transport.reportScanIssue(pullError);
      if (!r.opened) reportUrl = r.url;
    } catch (e) {
      reportUrl = null;
      pullError = `${pullError}\n\nCouldn't build a report: ${(e && e.message) || String(e)}`;
    } finally {
      reportingScan = false;
    }
  }

  async function pullInventory() {
    if (pullingInventory || !isDesktop) return;
    pullingInventory = true;
    pullError = null;
    try {
      const data = await transport.fetchInventory();
      await handleInventory({
        name: 'inventory (from game)',
        data,
      });
    } catch (e) {
      pullError = e.message || String(e);
    } finally {
      pullingInventory = false;
    }
  }

  // ---- Desktop WFM auth (login / unlock dialogs) -----------------------
  // The dialogs themselves, their state, and the login/unlock calls live in
  // WfmAuthDialogs.svelte; App.svelte triggers them imperatively (three call
  // sites: the Sell CTA below, doResume's needs_login/needs_unlock rejection,
  // and ListingReviewModal's onauthrequired) via this ref, and decides what
  // 'list' means on unlock (open the review modal).
  let wfmAuthDialogsRef = $state();
  // Bumped on every successful login/unlock so auth-gated surfaces (the orders
  // panel) retry their fetch once the session is live — the "input once, never
  // again" path is silent via the keyring, and this is what notices it.
  let sessionEpoch = $state(0);
  function handleWfmUnlocked(next) {
    sessionEpoch += 1;
    if (next === 'list') listingOpen = true;
  }

  let listingOpen = $state(false);
  // null = the normal bulk "List N on WFM" behaviour (top-50 of the current
  // view). A picks-strip "List" click stages exactly that one row instead —
  // same modal, same price/qty editing, no second staging path.
  let reviewRowsOverride = $state(null);

  // The Sell CTA routes here. Desktop-only (listing needs wfm-core's session).
  // Checks the session first so the user meets the login/passphrase dialog
  // BEFORE staging 50 rows — the same codes also arrive lazily via the modal's
  // onauthrequired if the session state changed between staging and Send.
  //
  // `overrideRow` is set by a picks-strip "List" click to stage that single
  // row instead of the top-50 bulk view; unconditional so every call (bulk
  // included) resets any stale override from a previous picks click.
  async function openListingFlow(overrideRow = null) {
    if (!isDesktop) return;
    reviewRowsOverride = overrideRow ? [overrideRow] : null;
    try {
      const s = await desktopWfmStatus();
      if (s.unlocked) listingOpen = true;
      else wfmAuthDialogsRef.open(s.logged_in ? 'needs_unlock' : 'needs_login', 'list');
    } catch (e) {
      // Status probe failed (IPC fault) — open the modal anyway; Send will
      // surface the typed code and route to the right dialog.
      console.error('wfm auth status check failed', e);
      listingOpen = true;
    }
  }

</script>

{#if phase !== 'done'}
<main class="landing" data-testid={isDesktop ? 'desktop-mode' : undefined}>
  {@render statusStrip(false)}
  <!-- One lede line under the strip (the strip's descriptor already says what
       this is); the theme switcher rides its right end. The old pitch
       paragraph / hero is gone — search is the first control. -->
  <header class="landing-head">
    <p class="lede">
      {#if isDesktop}
        Scan your account and TennoWorth ranks <em>your</em> inventory by what to sell — until then, look anything up below.
      {:else}
        What's worth selling in Warframe right now — search any item, spot the movers, see what's vaulted. No install, no login.
      {/if}
    </p>
    <div class="landing-theme">
      <ThemeSwitcher {theme} />
    </div>
  </header>

  {@render generalBanners()}

  {#if phase === 'idle' || phase === 'loading'}
    {#if isDesktop}
      <!-- The scan CTA leads the desktop empty state (fresh install AND
           post-Clear): scanning is the app's whole point, so it must never
           sit below the fold of the market browser — that's how a user ends
           up back on the manual file path. -->
      <section class="upsell-lead desktop-hero">
        <h2>Get your personal sell list</h2>
        <p class="sub">
          With Warframe open and past the login screen, scan your account —
          TennoWorth ranks <em>your</em> inventory by what to sell right now.
        </p>
        <div class="desktop-scan-row">
          <button
            class="rp-primary"
            data-testid="desktop-scan"
            onclick={pullInventory}
            disabled={pullingInventory}
          >{pullingInventory ? 'Scanning game…' : 'Scan inventory'}</button>
        </div>
        <span class="trust">Reads the running game's memory only — nothing leaves your machine.</span>
      </section>
    {/if}

    {#if market}
      {#if isDesktop}
        <MarketBrowser {market} staleness={marketStaleness} freshness={marketFreshness} loadHistory={() => transport.loadHistory()} />
      {:else}
        <!-- Hosted: the browser hands the visitor off to the desktop app with
             the same rows completed (DesktopShowcase renders inside the
             browser's flow so it can use the browser's sample rows). -->
        <MarketBrowser {market} staleness={marketStaleness} freshness={marketFreshness} loadHistory={() => transport.loadHistory()} handoff={handoffPanel} />
      {/if}
    {:else if !isDesktop}
      <DesktopShowcase />
    {/if}
  {/if}

  {#if phase === 'error' && isDesktop}
    <div class="card error">
      Error: {error}
      <div style="margin-top:10px">
        <button class="rp-primary" data-testid="desktop-scan" onclick={pullInventory} disabled={pullingInventory}>
          {pullingInventory ? 'Scanning game…' : 'Scan inventory'}
        </button>
      </div>
    </div>
  {/if}

  {@render faqContent()}

  <footer class="sitefoot">
    <span class="grow">TennoWorth is a fan project, not affiliated with Digital Extremes or warframe.market. Open source · MIT · data from warframe.market and warframestat.us.</span>
    {#if market?.updated_at}<span title="When the market snapshot was taken">Snapshot {snapshotStamp}</span>{/if}
    <a href="https://github.com/tennoworth/tennoworth" target="_blank" rel="noopener noreferrer">GitHub</a>
    <a href="#trust">Trust &amp; safety</a>
    <span class="ver" title="build {APP_COMMIT}">{APP_COMMIT}</span>
  </footer>
</main>
{:else}
<div class="shell">
  {@render statusStrip(true)}

  <aside class="sidebar">
    <nav>
      <div class="nav-group">
        <div class="nav-label">Trade</div>
        <button type="button" class="nav-item" class:active={effectiveView === 'sell'} onclick={() => setView('sell')}>
          <span>Sell</span>
          <!-- Pinned to the unfiltered sellable count: with a narrow preset
               active (Vaulted on a no-vaulted inventory), a filter-driven
               "Sell 0" reads as "your inventory got wiped". -->
          <span class="badge">{sellableCount}</span>
        </button>
        {#if setRecos.length > 0}
          <button type="button" class="nav-item" class:active={effectiveView === 'sets'} onclick={() => setView('sets')}>
            <span>Set picks</span>
            <span class="badge">{setRecos.length}</span>
          </button>
        {/if}
        {#if relicPlan.length > 0}
          <button type="button" class="nav-item" class:active={effectiveView === 'relics'} onclick={() => setView('relics')}>
            <span>Relics</span>
            <span class="badge">{relicPlan.length}</span>
          </button>
        {/if}
        {#if showBaroCard}
          <button type="button" class="nav-item baro-nav" class:active={effectiveView === 'baro'} onclick={() => setView('baro')}>
            <span>Baro</span>
            {#if baroState?.phase === 'here'}<span class="badge here">here</span>{/if}
          </button>
        {/if}
        <button type="button" class="nav-item" class:active={effectiveView === 'routines'} onclick={() => setView('routines')}>
          <span>Routines</span>
        </button>
      </div>

      {#if isDesktop}
      <div class="nav-group">
        <div class="nav-label">Manage</div>
        <button type="button" class="nav-item" class:active={effectiveView === 'orders'} onclick={() => setView('orders')}>
          <span>My orders</span>
          {#if pendingPlan && pendingRemaining > 0}<span class="badge warn">{pendingRemaining}</span>{/if}
        </button>
        <button type="button" class="nav-item" class:active={effectiveView === 'watches'} onclick={() => setView('watches')}>
          <span>Price watches</span>
        </button>
        <button type="button" class="nav-item" class:active={effectiveView === 'ledger'} onclick={() => setView('ledger')}>
          <span>Ledger</span>
        </button>
      </div>
      {/if}

      <div class="nav-group">
        <div class="nav-label">Library</div>
        <button type="button" class="nav-item" class:active={effectiveView === 'install'} onclick={() => setView('install')}>
          <span>FAQ</span>
        </button>
      </div>
    </nav>

    <div class="sfoot">
      <ThemeSwitcher {theme} />
      <div class="ver" title="build {APP_COMMIT}">Windows + Linux · no Overwolf · {APP_COMMIT}</div>
    </div>
  </aside>

  <main class="workspace">

    {@render generalBanners()}

    {#if effectiveView === 'sell'}
      <SellPane
        bind:minPrice bind:minOwned bind:typeFilter bind:hideAtLvl bind:activeTags
        bind:tableView
        {resolved} {results} {deltas} {totalPotential}
        {prevSummary} {sinceScan} {ordersSummary}
        {marketFreshness} {marketStaleness} {marketLoadError}
        {listableRows} {availableTags} {availableTypes}
        {visibleColumns} {presetSort} {emptyReason}
        {activePreset} {reserveCopies} {filtersOpen} {scoreExplainerDismissed}
        {sellOnboardingDismissed} {keepCopiesNudgeDismissed}
        {isDesktop}
        {applyPreset} {setReserveCopies} {toggleFiltersOpen} {dismissScoreExplainer}
        {dismissSellOnboarding} {dismissKeepCopiesNudge}
        {openListingFlow}
        {pendingBanner}
      />
    {:else if effectiveView === 'sets'}
      <section class="view-header">
        <h2>Set picks</h2>
        <p class="lede">
          Inventory cross-referenced against {Object.keys(market?.set_to_parts ?? {}).length}
          prime sets. Ranked by net plat.
          {#if setSurfaceAge}
            <span class="muted">· ⚠ set/vault data {setSurfaceAge}</span>
          {/if}
        </p>
      </section>
      {#if setRecos.length > 0}
        <section class="card set-recos">
          {#each setRecos as r (r.set_slug)}
            <div class="reco row">
              <div class="reco-body">
                <div class="reco-title">
                  <strong class="reco-verb">
                    {#if r.kind === 'near-complete'}Complete{:else if r.kind === 'complete-with-extras'}List{:else}List{/if}
                  </strong>
                  <a
                    href={wfmItemUrl(r.set_slug)}
                    target="_blank"
                    rel="noopener noreferrer"
                  >{r.set_name}</a>
                  <span class="reco-net-inline">+{r.net_plat}p</span>
                  <span class="kind kind-{r.kind}">
                    {#if r.kind === 'near-complete'}
                      own {r.parts.filter((p) => p.count > 0).length}/{r.parts.length}
                    {:else if r.kind === 'complete-with-extras'}
                      {r.extras} spare{r.extras === 1 ? '' : 's'} + full set
                    {:else}
                      {r.extras} duplicate{r.extras === 1 ? '' : 's'}
                    {/if}
                  </span>
                  {#if r.set_vol !== undefined && (r.kind === 'near-complete' || r.kind === 'complete-with-extras')}
                    {#if r.set_vol < 1}
                      <span class="set-liq cold" title="The assembled set has traded under 1×/48h — a flip may sit unsold for a while.">set rarely trades</span>
                    {:else if r.set_vol < 5}
                      <span class="set-liq thin" title="Thin set volume — expect to wait for a buyer before you recoup the plat.">thin · {r.set_vol}/48h</span>
                    {:else}
                      <span class="set-liq moving" title="Healthy set volume.">{r.set_vol}/48h</span>
                    {/if}
                  {/if}
                </div>
                <p class="reco-detail muted">
                  {#if r.kind === 'near-complete'}
                    {@const ownedCount = r.parts.filter((p) => p.count > 0).length}
                    Buy {r.missing.map((m) => m.name).join(' + ')} for
                    <strong class="bad-text">{r.missing_cost}p</strong>, sell as a set for
                    <strong class="good-text">{r.set_low_sell}p</strong>
                    (vs. {r.parts_low_sell}p selling the {ownedCount} part{ownedCount === 1 ? '' : 's'} individually).
                  {:else if r.kind === 'complete-with-extras'}
                    You hold a full set plus {r.extras} spare blueprint{r.extras === 1 ? '' : 's'}.
                    List the extras at <strong>{r.extras_plat}p</strong>.
                  {:else}
                    Duplicates of partial-set parts. List the {r.extras} spare {r.extras === 1 ? 'copy' : 'copies'}:
                    <strong>{r.extras_plat}p</strong>.
                  {/if}
                </p>
              </div>
            </div>
          {/each}
        </section>
      {:else}
        <div class="card empty">
          <div>
            <strong>No set recommendations.</strong>
            <p class="muted">You don't currently own enough prime parts to surface near-complete sets or spare-blueprint plays.</p>
          </div>
        </div>
      {/if}

    {:else if effectiveView === 'relics'}
      <section class="view-header">
        <h2>Relic planner</h2>
        <p class="lede">
          {#if relicPlan.length > relicVisible.length}
            Top {relicVisible.length} of {relicPlan.length} relics you own, ranked by expected plat per crack (Intact state).
          {:else}
            Your {relicPlan.length} relic{relicPlan.length === 1 ? '' : 's'} ranked by expected plat per crack (Intact state).
          {/if}
          {#if relicSurfaceAge}
            <span class="muted">· ⚠ drop-table data {relicSurfaceAge}</span>
          {/if}
        </p>
      </section>
      {#if relicPlan.length > 0}
        <section class="card relic-planner">
          <div class="relic-grid">
            {#each relicVisible as p (p.relic_slug)}
              <div class="relic-card">
                <div class="relic-title">
                  <strong class="reco-verb">Crack</strong>
                  <a
                    href={wfmItemUrl(p.relic_slug)}
                    target="_blank"
                    rel="noopener noreferrer"
                  >{p.relic_name}</a>
                  <span class="muted small">×{p.owned}</span>
                </div>
                <div class="relic-epp">
                  {p.epp.toFixed(1)}<span class="unit">p / crack</span>
                </div>
                <div class="relic-meta">
                  <span class:bad-text={p.moving_count < p.total_rewards / 2}>
                    {p.moving_count}/{p.total_rewards} rewards moving
                  </span>
                  <span class="muted">·</span>
                  <span title="If you cracked every one you own.">
                    {p.epp_owned.toFixed(0)}p total
                  </span>
                  {#if p.sell_now > 0}
                    <span class="muted">·</span>
                    <span
                      class:bad-text={p.sell_now > p.epp}
                      title="What this relic clears at sold intact on WFM, no cracking. When this beats the crack EV, selling wins."
                    >or sell: {p.sell_now.toFixed(0)}p ea</span>
                  {/if}
                </div>
                <details class="relic-rewards">
                  <summary>top drops</summary>
                  <ul>
                    {#each p.rewards.slice(0, 4) as r (r.slug)}
                      <li>
                        <span class="rarity rarity-{r.rarity.toLowerCase()}">{r.rarity[0]}</span>
                        <span class="reward-name">{r.name}</span>
                        <span class="muted small">{r.chance.toFixed(0)}%</span>
                        <span class={r.low_sell > 0 ? '' : 'muted'}>{r.low_sell || '—'}p</span>
                      </li>
                    {/each}
                  </ul>
                </details>
              </div>
            {/each}
          </div>
          {#if relicPlan.length > RELIC_PREVIEW}
            <div class="relic-more">
              <button class="ghost" onclick={() => (relicShowAll = !relicShowAll)}>
                {relicShowAll ? 'Show fewer' : `Show ${relicPlan.length - RELIC_PREVIEW} more`}
              </button>
            </div>
          {/if}
        </section>
      {:else}
        <div class="card empty">
          <div>
            <strong>No relics in your inventory.</strong>
            <p class="muted">Once you pick up relics, this planner ranks them by expected plat per crack.</p>
          </div>
        </div>
      {/if}

    {:else if effectiveView === 'baro'}
      <section class="view-header">
        <h2>Baro Ki'Teer</h2>
        <p class="lede">
          {#if baroState?.phase === 'here'}
            Here at {voidTrader.location} — leaves in {humanWindow(baroState.windowMs)}.
          {:else if baroState?.phase === 'incoming'}
            Arrives in {humanWindow(baroState.windowMs)} at {voidTrader.location}.
          {:else}
            Next visit at {voidTrader.location}.
          {/if}
          {#if baroSurfaceAge}
            <span class="muted">· ⚠ schedule data {baroSurfaceAge} — may be a rotation behind</span>
          {/if}
        </p>
      </section>
      <section class="card baro-card" class:here={baroState?.phase === 'here'}>
        <div class="row">
          <div class="src">
            <span class="baro-icon" aria-hidden="true">⌬</span>
            <div class="baro-body">
              <p class="baro-detail">
                You hold <strong>{ducatStats.total.toLocaleString()}<span class="unit">d</span></strong>
                across <strong>{ducatStats.count.toLocaleString()}</strong>
                ducat-earning {ducatStats.count === 1 ? 'item' : 'items'}.
                {#if baroState?.phase === 'here'}
                  Spend them on Baro's offerings — open the <strong>Ducats</strong>
                  preset to see what's worth dumping.
                {:else}
                  Earmark these for Baro using the <strong>Ducats</strong> preset.
                {/if}
              </p>
            </div>
          </div>
          <div class="row gap-sm">
            <button onclick={() => { setView('sell'); applyPreset('ducats'); }}>Open Ducats preset →</button>
          </div>
        </div>
      </section>

    {:else if effectiveView === 'routines'}
      <section class="view-header">
        <h2>Profit routines</h2>
        <p class="lede">
          Daily and weekly habits that compound — including the Endo sources that fund the
          buy-unranked → max → resell flip. Countdowns are live; what you've already claimed
          isn't tracked (your inventory + the market snapshot can't see account state).
        </p>
      </section>

      <section class="card routine">
        <div class="routine-clocks">
          <div class="clock">
            <span class="clock-label">Daily reset</span>
            <strong class="clock-val">{humanWindow(routinesState.dailyMs)}</strong>
            <span class="clock-sub">00:00 UTC</span>
          </div>
          <div class="clock">
            <span class="clock-label">Weekly reset</span>
            <strong class="clock-val">{humanWindow(routinesState.weeklyMs)}</strong>
            <span class="clock-sub">Mon 00:00 UTC</span>
          </div>
          <div class="clock">
            <span class="clock-label">{baroState?.phase === 'here' ? 'Baro leaves' : 'Baro arrives'}</span>
            <strong class="clock-val">{voidTrader ? humanWindow(baroState?.windowMs) : '—'}</strong>
            <span class="clock-sub">{voidTrader?.location ?? 'schedule unknown'}</span>
          </div>
        </div>
      </section>

      <details class="routine-checklist" bind:open={routineChecklistOpen}>
        <summary>{routineChecklistOpen ? 'Hide checklist' : "Show me today's checklist"}</summary>

        <section class="card routine">
          <h3>Daily</h3>
        <ul class="routine-list">
          <li><strong>Login tribute</strong> — claim it; the milestone days hand out Endo and the exclusive weapons/Forma that fund everything else.</li>
          <li><strong>Keep the foundry busy</strong> — start a Forma or a sellable BP every day; an idle foundry is lost plat.</li>
          <li><strong>Cap syndicate standing</strong> → buy augment mods / arcanes to flip on WFM — a steady daily plat trickle.</li>
          <li><strong>6 Steel Path incursions</strong> → Steel Essence → Teshin's weekly rotation (Riven slivers, Kuva, Umbra Forma).</li>
          <li><strong>Sortie</strong> — ~4,000 Endo on the Endo reward, plus a Riven chance.</li>
        </ul>
      </section>

      <section class="card routine">
        <h3>Weekly <span class="muted">· resets Monday</span></h3>
        <ul class="routine-list">
          <li><strong>Maroo's Ayatan Treasure Hunt</strong> — a free sculpture worth ~1,500–3,450 Endo once filled with stars.</li>
          <li><strong>Archon Hunt</strong> — up to ~8,000 Endo in one clear, plus an Archon Shard.</li>
          <li><strong>Nightwave acts</strong> → Cred for potatoes/Forma. This <em>saves</em> plat (those items are account-bound) — it doesn't earn it.</li>
          <li><strong>Baro check</strong> on arrival — but buy to <strong>hold</strong>, not flip: his mods crater ~50% on arrival and recover over weeks (watch the Sell view's “hold” tags).</li>
        </ul>
      </section>

      <section class="card routine">
        <h3>Endo — to fund the rank-up flip</h3>
        <p class="routine-note">
          Maxing one Primed mod ≈ <strong>20,000 Endo + ~1.3M credits</strong> and roughly doubles its
          value (e.g. Primed Continuity ~69p unranked → ~139p maxed). Best sources:
        </p>
        <ul class="routine-list">
          <li><strong>Arbitrations</strong> — ~5,000–10,000 Endo/hr (the grind option; needs the full star chart cleared).</li>
          <li><strong>Vodyanoi</strong> (Sedna, Steel Path) — the throughput king; a coordinated squad pushes far higher.</li>
          <li><strong>Hieracon (Pluto) excavation</strong> — steady and solo-friendly, with relics as a byproduct.</li>
          <li><strong>Archon (~8k/wk) + Sortie (~4k/day) + Maroo's weekly</strong> — passive lumps from the routines above.</li>
          <li class="routine-avoid"><strong>Skip Eidolons &amp; Profit-Taker for Endo</strong> — they pay ~zero Endo; farm those for arcanes/plat instead.</li>
        </ul>
      </section>
      </details>

    {:else if effectiveView === 'orders'}
      <section class="view-header">
        <h2>My orders</h2>
        <p class="lede">Your active warframe.market listings, fetched live from the desktop app.</p>
      </section>
      {@render pendingBanner()}
      <MyOrdersPanel
        {transport}
        {market}
        {sessionEpoch}
        ownedQty={ownedQtyForOrders}
        onauthrequired={(code) => wfmAuthDialogsRef.open(code)}
        onsummary={(s) => (ordersSummary = s)}
      />

    {:else if effectiveView === 'watches'}
      <section class="view-header">
        <h2>Price watches</h2>
        <p class="lede">Desktop notifications when an item hits your price — checked every 10 minutes against live warframe.market orders.</p>
      </section>
      <WatchlistPanel {market} />

    {:else if effectiveView === 'ledger'}
      <section class="view-header">
        <h2>Ledger</h2>
        <p class="lede">Every trade the game confirmed, read from its own log — realised plat, not estimates.</p>
      </section>
      <LedgerPanel onsetautoclose={(on) => store.setSetting('auto-close-sold', on ? 'on' : 'off')} />

    {:else if effectiveView === 'install'}
      <section class="view-header">
        <h2>FAQ</h2>
        <p class="lede">Answers to common questions.</p>
      </section>
      {@render faqContent()}
    {/if}

  </main>
</div>
{/if}

{#snippet statusStrip(inShell)}
  <!-- Shell-level status strip: one 40px spine on the landing AND the
       workspace. In the shell its brand cell sits exactly over the sidebar
       column; the rest answers "is what I'm looking at still true?" —
       inventory age, market age, orders to fix, Baro, WFM session. Rare
       inventory actions (Export / Restore / Clear) live one click deeper in
       the Refresh menu. -->
  <header class="statusbar" class:shell-strip={inShell}>
    <div class="brand">
      <h1>TennoWorth</h1>
      {#if !inShell}<span class="sub">warframe.market prices, ranked by what actually sells</span>{/if}
    </div>
    {#if isDesktop}
      <div class="cell inv" title={unresolvedCount > 0 ? `${unresolvedCount} items couldn't be price-matched (${unresolvedSummary}) — usually untradeable blueprints, quest items and very new content.` : undefined}>
        {#if inventoryName}
          <span class="dot {inventoryFreshness}" role="img" aria-label="Inventory {inventoryFreshness}"></span>
          <span>Inventory</span>
          <b class="file" title={inventoryName}>{inventoryName}</b>
          {#if inventoryStaleness}<span>·</span><b>{inventoryStaleness}</b>{/if}
        {:else}
          <span class="dot" aria-hidden="true"></span>
          <span>No inventory yet</span>
        {/if}
        <div class="refresh-wrap">
          <!-- Reflects the scan itself, not just the menu: refreshFromGame
               closes the popover before awaiting, so the "Scanning game…"
               label inside it vanished the moment it mattered and a ~10s scan
               looked like a dead click. This trigger stays on screen. -->
          <button
            class="refresh-trigger"
            class:busy={pullingInventory}
            onclick={() => (refreshOpen = !refreshOpen)}
            aria-expanded={refreshOpen}
            aria-busy={pullingInventory}
            disabled={pullingInventory}
            title={pullingInventory
              ? 'Reading the running game’s memory — this can take a few seconds.'
              : 'Load fresh inventory — re-fetch from the game. Export / Restore / Clear live in this menu too.'}
          >{pullingInventory ? 'Scanning…' : 'Refresh ▾'}</button>
          {#if refreshOpen}
            <div class="refresh-pop">
              <p class="rp-lede">Scan the running game — no file needed.</p>
              <button class="rp-primary" data-testid="desktop-scan" onclick={refreshFromGame} disabled={pullingInventory}>
                {pullingInventory ? 'Scanning game…' : 'Scan game'}
              </button>
              <div class="rp-sep" aria-hidden="true"></div>
              {#if inventoryName}
                <button class="rp-item" onclick={() => { refreshOpen = false; exportImportRef.openExport(); }} title="Download an encrypted snapshot for another device or backup.">Export…</button>
              {/if}
              <button class="rp-item" onclick={() => { refreshOpen = false; exportImportRef.pickImport(); }} title="Restore an encrypted snapshot exported from another device.">Restore…</button>
              {#if inventoryName}
                <button class="rp-item danger" onclick={() => { refreshOpen = false; handleClear(); }} title="Forget the saved inventory entirely.">Clear</button>
              {/if}
              {#if unresolvedCount > 0}
                <p class="rp-note" title="Breakdown: {unresolvedSummary}.">{unresolvedCount} items couldn't be price-matched (not shown) — usually untradeable blueprints, quest items and very new content; your sellable items aren't affected.</p>
              {/if}
            </div>
          {/if}
        </div>
      </div>
    {/if}
    <div class="cell">
      <span class="dot {marketFreshness}" role="img" aria-label="Market data {marketFreshness}"></span>
      <span>Market</span>
      <b>{marketStaleness ?? '—'}</b>
      {#if marketFreshness !== 'unknown'}<span>· {marketFreshness}</span>{/if}
    </div>
    {#if inShell && isDesktop && ordersToFix > 0}
      <div class="cell attn">
        <b>{ordersToFix}</b>
        <span>{ordersToFix === 1 ? 'order' : 'orders'} to fix</span>
        <button type="button" class="link" onclick={() => setView('orders')} aria-label="Open My orders">→</button>
      </div>
    {/if}
    {#if baroState && baroState.phase !== 'unknown'}
      <div class="cell baro">
        <span class="ducat" aria-hidden="true">⌬</span>
        <span>{baroState.phase === 'here' ? 'Baro leaves in' : 'Baro arrives in'}</span>
        <b>{humanWindow(baroState.windowMs)}</b>
      </div>
    {/if}
    <span class="grow"></span>
    {#if !inShell}
      <nav class="cell end" aria-label="Site">
        <a href="#faq">FAQ</a>
        {#if !isDesktop}<a href="#desktop">Desktop app ↓</a>{/if}
      </nav>
    {:else if isDesktop}
      <div class="cell end">
        <span>WFM</span>
        {#if wfmStatus && !wfmStatus.unlocked}
          <button type="button" class="link" onclick={() => wfmAuthDialogsRef.open(wfmStatus.logged_in ? 'needs_unlock' : 'needs_login')}>{wfmLabel}</button>
        {:else}
          <b>{wfmLabel}</b>
        {/if}
        {#if ordersSummary}<span>· {ordersSummary.live} live</span>{/if}
      </div>
    {/if}
  </header>
{/snippet}

{#snippet handoffPanel(rows)}
  <DesktopShowcase {rows} />
{/snippet}

{#snippet faqContent()}
  <section class="faq" id="faq">
    <h2>FAQ</h2>

    <details>
      <summary>Is this safe? Can I get banned?</summary>
      <p>
        The desktop app reads the running game's process memory to find the
        <code>accountId</code> and <code>nonce</code> your client already
        obtained at login, then calls DE's own inventory endpoint with
        those — same call your game client makes. It writes to disk; it
        doesn't write to the game's memory or modify game state.
      </p>
      <p>
        <strong>We can't promise this is ban-safe</strong> — no third-party
        tool honestly can. What we <em>can</em> say: the app only ever
        reads memory. It never writes to the game, never injects code, and
        doesn't interact with anti-cheat. Equivalent tools (Sainan's
        warframe-api-helper, AlecaFrame via Overwolf) have run for years with
        no documented bans, but Digital Extremes has never formally blessed
        this category of tool. Use it at your own risk; we accept none. More
        detail in <a href="#trust">Trust &amp; safety</a> below.
      </p>
    </details>

    <details>
      <summary>Where does my inventory data actually go?</summary>
      <p>
        Nowhere we control. The desktop app scans the game and keeps your
        inventory locally (SQLite + your browser's storage). This site is
        informational — it never receives or stores your inventory. The market
        snapshot is the only thing we host, and it's the same for every visitor.
      </p>
      <p>
        No accounts, no telemetry, no analytics. Inspect the network tab
        if you don't trust us.
      </p>
    </details>

    <details>
      <summary>How current are the prices?</summary>
      <p>
        A GitHub Actions cron job scrapes <a href="https://warframe.market">warframe.market</a>
        every 2 hours and commits a fresh <code>market.json</code> to the
        repo. The site serves that file. The dot next to “Market data” is
        green if the snapshot is under 6 h old, amber under 24 h, red after.
      </p>
    </details>

    <details>
      <summary>How do I list items on WFM?</summary>
      <p>
        Listing is a <strong>desktop app</strong> feature. Install the desktop
        app (Windows + Linux), scan your account, and use <strong>List on
        WFM</strong> from the Sell view. The first time, the app asks you to
        log in to warframe.market once — the sign-in token is encrypted behind
        a passphrase you choose, and it never leaves your machine. After that,
        listing and your orders work for the rest of the session.
      </p>
      <p>
        This informational site can't list: it has no account, no token, and no
        way to reach warframe.market on your behalf.
      </p>
    </details>

    <details>
      <summary>I listed an item — what happens next?</summary>
      <p>
        Your listing sits on warframe.market (we create them
        <strong>hidden</strong> so you can review first — flip them visible
        in My orders or on WFM). When a buyer wants it, they message
        you <strong>in-game</strong>: <code>/w YourName Hi! I want to buy your
        Serration…</code>. Invite them to your squad, go to any dojo or relay,
        open a trade, and put in the item while they put in the platinum.
      </p>
      <p>
        Two gotchas: you must be <strong>in-game</strong> to trade (WFM marks
        you online automatically while the site is open), and trading requires
        a clan dojo or Maroo's Bazaar. Mark the listing sold on WFM afterwards
        — or just delete it from the <strong>My orders</strong> tab here.
      </p>
    </details>

    <details>
      <summary>Can I sync between desktop and laptop?</summary>
      <p>
        There are no accounts. In the desktop app, use <strong>Export</strong>
        to save an encrypted snapshot (passphrase-only, AES-256-GCM, PBKDF2
        600k), then <strong>Restore</strong> that file on any other device.
      </p>
    </details>

    <details>
      <summary>Why no Overwolf / AlecaFrame integration?</summary>
      <p>
        Overwolf is Windows-only and bundles an always-running runtime
        plus ads. Aleca is built on top of it, which is why it's the
        dominant tool — but also why it doesn't run on Linux at all and
        why some users avoid it on Windows. We're aimed at the audience
        that wants the same insights without that surface area.
      </p>
    </details>

    <details>
      <summary>What about Rivens, Arcanes, frame mods?</summary>
      <p>
        Arcanes and frame mods (in <code>RawUpgrades</code>) are
        resolved and priced like anything else. Rivens are
        per-instance items with rolled stats — they don't have a single
        market price, they need a separate model (riven grader). Not
        supported here yet; semlar's tools do this better.
      </p>
    </details>

    <details>
      <summary>Want to support this?</summary>
      <p>
        Free, no ads, no accounts, no telemetry — always. If it saved
        you some plat and you want to chip in toward hosting, that's
        appreciated but never expected:
        <a href="https://ko-fi.com/prowly" target="_blank" rel="noopener">ko-fi.com/prowly</a>.
      </p>
    </details>
  </section>

  <section id="trust" class="faq trust">
    <h2>Trust &amp; safety</h2>
    <p class="trust-lede">
      Straight answers about what this tool touches, what it can't, and how to
      check us for yourself — not legal boilerplate. The short version: the
      desktop app only ever <em>reads</em> your running game, the web app runs
      entirely in your browser, and we can't promise this is ban-safe — so use
      it at your own risk.
    </p>

    <details>
      <summary>What the desktop app reads</summary>
      <p>
        While Warframe is running, the desktop app scans the game's memory for
        two things your client already has: your <strong>account ID</strong>
        and the <strong>session nonce</strong> it uses to talk to Digital
        Extremes. It then asks DE's own inventory API for your items — the exact
        same request the game client makes. It <strong>never writes to the
        game's memory and never injects code</strong>; it only reads, then makes
        one HTTPS call. Your account ID and nonce are used for that single
        request and discarded — never printed, never saved, never sent anywhere
        else.
      </p>
    </details>

    <details>
      <summary>Isn't reading game memory sketchy? It's what AlecaFrame does too</summary>
      <p>
        Reading a process you own is ordinary, and it's the same technique the
        most popular tool already uses. AlecaFrame reads Warframe's memory too —
        through an Overwolf plugin (<code>gep_warframeext.dll</code>) that calls
        Windows' <code>ReadProcessMemory</code>. We do the same thing without
        the Overwolf middleman (and without being Windows-only). Nothing here
        modifies the game.
      </p>
    </details>

    <details>
      <summary>What never leaves your machine</summary>
      <p>
        The web app has no backend and no accounts — every item, price join, and
        ranking is computed in your browser tab. If you log in to
        warframe.market to post listings, that login token is
        <strong>encrypted on disk</strong> (AES-256-GCM) at
        <code>~/.config/wfminv/</code> (or the Windows equivalent), and the
        webview never sees it — it stays in the Rust process. The
        desktop app can optionally remember the unlock key in your OS
        keyring (KWallet, GNOME Keyring, Windows Credential Manager) —
        never the passphrase itself; details in SECURITY.md.
      </p>
      <p>
        No telemetry, no analytics — confirm it in your browser's network tab.
      </p>
    </details>

    <details>
      <summary>How you can verify all this yourself</summary>
      <p>
        Everything is open source — read the desktop app's memory-scan code and
        the web app's join logic. The desktop releases are
        <strong>reproducibly built in public CI</strong>: you can audit the
        workflow file, the source at the tagged commit, and the build logs, and
        every release ships checksums so you can confirm
        your download matches. The site
        loads <strong>zero third-party scripts</strong> — inspect the page's
        Content-Security-Policy. Full detail lives in
        <a href="https://github.com/tennoworth/tennoworth/blob/main/SECURITY.md" target="_blank" rel="noopener noreferrer">SECURITY.md</a>.
      </p>
    </details>

    <details>
      <summary>The honest risk — and what happens if DE's stance changes</summary>
      <p>
        We <strong>can't promise this is ban-safe</strong>; no third-party tool
        honestly can. What we can say: the desktop app only reads memory, never
        writes or injects, and doesn't interact with anti-cheat. Equivalent
        tools (Sainan's warframe-api-helper, AlecaFrame via Overwolf) have run
        for years with no documented bans, but Digital Extremes has never
        formally blessed this category of tool. Use it at your own risk.
      </p>
      <p>
        And if DE's stance ever changes, only the memory-scan path stops: the
        <strong>market browser on this site keeps working</strong>.
      </p>
    </details>
  </section>
{/snippet}

{#snippet generalBanners()}
  <!-- Cross-view banner region: pull-error and the desktop update banner, each
       independently dismissible. Rendered on both the landing and the workspace
       so a failure is visible wherever the user is standing. -->
  {#if pullError}
    <div class="card warn-banner general-banner" role="alert">
      <div class="gb-body gb-pre">{pullError}</div>
      <div class="gb-actions">
        {#if isDesktop}
          <button class="gb-report" onclick={reportScanBroke} disabled={reportingScan}>
            {reportingScan ? 'Opening…' : 'Report this'}
          </button>
        {/if}
        <button class="gb-dismiss" aria-label="Dismiss" onclick={() => (pullError = null)}>×</button>
      </div>
    </div>
    {#if reportUrl}
      <!-- Shown only when the browser did not open: the report must still be
           filable by hand rather than dead-ending on a failed launch. -->
      <div class="card warn-banner general-banner" role="status">
        <div class="gb-body">
          Couldn't open a browser. Copy this link to file the report:
          <div class="gb-pre report-url">{reportUrl}</div>
        </div>
        <div class="gb-actions">
          <button class="gb-dismiss" aria-label="Dismiss" onclick={() => (reportUrl = null)}>×</button>
        </div>
      </div>
    {/if}
  {/if}
  {#if isDesktop && trayHint}
    <div class="card warn-banner general-banner" role="status">
      <div class="gb-body">
        Still running in your tray. Closing the window keeps TennoWorth in the
        background — use the tray icon's Quit to exit.
      </div>
      <div class="gb-actions">
        <button class="gb-dismiss" aria-label="Dismiss" onclick={() => (trayHint = false)}>×</button>
      </div>
    </div>
  {/if}
  {#if isDesktop}
    <DesktopUpdateBanner />
  {/if}
{/snippet}

{#snippet pendingBanner()}
  {#if isDesktop && (pendingPlan || resumePhase !== 'idle')}
    <section class="card pending-banner">
      {#if resumePhase === 'running'}
        <div class="row">
          <div class="src">
            <span class="dot aging" aria-hidden="true"></span>
            <strong>Resuming interrupted batch…</strong>
            <span class="muted">~{Math.ceil(pendingRemaining * 0.35 + 1)}s</span>
          </div>
        </div>
      {:else if resumePhase === 'done'}
        <div class="row">
          <div class="src">
            <span class="dot fresh" aria-hidden="true"></span>
            <strong>Resumed.</strong>
            <span class="muted">
              <span class="ok-text">{resumeOk} created</span>
              {#if resumeErr > 0}· <span class="bad">{resumeErr} failed</span>{/if}.
              New listings are still hidden — toggle from the orders panel.
            </span>
          </div>
          <div class="row gap-sm">
            <button class="ghost" onclick={() => { resumePhase = 'idle'; resumeResults = []; }}>Dismiss</button>
          </div>
        </div>
      {:else if resumePhase === 'error'}
        <div class="row">
          <div class="src">
            <span class="dot stale" aria-hidden="true"></span>
            <strong>Resume failed.</strong>
            <span class="muted bad">{resumeError}</span>
          </div>
          <div class="row gap-sm">
            <button onclick={doResume}>Retry</button>
            <button class="ghost" onclick={doDiscard}>Discard pending</button>
          </div>
        </div>
      {:else if pendingPlan && pendingRemaining > 0}
        <div class="row">
          <div class="src">
            <span class="dot aging" aria-hidden="true"></span>
            <strong>Interrupted batch from {new Date(pendingPlan.started_at).toLocaleString()}</strong>
            <span class="muted">
              · {pendingRemaining} pending{pendingDone > 0 ? `, ${pendingDone} already done` : ''}
            </span>
          </div>
          <div class="row gap-sm">
            <button onclick={doResume}>Resume</button>
            <button class="ghost" onclick={doDiscard}>Discard</button>
          </div>
        </div>
      {/if}
    </section>
  {/if}
{/snippet}

<!-- Desktop only (listing needs wfm-core's session). onauthrequired fires on
     typed needs_login/needs_unlock rejections. -->
<ListingReviewModal
  bind:open={listingOpen}
  rows={reviewRowsOverride ?? listableRows.slice(0, 50)}
  {transport}
  onauthrequired={(code) => wfmAuthDialogsRef.open(code, 'list')}
  onclose={() => (reviewRowsOverride = null)}
/>

{#if isDesktop}
  <WfmAuthDialogs bind:this={wfmAuthDialogsRef} onunlocked={handleWfmUnlocked} />
{/if}

<ExportImportDialogs
  bind:this={exportImportRef}
  owned={resolved.owned}
  {inventoryName}
  {lastUpdated}
  onimport={handleImported}
/>

<style>
  main.landing {
    /* Landing — a 76rem reading column (1216px at 1440 → 1292 at 1920 →
       1368 at 2560): strip → lede → search → movers → vaulted + Baro →
       hand-off → FAQ → footer, every block on the --stack rhythm. */
    max-width: 76rem;
    margin: 0 auto;
    padding: var(--s3) var(--gutter) var(--s6);
    display: flex;
    flex-direction: column;
    gap: var(--stack);
    min-width: 0;
  }

  /* Shell layout: a 40px status strip spanning both columns, then the
     persistent left rail + workspace column. Sidebar is 13.5rem (216px —
     room for nav-item label + 3-digit badge); workspace fills the rest.
     The shell caps at 122.5rem (1960px at 16px root) and centres. */
  .shell {
    display: grid;
    grid-template-columns: var(--sidebar) minmax(0, 1fr);
    grid-template-rows: auto 1fr;
    max-width: min(122.5rem, 100vw);
    margin: 0 auto;
    min-height: 100vh;
  }

  main.workspace {
    padding: var(--s3) var(--s4) var(--s6);
    display: flex;
    flex-direction: column;
    gap: var(--stack);
    min-width: 0;
  }

  /* ---- Status strip: one element on the landing and the shell ------- */
  .statusbar {
    display: flex;
    align-items: stretch;
    height: var(--strip);
    background: var(--panel);
    border-bottom: 1px solid var(--border);
    font-size: 0.75rem;
    line-height: 1rem;
    color: var(--muted);
    white-space: nowrap;
  }
  .statusbar > * { flex-shrink: 0; }
  .statusbar .grow { flex: 1 1 0; min-width: 0; }
  /* In the shell the strip is the y-origin: sticky above the sidebar and
     the workspace, spanning both grid columns. */
  .statusbar.shell-strip {
    grid-column: 1 / -1;
    position: sticky;
    top: 0;
    z-index: 20;
  }
  .statusbar .brand {
    display: flex;
    align-items: center;
    gap: var(--s3);
    padding: 0 var(--inset);
    min-width: 0;
  }
  /* The brand cell IS the sidebar's top: same width, same right border, so
     the top-left corner still says TennoWorth without a second brand block. */
  .statusbar.shell-strip .brand {
    width: var(--sidebar);
    border-right: 1px solid var(--border);
  }
  .statusbar .brand h1 {
    margin: 0;
    font-size: 0.9375rem;
    font-weight: 700;
    letter-spacing: 0.01em;
    line-height: 1.25rem;
    color: var(--fg);
  }
  .statusbar .brand .sub {
    color: var(--faint);
    font-size: 0.6875rem;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .statusbar .cell {
    display: flex;
    align-items: center;
    gap: var(--s2);
    padding: 0 var(--s3);
    border-left: 1px var(--rule) var(--hairline);
    min-width: 0;
  }
  .statusbar.shell-strip .brand + .cell { border-left: none; }
  .statusbar .cell.end { padding-right: var(--inset); }
  .statusbar .cell b {
    color: var(--fg);
    font-weight: 600;
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
  }
  .statusbar .cell.attn b { color: var(--warn); }
  /* A long inventory filename must not push the WFM cell off the right rail. */
  .statusbar .cell .file { max-width: 16rem; overflow: hidden; text-overflow: ellipsis; }
  .statusbar .cell .ducat { color: var(--ducat); }
  .statusbar .cell a { color: var(--accent); text-decoration: none; }
  .statusbar .cell a + a { margin-left: var(--s2); }
  .statusbar .cell .link {
    font: inherit;
    color: var(--accent);
    background: transparent;
    border: none;
    padding: 0 var(--s1);
    height: var(--ctl-xs);
    cursor: pointer;
  }
  .statusbar .cell .link:hover { text-decoration: underline; background: transparent; }
  /* Landing: same strip, framed as the first block of the reading column. */
  main.landing > .statusbar {
    border: 1px solid var(--border);
    border-radius: var(--radius-panel);
  }
  main.landing > .statusbar .brand { flex: 1 1 auto; }

  /* Sidebar — nav only, sticky under the strip; the theme switcher and the
     build string sit in its foot. */
  aside.sidebar {
    position: sticky;
    top: var(--strip);
    height: calc(100vh - var(--strip));
    border-right: 1px solid var(--border);
    background: var(--panel);
    display: flex;
    flex-direction: column;
    padding: var(--s2) 0 var(--s3);
  }
  aside.sidebar nav {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }
  .nav-group {
    padding: 0 0 var(--s2);
    border-bottom: 1px var(--rule) var(--hairline);
  }
  .nav-group:last-child { border-bottom: none; }
  .nav-label {
    font-size: 10px;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--faint);
    font-weight: 600;
    padding: var(--s2) var(--inset) var(--s1);
  }
  .nav-item {
    /* Native <button> reset → flat, full-width, transparent. The accent
       left border + tinted background mark the active view. */
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    height: var(--ctl);
    padding: 0 var(--inset);
    font: inherit;
    font-size: 13px;
    color: var(--muted);
    background: transparent;
    border: none;
    border-left: 2px solid transparent;
    text-align: left;
    cursor: pointer;
  }
  .nav-item:hover { color: var(--fg); background: var(--hover-tint); }
  .nav-item.active {
    color: var(--fg);
    border-left-color: var(--accent);
    background: var(--panel-2);
  }
  .nav-item .badge {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--muted);
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-tag);
    padding: 0 5px;
  }
  .nav-item.active .badge {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border));
  }
  .nav-item .badge.warn { color: var(--warn); border-color: color-mix(in srgb, var(--warn) 40%, var(--border)); }
  .nav-item .badge.here {
    color: var(--ducat);
    border-color: color-mix(in srgb, var(--ducat) 40%, var(--border));
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  /* Sidebar foot: theme switcher + build string. */
  .sfoot {
    padding: var(--s3) var(--s3) 0;
    border-top: 1px var(--rule) var(--hairline);
    display: flex;
    flex-direction: column;
    gap: var(--s2);
  }
  .sfoot .ver { padding: 0 var(--s1); }

  /* Refresh ▾ in the strip: a 24px control inside the 40px bar. */
  .refresh-trigger {
    font: inherit;
    font-size: 0.6875rem;
    height: var(--ctl-xs);
    padding: 0 var(--s2);
    color: var(--muted);
    background: transparent;
    border: 1px solid var(--border);
    border-radius: var(--radius-ctl);
    cursor: pointer;
    margin-left: var(--s1);
  }
  .refresh-trigger:hover:not(:disabled) { color: var(--fg); border-color: var(--accent); background: transparent; }
  .refresh-wrap { position: relative; }
  /* The trigger is disabled mid-scan, and the global button:disabled rule
     dims it to 0.5 — which reads as "unavailable", the opposite of the
     "working on it" signal a multi-second scan needs. Keep it legible and
     accent-tinted so it reads as busy. */
  /* Reserve the width both labels need. "Refresh ▾" and "Scanning…" render at
     different widths, and without a floor the swap resized the button mid-scan
     — enough to push Export up beside it and wrap Clear onto its own row, so
     the whole sidebar footer jumped exactly when the user was watching it. */
  .refresh-trigger { min-width: 9ch; text-align: center; }
  .refresh-trigger.busy {
    opacity: 1;
    color: var(--accent);
    border-color: var(--accent);
    cursor: progress;
  }
  /* Opens downward from the strip as a contained menu: Scan (primary), then
     the rare actions — Export / Restore / Clear — as plain menu items. */
  .refresh-pop {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    z-index: 30;
    width: 15rem;
    max-width: 80vw;
    padding: var(--s3);
    display: flex;
    flex-direction: column;
    gap: var(--s2);
    white-space: normal;
    text-align: left;
    background: var(--panel-2);
    border: 1px solid var(--accent);
    border-radius: var(--radius-panel);
    box-shadow: var(--shadow-pop);
  }
  .refresh-pop .rp-lede { margin: 0; font-size: 12px; color: var(--fg); line-height: 1.45; }
  .refresh-pop .rp-primary {
    font-size: 12px;
    padding: 7px 10px;
    color: var(--on-accent);
    background: var(--accent);
    border: 1px solid var(--accent);
    border-radius: var(--radius-ctl);
    cursor: pointer;
    font-weight: 600;
  }
  .refresh-pop .rp-primary:hover:not(:disabled) { filter: brightness(1.08); }
  .refresh-pop .rp-primary:disabled { opacity: 0.6; cursor: default; }
  .refresh-pop .rp-sep { border-top: 1px var(--rule) var(--hairline); margin: var(--s1) 0; }
  .refresh-pop .rp-item {
    font: inherit;
    font-size: 12px;
    text-align: left;
    color: var(--fg);
    background: transparent;
    border: none;
    border-radius: var(--radius-ctl);
    padding: var(--s1) var(--s2);
    height: var(--ctl-xs);
    cursor: pointer;
  }
  .refresh-pop .rp-item:hover { background: var(--hover); }
  .refresh-pop .rp-item.danger:hover { color: var(--bad); }
  .refresh-pop .rp-note { margin: var(--s1) 0 0; font-size: 11px; line-height: 1.45; color: var(--muted); }

  /* Workspace view header — h2 + a hover/focus info dot carrying the
     one-sentence lede, folded onto a single row (was h2 + a full lede
     paragraph on its own line) to reclaim vertical space above the fold. */
  .view-header {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-height: 28px;
  }
  .view-header h2 {
    font-size: 20px;
    font-weight: 600;
    text-transform: none;
    letter-spacing: -0.01em;
    color: var(--fg);
    margin: 0;
  }
  /* Mobile: stack the shell. The status strip wraps its cells; the sidebar
     becomes a horizontal scroll strip under it. Below ~900px the 216px rail
     eats too much of the workspace, so the grid collapses. */
  @media (max-width: 900px) {
    .shell { grid-template-columns: 1fr; }
    aside.sidebar {
      position: static;
      height: auto;
      border-right: none;
      border-bottom: 1px solid var(--border);
      padding: 12px 0 0;
    }
    aside.sidebar nav {
      flex-direction: row;
      overflow-x: auto;
      overflow-y: hidden;
      padding: 4px 12px;
    }
    .nav-group {
      flex: 0 0 auto;
      display: flex;
      align-items: center;
      gap: 4px;
      border-bottom: none;
      border-right: 1px var(--rule) var(--hairline);
      padding: 0 8px;
    }
    .nav-group:last-child { border-right: none; }
    .nav-label { display: none; }
    .nav-item {
      width: auto;
      border-left: none;
      border-bottom: 2px solid transparent;
      padding: 6px 10px;
    }
    .nav-item.active {
      border-left: none;
      border-bottom-color: var(--accent);
    }
    .statusbar { flex-wrap: wrap; height: auto; min-height: var(--strip); position: static; }
    .statusbar.shell-strip .brand { width: auto; border-right: none; }
    .sfoot { padding: var(--s2) var(--s3); }
    main.workspace { padding: 16px 16px 32px; }
  }
  header h1 {
    margin: 0;
    font-size: 22px;
    font-weight: 600;
    letter-spacing: -0.015em;
  }
  /* Landing header: title block left, theme switcher top-right; the switcher
     drops under the title on narrow viewports. */
  .landing-head { display: flex; align-items: center; justify-content: space-between; gap: var(--s2) var(--s5); flex-wrap: wrap; min-height: var(--rail); }
  /* --muted on the yorha ground is ~4.2:1, so the lede runs a step larger
     than body text to stay comfortably legible. */
  .landing-head .lede { flex: 1 1 20rem; min-width: 0; margin: 0; font-size: 14px; line-height: 1.375rem; color: var(--muted); }
  .landing-head .lede em { color: var(--fg); font-style: normal; font-weight: 600; }
  .landing-theme { flex: 0 0 auto; }
  h2 { margin: 0 0 4px 0; font-size: 14px; font-weight: 600; letter-spacing: 0.04em; text-transform: uppercase; color: var(--muted); }
  h3 { margin: 0 0 4px 0; font-size: 14px; font-weight: 600; }
  .sub { color: var(--muted); margin: 6px 0 0 0; max-width: 64ch; font-size: 13px; }
  .ver {
    color: var(--faint);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.02em;
  }

  /* Upsell lead — separates the free market browser above from the desktop-app
     pitch below. A hairline + top padding, no box. */
  .upsell-lead {
    border-top: 1px var(--rule) var(--hairline);
    padding-top: 18px;
    margin-top: 4px;
  }
  .upsell-lead h2 { margin: 0; }
  .upsell-lead .sub { margin-top: 4px; }
  .desktop-scan-row {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
    margin-top: 12px;
  }
  .desktop-hero .trust {
    color: var(--muted);
    font-size: 12px;
    margin-top: 10px;
  }

  .dot {

    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--muted);
    display: inline-block;
  }
  .dot.fresh   { background: var(--good); }
  .dot.aging   { background: var(--warn); }
  .dot.stale   { background: var(--bad); }

  /* Card / row scaffolding */
  .card {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--radius-panel);
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .card.error { border-color: var(--bad); color: var(--bad); }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    flex-wrap: wrap;
  }
  .row.gap-sm { gap: 10px; }
  .src { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; font-size: 13px; }
  .src strong { font-weight: 600; }


  /* First-session Score explainer. Single dismissable line above the
     table — the casual-flipper persona was confused by what Score
     meant; hover-tooltip alone wasn't enough. localStorage flag means
     each user sees it once. */
  /* Cross-view banner region (unreachable / bad deep link / pull error). Reuses
     the warn-banner left-accent + card tokens; adds a body/actions row so the
     dismiss × (and Retry) sit at the end without wrapping under the copy. */
  .general-banner {
    flex-direction: row;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }
  .general-banner .gb-body { min-width: 0; }
  .general-banner .gb-body.gb-pre {
    white-space: pre-wrap;
    font-family: var(--font-mono);
    font-size: 12px;
    line-height: 1.5;
  }
  .general-banner .gb-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }
  .general-banner .gb-dismiss {
    background: transparent;
    border: none;
    color: var(--muted);
    font-size: 16px;
    line-height: 1;
    cursor: pointer;
    padding: 3px;
  }
  .general-banner .gb-dismiss:hover { color: var(--fg); }
  .general-banner .gb-report {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--muted);
    font-size: 11px;
    padding: 2px 8px;
    border-radius: var(--radius-input);
    cursor: pointer;
    white-space: nowrap;
  }
  .general-banner .gb-report:hover:not(:disabled) { color: var(--fg); }
  .general-banner .gb-report:disabled { opacity: .6; cursor: default; }
  /* The fallback link is long; let it wrap and stay selectable rather than
     overflow the banner. */
  .general-banner .report-url {
    margin-top: 6px;
    word-break: break-all;
    user-select: all;
  }
  /* Action-verb prefix on rec cards. Casual users said the cards were
     too noun-heavy — leading with a verb gives them the instruction. */
  .reco-verb {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--accent);
    margin-right: 6px;
  }

  button.ghost {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--muted);
    font-size: 12px;
    padding: 4px 10px;
  }
  button.ghost:hover { background: var(--panel-2); color: var(--fg); }
  code { background: var(--panel-2); padding: 1px 6px; border-radius: var(--radius-input); font-family: var(--font-mono); font-size: 0.93em; }

  /* Set-completion card — recommendation rows. Three reco kinds
     distinguished by a small uppercase pill so the user can scan and
     pick a strategy without reading every detail line. Net plat is the
     right-aligned headline number per row. */
  .set-recos { padding: 14px 16px; gap: 8px; }
  .reco {
    border-top: 1px var(--rule) var(--hairline);
    padding: 10px 0;
    align-items: center;
  }
  .reco:first-of-type { border-top: none; }
  .reco-body { display: flex; flex-direction: column; gap: 4px; min-width: 0; flex: 1; }
  .reco-title { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .reco-title a { color: var(--fg); text-decoration: none; font-weight: 600; font-size: 13px; }
  .reco-title a:hover { color: var(--accent); text-decoration: underline; }
  .kind {
    font-size: 10px;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--muted);
    border: 1px solid var(--border);
    border-radius: var(--radius-tag);
    padding: 1px 6px;
    font-family: var(--font-mono);
  }
  .kind-near-complete       { color: var(--accent); border-color: color-mix(in srgb, var(--accent) 40%, var(--border)); }
  .kind-complete-with-extras { color: var(--good);   border-color: color-mix(in srgb, var(--good)   40%, var(--border)); }
  .kind-extras              { color: var(--warn);   border-color: color-mix(in srgb, var(--warn)   40%, var(--border)); }
  .set-liq {
    font-size: 10px;
    letter-spacing: 0.04em;
    border-radius: var(--radius-tag);
    padding: 1px 6px;
    font-family: var(--font-mono);
  }
  .set-liq.moving { color: var(--muted); }
  .set-liq.thin   { color: var(--warn); }
  .set-liq.cold   { color: var(--bad); }
  .reco-detail { font-size: 12.5px; margin: 0; line-height: 1.5; }
  .reco-detail strong { color: var(--fg); font-weight: 600; }
  .good-text { color: var(--good); }
  .bad-text { color: var(--bad); }
  /* The net plat is inline with the verb + set name, so the phrase reads
     "Complete Mesa Prime +95p" as a single declarative sentence rather
     than a verb-on-left / number-on-right two-column row. */
  .reco-net-inline {
    font-family: var(--font-mono);
    font-size: 14px;
    font-weight: 600;
    color: var(--good);
    margin-left: 4px;
  }

  /* Baro card. Quiet by default (countdown mode); flips to a warm-gold
     border when Baro is actively visiting so the user can't miss the
     window. Sibling-card pattern, same shape as set-recos / relic-planner. */
  .baro-card {
    padding: 14px 16px;
    border-left: 3px solid color-mix(in srgb, var(--ducat) 60%, var(--border));
  }
  .baro-card.here {
    border-left-color: var(--ducat);
    background: color-mix(in srgb, var(--ducat) 6%, var(--panel));
  }
  .baro-icon {
    color: var(--ducat);
    font-size: 22px;
    line-height: 1;
  }
  .baro-body { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .baro-detail { font-size: 12.5px; margin: 0; line-height: 1.5; }
  .baro-detail strong { color: var(--fg); font-weight: 600; }
  .baro-detail .unit { color: var(--muted); font-size: 11px; margin-left: 1px; }

  /* Profit routines — countdown clocks + reminder lists. */
  .routine h3 { margin: 0 0 6px; font-size: 14px; }
  .routine h3 .muted { font-weight: 400; }
  .routine-clocks {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 12px;
  }
  @media (max-width: 760px) { .routine-clocks { grid-template-columns: 1fr; } }
  .clock {
    display: flex;
    flex-direction: column;
    gap: 2px;
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-panel);
    padding: 12px 14px;
  }
  .clock-label { font-size: 11px; text-transform: uppercase; letter-spacing: 0.04em; color: var(--muted); }
  .clock-val { font-size: 20px; font-weight: 600; color: var(--fg); }
  .clock-sub { font-size: 11px; color: var(--muted); }
  .routine-list { margin: 8px 0 0; padding-left: 18px; display: flex; flex-direction: column; gap: 7px; }
  .routine-list li { font-size: 12.5px; line-height: 1.5; }
  .routine-list strong { color: var(--fg); font-weight: 600; }
  .routine-note { font-size: 12.5px; line-height: 1.5; margin: 0; color: var(--muted); }
  .routine-avoid { color: var(--muted); }
  /* Routine checklist — the three routine cards sit behind a collapsed-by-
     default <details> so the clocks (the daily urgency) stay above the fold. */
  .routine-checklist { display: flex; flex-direction: column; gap: 12px; }
  .routine-checklist summary {
    cursor: pointer;
    align-self: flex-start;
    color: var(--muted);
    font-size: 12px;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    user-select: none;
  }
  .routine-checklist summary:hover { color: var(--accent); }
  .routine-checklist[open] summary { color: var(--accent); }

  /* Relic planner — three-card grid above the main table. Equal-weight
     cards because the user is making a "what tonight" choice and equal
     real estate makes the comparison direct. Each card leads with EPP
     (expected plat per crack); the moving-rewards fraction flags traps
     where a high-EPP relic has dead reward markets. */
  .relic-planner { padding: 14px 16px; gap: 12px; }
  .relic-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 12px;
  }
  @media (max-width: 760px) { .relic-grid { grid-template-columns: 1fr; } }
  .relic-more { margin-top: 12px; text-align: center; }
  .relic-card {
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-panel);
    padding: 12px 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .relic-title { display: flex; justify-content: space-between; align-items: baseline; gap: 8px; }
  .relic-title a { color: var(--fg); text-decoration: none; font-weight: 600; font-size: 13px; }
  .relic-title a:hover { color: var(--accent); text-decoration: underline; }
  .small { font-size: 11px; }
  .relic-epp {
    font-family: var(--font-mono);
    font-size: 22px;
    font-weight: 600;
    letter-spacing: -0.01em;
    line-height: 1.1;
  }
  .relic-epp .unit { font-size: 11px; color: var(--muted); margin-left: 4px; }
  .relic-meta {
    font-size: 11.5px;
    color: var(--muted);
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
  }
  .relic-rewards { font-size: 11.5px; }
  .relic-rewards summary {
    cursor: pointer;
    color: var(--muted);
    letter-spacing: 0.03em;
    text-transform: uppercase;
    font-size: 10px;
    user-select: none;
  }
  .relic-rewards[open] summary { color: var(--accent); }
  .relic-rewards ul { list-style: none; margin: 6px 0 0; padding: 0; display: flex; flex-direction: column; gap: 3px; }
  .relic-rewards li {
    display: grid;
    grid-template-columns: 14px 1fr auto auto;
    gap: 6px;
    align-items: baseline;
    font-family: var(--font-mono);
    font-size: 11px;
  }
  .reward-name { color: var(--fg); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .rarity { font-weight: 600; font-size: 10px; text-align: center; }
  .rarity-common   { color: var(--muted); }
  .rarity-uncommon { color: var(--accent); }
  .rarity-rare     { color: var(--warn); }
  .rarity-legendary { color: var(--good); }

  /* Pending-batch banner — draws the eye with a left-border accent so the
     user doesn't miss that an interrupted batch is recoverable. */
  .card.pending-banner {
    border-left: 3px solid var(--warn);
    padding-left: 13px;
  }
  .ok-text { color: var(--good); }
  .bad { color: var(--bad); }

  /* Empty state with a one-click fix instead of just shrugging. */
  .card.empty {
    flex-direction: row;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }
  .card.empty strong { font-weight: 600; }
  .card.empty p { margin: 4px 0 0 0; }

  /* FAQ — native <details>, minimal chrome, custom marker. */
  .faq {
    display: flex;
    flex-direction: column;
    gap: 0;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--radius-panel);
    padding: 4px 18px;
    margin-top: 8px;
  }
  .faq h2 { padding: 14px 0 8px; margin: 0; }
  /* Landing: two columns of questions under the one head. Row heights follow
     the tallest open answer in that row — acceptable for a FAQ. */
  main.landing .faq { display: grid; grid-template-columns: 1fr 1fr; gap: 0 var(--s5); margin-top: calc(var(--s5) - var(--stack)); }
  main.landing .faq > h2, main.landing .faq > .trust-lede { grid-column: 1 / -1; }
  main.landing .faq details:nth-of-type(2) { border-top: 1px var(--rule) var(--hairline); }
  @media (max-width: 720px) { main.landing .faq { grid-template-columns: 1fr; } }
  .faq details {
    border-top: 1px var(--rule) var(--hairline);
    padding: 12px 0;
  }
  .faq details > summary {
    cursor: pointer;
    list-style: none;
    font-size: 13.5px;
    font-weight: 500;
    color: var(--fg);
    display: flex;
    align-items: center;
    gap: 10px;
    padding-right: 24px;
    position: relative;
    user-select: none;
    transition: color 120ms ease;
  }
  .faq details > summary::-webkit-details-marker { display: none; }
  .faq details > summary::after {
    content: '+';
    position: absolute;
    right: 0;
    font-family: var(--font-mono);
    color: var(--muted);
    font-size: 14px;
    transition: transform 120ms ease, color 120ms ease;
  }
  .faq details[open] > summary::after { content: '−'; color: var(--accent); }
  .faq details > summary:hover { color: var(--accent); }
  .faq details > p {
    margin: 10px 0 0 0;
    font-size: 13px;
    color: var(--muted);
    line-height: 1.6;
    max-width: 72ch;
  }
  .faq details > p + p { margin-top: 8px; }
  .faq details > p code,
  .faq details > p strong { color: var(--fg); }

  .trust { scroll-margin-top: 16px; }
  .trust-lede {
    margin: 12px 0 4px;
    padding-bottom: 4px;
    font-size: 13px;
    color: var(--fg);
    line-height: 1.6;
    max-width: 72ch;
  }

  footer.sitefoot {
    display: flex;
    align-items: baseline;
    gap: var(--s4);
    flex-wrap: wrap;
    padding: var(--s3) 0 var(--s5);
    margin-top: calc(var(--s5) - var(--stack));
    border-top: 1px var(--rule) var(--hairline);
    color: var(--muted); /* real information (licence, data sources) — --faint is decorative-only and lands at ~2.2:1 on the yorha ground */
    font-size: 11px;
    line-height: 1rem;
  }
  footer.sitefoot .grow { flex: 1 1 24rem; }
  footer.sitefoot a { color: var(--muted); text-decoration: none; }
  footer.sitefoot a:hover { color: var(--accent); }

  /* .cryptobox dialog styling moved to WfmAuthDialogs.svelte and
     ExportImportDialogs.svelte — no more dialog.cryptobox elements render
     directly in this template. */
</style>
