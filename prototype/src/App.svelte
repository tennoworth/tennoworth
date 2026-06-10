<script lang="ts">
  // @ts-nocheck — App.svelte is presentation glue: dialog refs, catch
  // blocks, event handlers. The high-value typing already lives at the
  // lib/ boundary (Market, Inventory, OwnedRecord etc.). Annotating
  // every `catch (e: unknown)` and `dialog: HTMLDialogElement | undefined`
  // here would be busy-work that catches no real bugs. Revisit if a
  // refactor extracts state into a typed store.
  import { onMount, untrack } from 'svelte';
  import DropZone from './components/DropZone.svelte';
  import ResultsTable from './components/ResultsTable.svelte';
  import InstallWidget from './components/InstallWidget.svelte';
  import ListingReviewModal from './components/ListingReviewModal.svelte';
  import MyOrdersPanel from './components/MyOrdersPanel.svelte';
  import { flattenInventory, extractKeptLvls } from './lib/inventory';
  import { loadCatalogs, resolvePath, type Catalogs } from './lib/resolver';
  import { loadMarket, lookup } from './lib/market';
  import { scoreRow, bandSignal, clearingPrice } from './lib/sell-priority';
  import { deriveSetRecos } from './lib/set-recos';
  import { deriveRelicPlan } from './lib/relic-planner';
  import {
    saveSnapshot, loadSnapshot, clearSnapshot, diffOwned,
  } from './lib/storage';
  import { encryptPayload, decryptPayload, isEncrypted } from './lib/crypto';
  import {
    loadCompanionConfig, saveCompanionConfig, clearCompanionConfig,
    parseCompanionUrl, pingCompanion,
    getPendingPlan, resumePendingPlan, discardPendingPlan,
  } from './lib/companion';
  import type { CompanionConfig, Inventory, Market, OwnedRecord, PendingPlan, ItemResult } from './lib/types';

  type Phase = 'idle' | 'loading' | 'done' | 'error';
  let phase = $state<Phase>('idle');
  let error = $state<string | null>(null);
  let inventoryName = $state<string | null>(null);
  let lastUpdated = $state<number | null>(null);

  let catalogs = $state<Catalogs | null>(null);
  let market = $state<Market | null>(null);
  let resolved = $state<{ owned: Map<string, OwnedRecord>; unresolved: Record<string, number> }>({
    owned: new Map(),
    unresolved: {},
  });
  let deltas = $state<Map<string, number>>(new Map());
  let results = $state<any[]>([]);
  let minPrice = $state(5);
  let minOwned = $state(1);
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
  const FILTERS_OPEN_KEY = 'wfminv:filters-open-v1';
  let filtersOpen = $state(
    typeof localStorage !== 'undefined' && localStorage.getItem(FILTERS_OPEN_KEY) === '1'
  );
  function toggleFiltersOpen(e: Event): void {
    const isOpen = (e.currentTarget as HTMLDetailsElement).open;
    filtersOpen = isOpen;
    try { localStorage.setItem(FILTERS_OPEN_KEY, isOpen ? '1' : '0'); } catch { /* ignore */ }
  }

  // Sidebar nav view — which workspace pane is active. Persists so a
  // reload lands the user back where they left off. Falls through to
  // 'sell' if the persisted view's data isn't available (Baro not
  // visiting, companion not connected).
  type View = 'sell' | 'sets' | 'relics' | 'baro' | 'routines' | 'orders' | 'companion' | 'install';
  const VIEW_KEY = 'wfminv:view-v1';
  const VALID_VIEWS: ReadonlySet<View> = new Set([
    'sell', 'sets', 'relics', 'baro', 'routines', 'orders', 'companion', 'install',
  ]);
  let view = $state<View>(
    (() => {
      try {
        const saved = localStorage.getItem(VIEW_KEY) as View | null;
        return saved && VALID_VIEWS.has(saved) ? saved : 'sell';
      } catch { return 'sell'; }
    })()
  );
  function setView(v: View): void {
    view = v;
    try { localStorage.setItem(VIEW_KEY, v); } catch { /* ignore */ }
  }

  // First-session Score explainer — dismissable, one-time, persists.
  const SCORE_EXPLAINER_KEY = 'wfminv:score-explainer-dismissed-v1';
  let scoreExplainerDismissed = $state(
    typeof localStorage !== 'undefined' && localStorage.getItem(SCORE_EXPLAINER_KEY) === '1'
  );
  function dismissScoreExplainer(): void {
    scoreExplainerDismissed = true;
    try { localStorage.setItem(SCORE_EXPLAINER_KEY, '1'); } catch { /* ignore */ }
  }

  // Preset pills above the rec cards. Each preset is one click that
  // updates the filter state to a known-useful combination. Currently
  // selected preset is tracked so the pill can show as active. Custom
  // edits null out the selection (you're no longer "on" a preset).
  let activePreset = $state<string | null>('default');
  // Each preset is a one-click configuration of (filters, tag chips, and
  // visible columns). Casual users said the 11-column default table was
  // overwhelming; presets now also reshape what shows so the workflow's
  // signal isn't drowned in unrelated numbers. `columns` is the ordered
  // visible-column list; missing = all columns (Default).
  interface Preset {
    minPrice: number;
    hideAtLvl: number;
    typeFilter: string;
    activeTags: string[];
    label: string;
    hint: string;
    columns?: string[];
    vaultOnly?: boolean;
    minVol?: number; // hard per-preset liquidity floor (Trending uses it)
    minMedian?: number; // 90d-baseline price floor — a +1100% Δ on a 1p fish is noise or wash-trading, not a mover
    defaultSort?: { key: string; dir: number };
  }
  const PRESETS: Record<string, Preset> = {
    default:  {
      minPrice: 5, hideAtLvl: 5, typeFilter: 'all', activeTags: [],
      label: 'Default', hint: 'sane defaults — score sort',
      defaultSort: { key: 'sell_score', dir: -1 },
    },
    ducats: {
      minPrice: 0, hideAtLvl: 11, typeFilter: 'all', activeTags: ['prime'],
      label: 'Ducats', hint: 'best ducat value first',
      columns: ['name', 'owned', 'sell_score', 'low_sell', 'volume_48h', 'ducats', 'plat_per_100d'],
      // Rank by plat-per-100-ducats ASCENDING: lowest plat value per ducat =
      // worth more fed to Baro than sold on WFM. (Nulls — non-ducat rows — sink.)
      defaultSort: { key: 'plat_per_100d', dir: 1 },
    },
    trending: {
      minPrice: 5, hideAtLvl: 5, typeFilter: 'all', activeTags: [],
      label: 'Trending', hint: 'movers vs 90d median · vol ≥ 10 · baseline ≥ 5p',
      columns: ['name', 'owned', 'sell_score', 'low_sell', 'medians_7d', 'delta_90d_pct', 'volume_48h', 'ratio'],
      defaultSort: { key: 'delta_90d_pct', dir: -1 },
      minVol: 10,
      minMedian: 5,
    },
    sets: {
      minPrice: 0, hideAtLvl: 11, typeFilter: 'all', activeTags: ['set'],
      label: 'Sets', hint: 'only set-tagged rows',
      columns: ['name', 'owned', 'sell_score', 'low_sell', 'top_buy', 'potential_plat'],
      defaultSort: { key: 'sell_score', dir: -1 },
    },
    vault: {
      minPrice: 0, hideAtLvl: 11, typeFilter: 'all', activeTags: [],
      label: 'Vaulted', hint: 'vaulted + vaulting-soon prime parts (sell before the cliff)',
      columns: ['name', 'owned', 'sell_score', 'low_sell', 'top_buy', 'volume_48h', 'potential_plat'],
      vaultOnly: true,
      defaultSort: { key: 'sell_score', dir: -1 },
    },
  };
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
  function applyPreset(name: string): void {
    const p = PRESETS[name];
    if (!p) return;
    minPrice = p.minPrice;
    hideAtLvl = p.hideAtLvl;
    typeFilter = p.typeFilter;
    activeTags = new Set(p.activeTags);
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
      const p = PRESETS[activePreset];
      if (!p) return;
      const matches =
        minPrice === p.minPrice &&
        hideAtLvl === p.hideAtLvl &&
        typeFilter === p.typeFilter &&
        activeTags.size === p.activeTags.length &&
        p.activeTags.every((t) => activeTags.has(t));
      if (!matches) activePreset = null;
    });
  });

  // Restore the last snapshot exactly once after mount. Using onMount (not
  // $effect) is critical: $effect tracks any state read inside its body as
  // a dependency, so writing `resolved` here and then reading it via
  // recomputeResults caused an infinite re-run loop.
  onMount(async () => {
    // Companion config (independent of inventory).
    companionConfig = loadCompanionConfig();
    if (companionConfig) verifyCompanion();

    // Restore the last inventory snapshot if there is one.
    const snap = loadSnapshot();
    if (!snap) return;
    try {
      inventoryName = snap.invName;
      lastUpdated = snap.ts;
      resolved = { owned: snap.owned, unresolved: {} };
      if (!market) market = await loadMarket();
      // No explicit recompute: the results $effect tracks resolved/market and
      // flushes before paint — the old call here just computed everything twice.
      phase = 'done';
    } catch (e) {
      console.error(e);
      error = e.message || String(e);
      phase = 'error';
    }
  });

  function handleClear() {
    clearSnapshot();
    inventoryName = null;
    lastUpdated = null;
    resolved = { owned: new Map(), unresolved: {} };
    deltas = new Map();
    results = [];
    phase = 'idle';
  }

  async function handleInventory({ name, data }) {
    // Encrypted exports route to the passphrase dialog instead of the
    // inventory-resolution pipeline.
    if (isEncrypted(data)) {
      openImportDialog(data);
      return;
    }
    inventoryName = name;
    phase = 'loading';
    error = null;
    try {
      if (!catalogs || !market) {
        [catalogs, market] = await Promise.all([
          catalogs ?? loadCatalogs(),
          market ?? loadMarket(),
        ]);
      }

      const keptLvls = extractKeptLvls(data);  // /Lotus/... → max lvl in Upgrades
      const owned = new Map();
      const unresolved = {};
      for (const { category: invCat, path, count } of flattenInventory(data)) {
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
          kept_lvl: null,
        };
        rec.count += count;
        // Carry forward the highest kept lvl across any inventory path
        // that resolved to the same slug+subtype (rare for mods — one path
        // per slug — but harmless for the relic refinement case).
        if (typeof keptLvl === 'number' && (rec.kept_lvl === null || keptLvl > rec.kept_lvl)) {
          rec.kept_lvl = keptLvl;
        }
        owned.set(key, rec);
      }
      // Diff against the previously-saved snapshot before overwriting it.
      const previous = loadSnapshot();
      deltas = diffOwned(previous?.owned, owned);
      resolved = { owned, unresolved };
      saveSnapshot({ invName: name, owned });
      lastUpdated = Date.now();

      results = computeResults(owned);
      phase = 'done';
    } catch (e) {
      console.error(e);
      error = e.message || String(e);
      phase = 'error';
    }
  }

  function computeResults(owned) {
    const out = [];
    for (const [key, rec] of owned) {
      const m = lookup(market, rec.slug);
      if (!m) continue;
      if (m.avg < minPrice) continue;
      if (rec.count < minOwned) continue;
      if (typeFilter !== 'all' && rec.type !== typeFilter) continue;
      // Hide rows where the user has a leveled-enough copy in `Upgrades`.
      // null kept_lvl = no individualised instance at all (always show).
      if (rec.kept_lvl !== null && rec.kept_lvl >= hideAtLvl) continue;
      // Tag chips — OR within the active set, AND with everything above.
      if (activeTags.size > 0) {
        const tags = m.tags || [];
        let any = false;
        for (const t of tags) { if (activeTags.has(t)) { any = true; break; } }
        if (!any) continue;
      }
      // Vault preset extra-filter: only rows whose part is vaulted or
      // about to be. Implicit-"available" rows drop out.
      if (PRESETS[activePreset]?.vaultOnly) {
        const status = market.vault_status?.[rec.slug];
        if (status !== 'vaulted' && status !== 'vaulting-soon') continue;
      }
      // Trending's liquidity floor: drop thin-volume rows so the Δ-sort
      // surfaces real movers, not median spikes (a fish whose 4p median
      // ticked to 48p reads as +1100% on ~1 trade).
      const presetMinVol = PRESETS[activePreset]?.minVol ?? 0;
      if (presetMinVol > 0 && (m.vol || 0) < presetMinVol) continue;
      // Baseline-price floor: wash trades fake volume AND avg, so the two
      // floors above don't catch penny-junk pumps (Goopolla: 1p fish pushed
      // to "12p", vol 47). The 90d-baseline median is the hardest number to
      // fake — it takes 45+ days of sustained manipulation to move it.
      const presetMinMedian = PRESETS[activePreset]?.minMedian ?? 0;
      if (presetMinMedian > 0 && (m.median_90d || 0) < presetMinMedian) continue;
      const { sell_score, patience } = scoreRow({ owned: rec.count, m });
      // ducats live on `m` because WFM is authoritative for the value —
      // warframestat's bulk /items/ endpoint doesn't carry it. Relics get
      // null so we don't suggest "Baro this" on a non-ducat trade.
      const ducats = rec.subtype ? null : (m.ducats ?? null);
      // p/100d — "platinum cost per 100 ducats of value." Low numbers
      // mean ducat-trading the part is the better deal vs selling it on
      // WFM. Null when no ducats data. Uses the clamped clearing price,
      // not raw low_sell — a single 1p troll ask made a stable 38p part
      // read as a "feed it to Baro" deal.
      const row_price = clearingPrice(m);
      const plat_per_100d = ducats && ducats > 0 && row_price > 0
        ? (row_price * 100) / ducats
        : null;
      // 90d trend signal. `median_90d` is what experienced WFM traders
      // price against (48h avg is noisy on low-volume items). We compute
      // Δ% vs the 90d median using the most recent daily median as
      // "now". Null when there's no series yet (CSV-only rebuilds
      // inherit zeros until the next full scrape).
      const medians = Array.isArray(m.medians_7d) ? m.medians_7d.filter(v => v > 0) : [];
      // "today" = latest daily median. Pre-split snapshots have no median_now,
      // so fall back to median_90d (which on those WAS the latest day).
      // `||` not `??`: a literal median_now of 0 is never a meaningful "today"
      // price (it's a thin item with no recent trade), so fall back to the 90d
      // baseline rather than null out the band + Δ signals entirely.
      const median_now = m.median_now || m.median_90d || null;
      // median_90d is now the 90-day BASELINE (median of the daily medians), so
      // Δ-vs-90d = today vs the 90-day norm — a real signal at last. On old
      // snapshots median_now === median_90d → Δ = 0 until the next scrape, which
      // is honest rather than fake.
      const median_90d = m.median_90d > 0 ? m.median_90d : null;
      const delta_90d_pct = median_now != null && median_90d != null && median_90d > 0
        ? ((median_now - median_90d) / median_90d) * 100
        : null;
      // Timing: where today's median sits in its 90-day band. Uses median_now,
      // not low_sell — the Donchian bands are built from the daily median
      // series, so a thin-book ask outlier (a lone 200p listing on a ~20p item)
      // would mislabel as "peak". median_now is always inside its own band.
      // "hold" = near the 90d low (don't dump into a trough — e.g. a mod Baro
      // just flooded), "peak" = near the 90d high (list now).
      const timing = bandSignal({
        price: median_now,
        donchTop: m.donch_top_90d,
        donchBot: m.donch_bot_90d,
        lowSell: m.low_sell,
        topBuy: m.top_buy,
      });
      const tags = Array.isArray(m.tags) ? m.tags : [];
      out.push({
        key,
        slug: rec.slug,
        subtype: rec.subtype ?? null,
        name: rec.name,
        owned: rec.count,
        type: rec.type,
        kept_lvl: rec.kept_lvl,
        ducats,
        plat_per_100d,
        avg_price: m.avg,
        low_sell: m.low_sell,
        top_buy: m.top_buy,
        volume_48h: m.vol,
        ratio: m.ratio,
        potential_plat: rec.count * m.avg,
        sell_score,
        patience,
        timing,
        medians_7d: medians,
        median_90d,
        delta_90d_pct,
        // Per-row metadata for the new chip / badge surfaces. `tags` is
        // already the source of truth for filter chips; passing it on
        // the row lets ResultsTable render an [Aug] pill without
        // re-looking-up the market entry. vault_status drives the
        // vault badge; absent = "available" implicitly.
        tags,
        is_augment: tags.includes('augment'),
        vault_status: market.vault_status?.[rec.slug] ?? null,
      });
    }
    out.sort((a, b) => b.sell_score - a.sell_score);
    return out;
  }

  // Re-derive results whenever any filter input or the owned set changes.
  // We deliberately read the filter state inside the effect (so they're
  // tracked) but write only to `results`, which the effect doesn't read —
  // no chance of a re-run loop.
  $effect(() => {
    minPrice; minOwned; typeFilter; hideAtLvl; activeTags.size;  // track filter changes
    if (resolved.owned.size && market) {        // track owned + market readiness
      results = computeResults(resolved.owned);
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
  let voidTrader = $derived(market?.baro ?? null);

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

  function humanWindow(ms) {
    if (ms == null || !Number.isFinite(ms) || ms < 0) return '—';
    const totalMin = Math.floor(ms / 60000);
    const d = Math.floor(totalMin / (60 * 24));
    const h = Math.floor((totalMin / 60) % 24);
    const m = totalMin % 60;
    if (d > 0) return `${d}d ${h}h`;
    if (h > 0) return `${h}h ${m}m`;
    return `${m}m`;
  }

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
    return deriveRelicPlan(resolved.owned, market, 3);
  });

  // Available tags = every tag that appears on a row surviving the OTHER
  // filters (price/owned/type/kept), with its live count. Empty chips
  // (count 0) are still rendered (strikethrough) so the user can see what
  // categories exist in their inventory rather than wondering where they
  // went. Sorted by count desc, then alphabetical.
  let availableTags = $derived.by(() => {
    if (!resolved.owned.size || !market) return [];
    const counts = new Map();
    // Mirror every filter clause `computeResults` applies — otherwise
    // chip counts overstate what clicking actually yields. Specifically
    // the vaultOnly preset clause was missed in the original derivation,
    // so "23 prime" would show but Vaulted preset + prime chip would
    // produce 6 rows.
    const vaultOnly = !!PRESETS[activePreset]?.vaultOnly;
    const minVol = PRESETS[activePreset]?.minVol ?? 0;
    const minMedianFloor = PRESETS[activePreset]?.minMedian ?? 0;
    for (const rec of resolved.owned.values()) {
      const m = lookup(market, rec.slug);
      if (!m) continue;
      if (m.avg < minPrice) continue;
      if (rec.count < minOwned) continue;
      if (typeFilter !== 'all' && rec.type !== typeFilter) continue;
      if (rec.kept_lvl !== null && rec.kept_lvl >= hideAtLvl) continue;
      if (vaultOnly) {
        const status = market.vault_status?.[rec.slug];
        if (status !== 'vaulted' && status !== 'vaulting-soon') continue;
      }
      if (minVol > 0 && (m.vol || 0) < minVol) continue;
      if (minMedianFloor > 0 && (m.median_90d || 0) < minMedianFloor) continue;
      for (const t of (m.tags || [])) {
        counts.set(t, (counts.get(t) || 0) + 1);
      }
    }
    return [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  });

  function toggleTag(tag) {
    const next = new Set(activeTags);
    if (next.has(tag)) next.delete(tag); else next.add(tag);
    activeTags = next;
  }

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

  let unresolvedSummary = $derived(
    Object.entries(resolved.unresolved)
      .map(([k, v]) => `${k}:${v}`)
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

  // Coarse freshness bucket for the small status dot next to "market Xh ago".
  // green ≤ 6h, amber ≤ 24h, red after that. Matches our 2h scrape cadence
  // so a healthy snapshot is always green.
  let marketFreshness = $derived.by(() => {
    if (!market?.updated_at) return 'unknown';
    const h = (Date.now() - new Date(market.updated_at)) / 3.6e6;
    if (h <= 6) return 'fresh';
    if (h <= 24) return 'aging';
    return 'stale';
  });

  // Total theoretical plat across visible results — for the stats strip.
  let totalPotential = $derived(
    results.reduce((s, r) => s + r.potential_plat, 0)
  );

  // Friendly diagnosis of WHY the table is empty so we don't just shrug.
  let emptyReason = $derived.by(() => {
    if (results.length > 0 || !resolved.owned.size) return null;
    // Walk the same filter logic but count what each restriction excludes.
    let candidates = 0, byPrice = 0, byOwned = 0, byType = 0, byKept = 0;
    for (const rec of resolved.owned.values()) {
      const m = lookup(market, rec.slug);
      if (!m) continue;
      candidates += 1;
      if (m.avg < minPrice) byPrice += 1;
      if (rec.count < minOwned) byOwned += 1;
      if (typeFilter !== 'all' && rec.type !== typeFilter) byType += 1;
      if (rec.kept_lvl !== null && rec.kept_lvl >= hideAtLvl) byKept += 1;
    }
    if (candidates === 0) return { kind: 'no-market', candidates };
    const top = [
      ['price', byPrice],
      ['owned', byOwned],
      ['type',  byType],
      ['kept',  byKept],
    ].sort((a, b) => b[1] - a[1])[0];
    return { kind: top[0], excluded: top[1], candidates };
  });

  // One-shot quick-fix actions the empty state can offer.
  function relaxFilters({ kind }) {
    if (kind === 'price') minPrice = 1;
    if (kind === 'owned') minOwned = 1;
    if (kind === 'type')  typeFilter = 'all';
    if (kind === 'kept')  hideAtLvl = 11;
  }

  // Hidden file input we trigger from the "Replace inventory" button. Using
  // the same handler as the drop-zone keeps both paths identical.
  let hiddenFileInput;
  function openFilePicker() {
    hiddenFileInput?.click();
  }
  async function onHiddenPicked(e) {
    const file = e.target.files?.[0];
    if (!file) return;
    try {
      const text = await file.text();
      const data = JSON.parse(text);
      await handleInventory({ name: file.name, data });
    } catch (err) {
      error = `Couldn't parse ${file.name}: ${err.message}`;
      phase = 'error';
    } finally {
      // Reset so picking the same file twice still fires change.
      e.target.value = '';
    }
  }

  // ---- Encrypted export / import ---------------------------------------
  // Path-of-Building style: passphrase-derived AES-GCM, no accounts. The
  // exported file decrypts back into the same {invName, owned} the UI
  // restores from localStorage on page load.
  let exportDialog;
  let importDialog;
  let exportPass = $state('');
  let exportConfirm = $state('');
  let exportBusy = $state(false);
  let importPass = $state('');
  let importBlob = $state(null);
  let importBusy = $state(false);
  let cryptoError = $state(null);

  // ---- Companion connection state ----
  let companionConfig = $state(null);          // {baseUrl, token} | null
  let companionStatus = $state('unchecked');   // unchecked | connecting | connected | error
  let companionPlatform = $state(null);
  let companionError = $state(null);
  let companionInput = $state('');
  let listingOpen = $state(false);

  // Sidebar nav: if the user's persisted view is unavailable (Baro not
  // visiting, companion not connected), fall back to Sell rather than
  // rendering an empty pane. The nav itself hides those entries; this
  // protects against a stale localStorage value or in-session state
  // change (e.g. companion disconnects while user is on Orders).
  let effectiveView = $derived.by<View>(() => {
    if (view === 'baro' && !showBaroCard) return 'sell';
    if (view === 'orders' && companionStatus !== 'connected') return 'sell';
    return view;
  });

  async function verifyCompanion() {
    if (!companionConfig) return;
    companionStatus = 'connecting';
    companionError = null;
    try {
      const r = await pingCompanion(companionConfig);
      companionPlatform = r?.platform ?? null;
      companionStatus = 'connected';
      // Poll once for an interrupted batch so the user gets a Resume option
      // without having to dig anywhere. Best-effort — a network blip here
      // shouldn't break the connect flow.
      try {
        pendingPlan = await getPendingPlan(companionConfig);
      } catch { pendingPlan = null; }
    } catch (e) {
      companionStatus = 'error';
      companionError = e.message || String(e);
    }
  }

  // ---- Pending-plan recovery ----
  let pendingPlan = $state(null);          // {plan_id, started_at, items[]} | null
  let resumePhase = $state('idle');        // idle | running | done | error
  let resumeError = $state(null);
  let resumeResults = $state([]);

  let pendingRemaining = $derived(
    pendingPlan?.items?.filter((i) => i.status === 'pending').length ?? 0
  );
  let pendingDone = $derived(
    pendingPlan?.items?.filter((i) => i.status === 'ok').length ?? 0
  );

  async function doResume() {
    if (!companionConfig) return;
    resumePhase = 'running';
    resumeError = null;
    try {
      const resp = await resumePendingPlan(companionConfig);
      resumeResults = resp?.results ?? [];
      resumePhase = 'done';
      pendingPlan = null;
    } catch (e) {
      resumePhase = 'error';
      resumeError = e.message || String(e);
    }
  }

  async function doDiscard() {
    if (!companionConfig) return;
    try { await discardPendingPlan(companionConfig); } catch { /* ignore */ }
    pendingPlan = null;
    resumePhase = 'idle';
    resumeResults = [];
  }

  let resumeOk = $derived(resumeResults.filter((r) => r.status === 'ok').length);
  let resumeErr = $derived(resumeResults.filter((r) => r.status !== 'ok').length);

  async function connectCompanion() {
    companionError = null;
    try {
      const cfg = parseCompanionUrl(companionInput);
      companionConfig = cfg;
      saveCompanionConfig(cfg);
      companionInput = '';
      await verifyCompanion();
    } catch (e) {
      companionStatus = 'error';
      companionError = e.message || String(e);
    }
  }

  function disconnectCompanion() {
    clearCompanionConfig();
    companionConfig = null;
    companionStatus = 'unchecked';
    companionPlatform = null;
    pendingPlan = null;
    resumePhase = 'idle';
    resumeResults = [];
  }

  function openExportDialog() {
    cryptoError = null;
    exportPass = '';
    exportConfirm = '';
    exportDialog?.showModal();
  }

  async function performExport(e) {
    e?.preventDefault();
    cryptoError = null;
    if (exportPass !== exportConfirm) {
      cryptoError = "Passphrases don't match.";
      return;
    }
    if (exportPass.length < 4) {
      cryptoError = 'Passphrase must be at least 4 characters.';
      return;
    }
    exportBusy = true;
    try {
      const payload = {
        invName: inventoryName,
        ts: lastUpdated,
        owned: [...resolved.owned.entries()].map(([key, rec]) => [
          key,
          {
            count: rec.count,
            name: rec.name,
            type: rec.type,
            slug: rec.slug,
            subtype: rec.subtype ?? null,
            kept_lvl: rec.kept_lvl ?? null,
          },
        ]),
      };
      const blob = await encryptPayload(payload, exportPass);
      const text = JSON.stringify(blob);
      const file = new Blob([text], { type: 'application/json' });
      const url = URL.createObjectURL(file);
      const a = document.createElement('a');
      const stamp = new Date().toISOString().slice(0, 10);
      a.href = url;
      a.download = `wfminv-${stamp}.json`;
      document.body.appendChild(a);
      a.click();
      a.remove();
      URL.revokeObjectURL(url);
      exportDialog?.close();
    } catch (err) {
      cryptoError = err.message || String(err);
    } finally {
      exportBusy = false;
    }
  }

  function openImportDialog(blob) {
    cryptoError = null;
    importPass = '';
    importBlob = blob;
    importDialog?.showModal();
  }

  async function performImport(e) {
    e?.preventDefault();
    cryptoError = null;
    importBusy = true;
    try {
      const payload = await decryptPayload(importBlob, importPass);
      if (!Array.isArray(payload?.owned)) {
        throw new Error('Decrypted file is missing the owned-items array.');
      }
      // Hydrate the same way onMount/localStorage restoration does. Old
      // (pre-subtype) exports stored the slug as the map key and lacked
      // rec.slug / rec.subtype — backfill from the key so they still load.
      inventoryName = payload.invName || 'imported.json';
      lastUpdated = payload.ts || Date.now();
      const ownedMap = new Map(
        payload.owned.map(([key, rec]) => [
          key.includes('|') ? key : `${key}|`,
          {
            ...rec,
            slug: rec.slug ?? (key.includes('|') ? key.split('|')[0] : key),
            subtype: rec.subtype ?? null,
          },
        ])
      );
      deltas = diffOwned(loadSnapshot()?.owned, ownedMap);
      resolved = { owned: ownedMap, unresolved: {} };
      if (!market) market = await loadMarket();
      saveSnapshot({ invName: inventoryName, owned: ownedMap });
      phase = 'done';
      importDialog?.close();
    } catch (err) {
      cryptoError = err.message || String(err);
    } finally {
      importBusy = false;
    }
  }
</script>

{#if phase !== 'done'}
<main class="landing">
  <header>
    <h1>WF inventory · market check</h1>
    <p class="sub">
      Drop your <code>inventory.json</code>. We cross-reference against a
      warframe.market snapshot (refreshed centrally on a schedule) and rank
      what's worth selling. Everything happens in your browser — your inventory
      never leaves the page.
    </p>
  </header>

  {#if phase === 'idle' || phase === 'loading'}
    <ol class="steps">
      <li>
        <span class="n">01</span>
        <div class="body">
          <h3>Get the companion</h3>
          <p>
            Tiny <a href="#companion">CLI binary</a>. Reads the running
            game's process memory to fetch your inventory from DE's own
            endpoint. Nothing else.
          </p>
        </div>
      </li>
      <li>
        <span class="n">02</span>
        <div class="body">
          <h3>Run it once</h3>
          <p>With Warframe past the login screen, run:</p>
          <pre class="snippet"><code>wfm-fetch-inventory</code></pre>
          <p class="muted">
            Writes <code>inventory.json</code> to your Downloads folder.
            Windows: no admin needed. Linux: grant ptrace once
            (<code>sudo setcap cap_sys_ptrace=eip ~/.local/bin/wfm-fetch-inventory</code>)
            and it runs without sudo forever.
          </p>
        </div>
      </li>
      <li>
        <span class="n">03</span>
        <div class="body">
          <h3>Drop it below</h3>
          <p>
            The file resolves against a cached warframe.market snapshot
            (refreshed every 2 h) and ranks what's worth selling.
          </p>
          <p class="muted">Everything runs locally. Nothing's uploaded.</p>
        </div>
      </li>
    </ol>

    <DropZone
      oninventory={handleInventory}
      loading={phase === 'loading'}
    />
  {/if}

  {#if phase === 'error'}
    <div class="card error">Error: {error}</div>
  {/if}

  <InstallWidget />

  {@render faqContent()}

  <footer>
    Open source · MIT · data from warframe.market and warframestat.us
  </footer>
</main>
{:else}
<div class="shell">

  <aside class="sidebar">
    <div class="brand">
      <h1>WF · market check</h1>
      <div class="sub">Windows + Linux · no Overwolf</div>
    </div>

    <nav>
      <div class="nav-group">
        <div class="nav-label">Trade</div>
        <button type="button" class="nav-item" class:active={effectiveView === 'sell'} onclick={() => setView('sell')}>
          <span>Sell</span>
          <span class="badge">{results.length}</span>
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

      <div class="nav-group">
        <div class="nav-label">Manage</div>
        {#if companionStatus === 'connected'}
          <button type="button" class="nav-item" class:active={effectiveView === 'orders'} onclick={() => setView('orders')}>
            <span>My orders</span>
            {#if pendingPlan && pendingRemaining > 0}<span class="badge warn">{pendingRemaining}</span>{/if}
          </button>
        {/if}
        <button type="button" class="nav-item" class:active={effectiveView === 'companion'} onclick={() => setView('companion')}>
          <span>Companion</span>
          <span class="dot {companionStatus === 'connected' ? 'fresh' : companionStatus === 'error' ? 'stale' : ''}" aria-hidden="true"></span>
        </button>
      </div>

      <div class="nav-group">
        <div class="nav-label">Library</div>
        <button type="button" class="nav-item" class:active={effectiveView === 'install'} onclick={() => setView('install')}>
          <span>Install · FAQ</span>
        </button>
      </div>
    </nav>

    <div class="src-pin">
      <strong>{inventoryName}</strong>
      {#if inventoryStaleness}
        <span class="muted small">saved {inventoryStaleness}</span>
      {/if}
      {#if unresolvedSummary}
        <span class="muted small" title="Item paths warframestat.us couldn't resolve. Usually quest items, resources, and very new content.">
          unresolved {unresolvedSummary}
        </span>
      {/if}
      <div class="src-pin-actions">
        <button onclick={openFilePicker} title="Drop or pick a new inventory.json. Counts that changed will be highlighted.">Replace</button>
        <button class="ghost" onclick={openExportDialog} title="Download an encrypted snapshot for another device or backup.">Export</button>
        <button class="ghost" onclick={handleClear} title="Forget the saved inventory entirely.">Clear</button>
      </div>
    </div>
  </aside>

  <main class="workspace">

    {#if effectiveView === 'sell'}
      <section class="view-header">
        <h2>Sell</h2>
        <p class="lede">Items in your inventory worth listing right now, ranked by sell score.</p>
      </section>

      <section class="stats">
        <div class="stat">
          <span class="k">Owned</span>
          <span class="v">{resolved.owned.size.toLocaleString()}</span>
        </div>
        <div class="stat">
          <span class="k">Sellable</span>
          <span class="v">{results.length.toLocaleString()}</span>
        </div>
        <div class="stat">
          <span class="k">Potential</span>
          <span class="v">
            {totalPotential.toLocaleString(undefined, { maximumFractionDigits: 0 })}
            <span class="unit">p</span>
          </span>
        </div>
        <div class="stat right">
          <span class="k">
            <span class="dot {marketFreshness}" aria-hidden="true"></span>
            Market data
          </span>
          <span class="v small">{marketStaleness ?? '—'}</span>
        </div>
      </section>

      <section class="card">
        <div class="row presets-row">
          {#each Object.entries(PRESETS) as [name, preset]}
            <button
              type="button"
              class="preset"
              class:active={activePreset === name}
              onclick={() => applyPreset(name)}
              title={preset.hint}
            >{preset.label}</button>
          {/each}
          <span class="muted preset-hint">
            {activePreset ? PRESETS[activePreset].hint : 'custom — saved preset cleared'}
          </span>
          {#if companionStatus === 'connected'}
            <button
              class="list-cta"
              onclick={() => (listingOpen = true)}
              disabled={results.length === 0}
              title="Open the review modal for the current filtered rows"
            >List {Math.min(results.length, 50)} on WFM</button>
          {/if}
        </div>
        {#if availableTags.length > 0}
          <div class="row tagchips">
            {#each availableTags as [tag, count]}
              <button
                type="button"
                class="chip"
                class:active={activeTags.has(tag)}
                class:zero={count === 0}
                onclick={() => toggleTag(tag)}
                title={count === 0 ? `No matching rows pass the other filters` : `${count} row${count === 1 ? '' : 's'} carry this tag`}
              >
                {tag}
                <span class="chip-count">{count}</span>
              </button>
            {/each}
            {#if activeTags.size > 0}
              <button type="button" class="chip-clear" onclick={() => (activeTags = new Set())}>
                clear ({activeTags.size})
              </button>
            {/if}
          </div>
        {/if}
        <details class="filter-disclosure" open={filtersOpen} ontoggle={toggleFiltersOpen}>
          <summary>
            <span class="dis-label">Filters</span>
            <span class="muted small">price · owned · type · ranked-mods threshold</span>
          </summary>
          <div class="row">
            <div class="filters">
              <label>
                Min avg price
                <input type="number" bind:value={minPrice} min="0" step="1" style="width:60px" />
                <span class="muted">p</span>
              </label>
              <label title="Hides items you own fewer copies of than this. Set to 2 to keep one of each for yourself.">
                Min owned
                <input type="number" bind:value={minOwned} min="1" step="1" style="width:50px" />
              </label>
              <label>
                Type
                <select bind:value={typeFilter}>
                  <option value="all">All</option>
                  {#each availableTypes as t}
                    <option value={t}>{t}</option>
                  {/each}
                </select>
              </label>
              <label title="Hides a row when you have a copy of that mod in `Upgrades` at this rank or higher. 5 ≈ regular maxed (most mods cap at lvl 5). 10 ≈ only Primed/Galvanized maxed. 0 ≈ also hide unranked instances (e.g. rivens). 11 disables the filter.">
                Hide if ranked ≥
                <input type="number" bind:value={hideAtLvl} min="0" max="11" step="1" style="width:55px" />
              </label>
            </div>
          </div>
        </details>
      </section>

      {@render pendingBanner()}

      {#if results.length > 0 && !scoreExplainerDismissed}
        <div class="score-explainer">
          <strong>About the “Score” column.</strong>
          Expected plat <strong>per day</strong> if you listed everything —
          <code>min(owned, vol_48h / 2) × low_sell</code>.
          Higher = better, uncapped. Items below 2 trades / 48 h get a
          “patience” tag instead of a near-zero score.
          Click <code>?</code> on any column header for the same kind of
          explainer.
          <button class="dismiss" onclick={dismissScoreExplainer} aria-label="Dismiss">×</button>
        </div>
      {/if}

      {#if results.length > 0}
        <ResultsTable {results} {deltas} {visibleColumns} {presetSort} />
      {:else if emptyReason}
        <div class="card empty">
          {#if emptyReason.kind === 'no-market'}
            <div>
              <strong>Nothing in this inventory has live market data.</strong>
              <p class="muted">
                Either nothing here is tradeable, or your market snapshot is
                empty. Check that <code>market.json</code> looks healthy
                ({marketStaleness ?? 'never updated'}).
              </p>
            </div>
          {:else if emptyReason.kind === 'price'}
            <div>
              <strong>{emptyReason.excluded} sellable items are under {minPrice}p average.</strong>
              <p class="muted">Lower the price threshold to see them.</p>
            </div>
            <button onclick={() => relaxFilters({ kind: 'price' })}>Drop min price to 1p</button>
          {:else if emptyReason.kind === 'owned'}
            <div>
              <strong>{emptyReason.excluded} items you own are below the “owned” threshold ({minOwned}).</strong>
              <p class="muted">Most are 1-of-a-kind — set min-owned to 1 to include them.</p>
            </div>
            <button onclick={() => relaxFilters({ kind: 'owned' })}>Set min owned to 1</button>
          {:else if emptyReason.kind === 'type'}
            <div>
              <strong>Nothing in your inventory matches type “{typeFilter}”.</strong>
              <p class="muted">Switch back to All to see everything.</p>
            </div>
            <button onclick={() => relaxFilters({ kind: 'type' })}>Show all types</button>
          {:else if emptyReason.kind === 'kept'}
            <div>
              <strong>All {emptyReason.excluded} candidates have a copy you've ranked to {hideAtLvl}+ in <code>Upgrades</code>.</strong>
              <p class="muted">Raise the threshold (or set 11 to disable) to see them.</p>
            </div>
            <button onclick={() => relaxFilters({ kind: 'kept' })}>Disable the rank filter</button>
          {/if}
        </div>
      {/if}

    {:else if effectiveView === 'sets'}
      <section class="view-header">
        <h2>Set picks</h2>
        <p class="lede">
          Inventory cross-referenced against {Object.keys(market?.set_to_parts ?? {}).length}
          prime sets. Ranked by net plat.
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
                    href="https://warframe.market/items/{r.set_slug}"
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
        <p class="lede">Top {relicPlan.length} relics you own by expected plat per crack (Intact state).</p>
      </section>
      {#if relicPlan.length > 0}
        <section class="card relic-planner">
          <div class="relic-grid">
            {#each relicPlan as p (p.relic_slug)}
              <div class="relic-card">
                <div class="relic-title">
                  <strong class="reco-verb">Crack</strong>
                  <a
                    href="https://warframe.market/items/{p.relic_slug}"
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

    {:else if effectiveView === 'orders'}
      <section class="view-header">
        <h2>My orders</h2>
        <p class="lede">Your active warframe.market listings, fetched live from the companion.</p>
      </section>
      {@render pendingBanner()}
      <MyOrdersPanel config={companionConfig} />

    {:else if effectiveView === 'companion'}
      <section class="view-header">
        <h2>Companion</h2>
        <p class="lede">
          The loopback CLI that reads your inventory and relays listing actions to WFM.
          {#if companionStatus !== 'connected'}Required to list on WFM.{/if}
        </p>
      </section>
      <section class="card companion-strip">
        {#if companionStatus === 'connected'}
          <div class="row">
            <div class="src">
              <span class="dot fresh" aria-hidden="true"></span>
              <strong>Connected</strong>
              <span class="muted">· {companionPlatform ?? 'pc'} · {companionConfig.baseUrl}</span>
            </div>
            <div class="row gap-sm">
              <button
                onclick={() => (listingOpen = true)}
                disabled={results.length === 0}
                title="Open the review modal for the current filtered rows"
              >List {Math.min(results.length, 50)} on WFM</button>
              <button class="ghost" onclick={disconnectCompanion}>Disconnect</button>
            </div>
          </div>
        {:else}
          <div class="row">
            <div class="src">
              <strong>Not connected</strong>
              <span class="muted">
                · paste the URL the <code>serve</code> subcommand printed
                {#if companionStatus === 'error'}(<span class="bad">{companionError}</span>){/if}
              </span>
            </div>
            <div class="row gap-sm">
              <input
                type="text"
                placeholder="http://127.0.0.1:XXXXX?token=…"
                bind:value={companionInput}
                style="min-width:320px;font-family:ui-monospace,Menlo,monospace;font-size:12px"
              />
              <button onclick={connectCompanion} disabled={!companionInput.trim()}>
                {companionStatus === 'connecting' ? 'Checking…' : 'Connect'}
              </button>
            </div>
          </div>
        {/if}
      </section>

    {:else if effectiveView === 'install'}
      <section class="view-header">
        <h2>Install · FAQ</h2>
        <p class="lede">Getting the companion + answers to common questions.</p>
      </section>
      <InstallWidget />
      {@render faqContent()}
    {/if}

  </main>
</div>
{/if}

{#snippet faqContent()}
  <section class="faq">
    <h2>FAQ</h2>

    <details>
      <summary>Is this safe? Can I get banned?</summary>
      <p>
        The companion reads the running game's process memory to find the
        <code>accountId</code> and <code>nonce</code> your client already
        obtained at login, then calls DE's own inventory endpoint with
        those — same call your game client makes. It writes to disk; it
        doesn't write to the game's memory or modify game state.
      </p>
      <p>
        EAC (the anti-cheat) targets memory <em>writes</em> and known cheat
        signatures, not read-only inspection. Equivalent tools (Sainan's
        warframe-api-helper, AlecaFrame via Overwolf) have been used for
        years with no documented bans. <strong>That's not a guarantee.</strong>
        DE has not formally blessed this category. Use at your own risk;
        we accept none.
      </p>
    </details>

    <details>
      <summary>Where does my inventory data actually go?</summary>
      <p>
        Nowhere we control. The companion writes <code>inventory.json</code>
        to your <code>~/Downloads</code>. The browser app processes that
        file locally — every byte stays in your tab. We persist a copy in
        your browser's storage (localStorage + IndexedDB) so a refresh
        doesn't wipe it. The market snapshot is the only thing we host,
        and it's the same for every visitor.
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
      <summary>Can I sync between desktop and laptop?</summary>
      <p>
        There are no accounts. Use the built-in <strong>Export</strong>
        button in the sidebar: it produces a file encrypted with a
        passphrase only you hold (AES-256-GCM, PBKDF2 600k), which you
        can drop into the app on any other device. Or simply copy your
        <code>inventory.json</code> across / run the companion on each
        device.
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
  </section>
{/snippet}

{#snippet pendingBanner()}
  {#if companionStatus === 'connected' && (pendingPlan || resumePhase !== 'idle')}
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
              New listings are still invisible — toggle from the orders panel.
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

<input
  bind:this={hiddenFileInput}
  type="file"
  accept="application/json,.json"
  onchange={onHiddenPicked}
  style="display:none"
/>

<ListingReviewModal
  bind:open={listingOpen}
  rows={results.slice(0, 50)}
  config={companionConfig}
/>

<dialog bind:this={exportDialog} class="cryptobox">
  <form onsubmit={performExport}>
    <header>
      <h3>Export encrypted snapshot</h3>
      <p class="muted">
        Saves your resolved inventory as an encrypted JSON file. Decrypt on
        another device with the same passphrase. Nothing leaves your browser.
      </p>
    </header>
    <label>
      Passphrase
      <input
        type="password"
        autocomplete="new-password"
        bind:value={exportPass}
        placeholder="something only you'd type"
        required
        minlength="4"
        autofocus
      />
    </label>
    <label>
      Confirm
      <input
        type="password"
        autocomplete="new-password"
        bind:value={exportConfirm}
        required
        minlength="4"
      />
    </label>
    {#if cryptoError}
      <div class="err">{cryptoError}</div>
    {/if}
    <footer>
      <button type="button" class="ghost" onclick={() => exportDialog?.close()}>Cancel</button>
      <button type="submit" disabled={exportBusy}>{exportBusy ? 'Encrypting…' : 'Download'}</button>
    </footer>
  </form>
</dialog>

<dialog bind:this={importDialog} class="cryptobox">
  <form onsubmit={performImport}>
    <header>
      <h3>Decrypt snapshot</h3>
      <p class="muted">
        This looks like an encrypted wfminv snapshot. Enter the passphrase you
        used when exporting it.
      </p>
    </header>
    <label>
      Passphrase
      <input
        type="password"
        autocomplete="current-password"
        bind:value={importPass}
        required
        minlength="4"
        autofocus
      />
    </label>
    {#if cryptoError}
      <div class="err">{cryptoError}</div>
    {/if}
    <footer>
      <button type="button" class="ghost" onclick={() => importDialog?.close()}>Cancel</button>
      <button type="submit" disabled={importBusy}>{importBusy ? 'Decrypting…' : 'Decrypt'}</button>
    </footer>
  </form>
</dialog>

<style>
  main.landing {
    /* Landing screen — the WF inventory pitch, three-step intro, dropzone,
       installer widget, FAQ. Centred, narrow, single-column. */
    max-width: min(900px, calc(100vw - 32px));
    margin: 0 auto;
    padding: 32px 24px 48px;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  /* Shell layout: persistent left rail + workspace column. Sidebar is
     220px (room for nav-item label + 3-digit badge); workspace fills
     the rest. Shell caps at 1720 (220 + 1500) so the 13-column table
     can still breathe on ultra-wide displays. */
  .shell {
    display: grid;
    grid-template-columns: 220px 1fr;
    max-width: min(1720px, 100vw);
    margin: 0 auto;
    min-height: 100vh;
  }

  main.workspace {
    padding: 24px 28px 48px;
    display: flex;
    flex-direction: column;
    gap: 16px;
    min-width: 0;
  }

  /* Sidebar — sticky to viewport; nav scrolls if it overflows.
     `aside` is the persistent navigation surface; src-pin is the inventory
     metadata footer (replace/export/clear). */
  aside.sidebar {
    position: sticky;
    top: 0;
    height: 100vh;
    border-right: 1px solid var(--border);
    background: var(--panel);
    display: flex;
    flex-direction: column;
    padding: 18px 0;
  }
  aside.sidebar .brand {
    padding: 0 18px 14px;
    border-bottom: 1px solid var(--border);
    margin-bottom: 10px;
  }
  aside.sidebar .brand h1 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    letter-spacing: -0.005em;
  }
  aside.sidebar .brand .sub {
    color: var(--muted);
    font-size: 11px;
    margin-top: 2px;
  }
  aside.sidebar nav {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }
  .nav-group {
    padding: 6px 0;
    border-bottom: 1px solid var(--border);
  }
  .nav-group:last-child { border-bottom: none; }
  .nav-label {
    font-size: 10px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
    padding: 8px 18px 4px;
  }
  .nav-item {
    /* Native <button> reset → flat, full-width, transparent. The accent
       left border + tinted background mark the active view. */
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 6px 18px;
    font: inherit;
    font-size: 13px;
    color: var(--muted);
    background: transparent;
    border: none;
    border-left: 2px solid transparent;
    text-align: left;
    cursor: pointer;
  }
  .nav-item:hover { color: var(--fg); background: rgba(255,255,255,0.02); }
  .nav-item.active {
    color: var(--fg);
    border-left-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 6%, transparent);
  }
  .nav-item .badge {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 10px;
    color: var(--muted);
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-radius: 3px;
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

  /* Sidebar footer — inventory source + Replace/Export/Clear. Pinned to
     bottom via margin-top:auto on the nav above. */
  .src-pin {
    padding: 12px 18px;
    border-top: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .src-pin strong {
    font-size: 12px;
    font-weight: 600;
    word-break: break-all;
  }
  .src-pin .small { font-size: 11px; }
  .src-pin-actions {
    display: flex;
    gap: 6px;
    margin-top: 6px;
    flex-wrap: wrap;
  }
  .src-pin-actions button {
    font: inherit;
    font-size: 11px;
    color: var(--muted);
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 3px 8px;
    cursor: pointer;
  }
  .src-pin-actions button:hover { color: var(--fg); border-color: var(--accent); }

  /* Workspace view header — h2 + lede paragraph. The lede gives one
     sentence of context about what this view does so the user lands
     without re-reading the docs. */
  .view-header h2 {
    font-size: 20px;
    font-weight: 600;
    text-transform: none;
    letter-spacing: -0.01em;
    color: var(--fg);
    margin: 0;
  }
  .view-header .lede {
    color: var(--muted);
    font-size: 13px;
    margin: 4px 0 0;
    max-width: 64ch;
  }

  /* Primary CTA inside the presets row — pushed to the far right via
     margin-left:auto so it doesn't visually mix with the chip-style
     presets next to it. Same colour family as the accent. */
  .list-cta {
    margin-left: auto;
    background: var(--accent);
    color: var(--bg);
    border: 1px solid var(--accent);
    font-weight: 600;
  }
  .list-cta:hover:not(:disabled) { filter: brightness(1.1); }
  .list-cta:disabled { opacity: 0.4; cursor: not-allowed; }

  /* Mobile: stack the shell. Sidebar becomes a horizontal scroll strip
     at the top; src-pin moves under the nav and shows the inventory name
     inline. Below ~900px the 220px rail eats too much of the workspace,
     so the grid collapses. */
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
      border-right: 1px solid var(--border);
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
    .src-pin {
      flex-direction: row;
      flex-wrap: wrap;
      align-items: center;
      gap: 8px 12px;
      padding: 8px 16px;
    }
    .src-pin-actions { margin-top: 0; }
    main.workspace { padding: 16px 16px 32px; }
  }
  header h1 {
    margin: 0;
    font-size: 22px;
    font-weight: 600;
    letter-spacing: -0.015em;
  }
  h2 { margin: 0 0 4px 0; font-size: 14px; font-weight: 600; letter-spacing: 0.04em; text-transform: uppercase; color: var(--muted); }
  h3 { margin: 0 0 4px 0; font-size: 14px; font-weight: 600; }
  pre {
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 10px 12px;
    overflow-x: auto;
    font-size: 12.5px;
    margin: 0;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  pre code { background: transparent; padding: 0; }
  .sub { color: var(--muted); margin: 6px 0 0 0; max-width: 64ch; font-size: 13px; }

  /* "How this works" — three numbered steps. Asymmetric: large outlined number,
     compact body. Steps separated by hairlines, not boxes. */
  .steps {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 0;
    list-style: none;
    padding: 0;
    margin: 0;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 10px;
    overflow: hidden;
  }
  .steps li {
    padding: 18px 20px;
    display: flex;
    gap: 14px;
    align-items: flex-start;
    border-right: 1px solid var(--border);
  }
  .steps li:last-child { border-right: none; }
  .steps .n {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 13px;
    letter-spacing: 0.05em;
    color: var(--accent);
    font-weight: 600;
    padding-top: 2px;
  }
  .steps .body { min-width: 0; flex: 1; }
  .steps .body p {
    margin: 0;
    font-size: 13px;
    color: var(--fg);
    line-height: 1.45;
  }
  .steps .body p + p { margin-top: 6px; }
  .steps .body p.muted { color: var(--muted); font-size: 12px; }
  .steps .snippet { margin: 6px 0; font-size: 12px; padding: 6px 10px; }
  @media (max-width: 760px) {
    .steps { grid-template-columns: 1fr; }
    .steps li { border-right: none; border-bottom: 1px solid var(--border); }
    .steps li:last-child { border-bottom: none; }
  }

  /* Stats strip — big numbers, tiny labels. Number-first hierarchy. */
  .stats {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 1px;
    background: var(--border);
    border: 1px solid var(--border);
    border-radius: 10px;
    overflow: hidden;
  }
  .stat {
    background: var(--panel);
    padding: 14px 18px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .stat .k {
    font-size: 11px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--muted);
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .stat .v {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 22px;
    font-weight: 600;
    letter-spacing: -0.01em;
    line-height: 1.1;
  }
  .stat .v.small { font-size: 14px; font-weight: 500; }
  .stat .v .unit { font-size: 13px; color: var(--muted); margin-left: 2px; }
  .stat.right .k, .stat.right .v { justify-content: flex-end; text-align: right; }
  @media (max-width: 760px) {
    .stats { grid-template-columns: repeat(2, 1fr); }
  }

  /* Freshness dot: green/amber/red signal, tuned for our 2 h scrape cadence. */
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--muted);
    display: inline-block;
  }
  .dot.fresh   { background: var(--good); box-shadow: 0 0 6px color-mix(in srgb, var(--good) 60%, transparent); }
  .dot.aging   { background: var(--warn); }
  .dot.stale   { background: var(--bad); }

  /* Card / row scaffolding */
  .card {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 10px;
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
  .muted { color: var(--muted); font-size: 12.5px; }
  .filters { display: flex; gap: 14px; align-items: center; flex-wrap: wrap; }

  /* Preset pills — one-click filter configurations above the chips. The
     active pill is accent-bordered. A subtle hint string trails the row
     to let the user know what the current selection emphasises. */
  .presets-row { gap: 8px; flex-wrap: wrap; }
  .preset {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--muted);
    border-radius: 999px;
    padding: 4px 14px;
    font-size: 12px;
    letter-spacing: 0.02em;
    cursor: pointer;
    transition: color 120ms, border-color 120ms, background 120ms;
    font: inherit;
    font-size: 12px;
  }
  .preset:hover { color: var(--fg); border-color: var(--accent); }
  .preset.active {
    color: var(--accent);
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }
  .preset-hint { margin-left: auto; font-size: 11.5px; }

  /* Filters disclosure — collapses the rail's numeric inputs behind a
     single summary line. Visible by default: search (in the table) +
     tag chips above. Open state persists in localStorage so power users
     don't re-click every session. */
  .filter-disclosure {
    border-top: 1px solid var(--border);
    padding-top: 10px;
    margin-top: 2px;
  }
  .filter-disclosure > summary {
    cursor: pointer;
    list-style: none;
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 12px;
    color: var(--muted);
    padding: 2px 0;
    user-select: none;
  }
  .filter-disclosure > summary::-webkit-details-marker { display: none; }
  .filter-disclosure > summary::before {
    content: '+';
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 13px;
    color: var(--muted);
    width: 10px;
    display: inline-block;
  }
  .filter-disclosure[open] > summary::before { content: '−'; color: var(--accent); }
  .filter-disclosure > summary .dis-label {
    color: var(--fg);
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    font-size: 11px;
  }
  .filter-disclosure[open] > summary .dis-label { color: var(--accent); }
  .filter-disclosure > .row { margin-top: 10px; }

  /* First-session Score explainer. Single dismissable line above the
     table — the casual-flipper persona was confused by what Score
     meant; hover-tooltip alone wasn't enough. localStorage flag means
     each user sees it once. */
  .score-explainer {
    background: var(--panel);
    border: 1px solid var(--border);
    border-left: 3px solid var(--accent);
    border-radius: 8px;
    padding: 10px 38px 10px 14px;
    font-size: 12.5px;
    color: var(--muted);
    line-height: 1.5;
    position: relative;
  }
  .score-explainer strong { color: var(--fg); font-weight: 600; }
  .score-explainer code {
    background: var(--panel-2);
    padding: 1px 6px;
    border-radius: 4px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.93em;
    color: var(--fg);
  }
  .score-explainer .dismiss {
    position: absolute;
    top: 6px; right: 8px;
    background: transparent;
    border: none;
    color: var(--muted);
    font-size: 16px;
    line-height: 1;
    cursor: pointer;
    padding: 4px 8px;
  }
  .score-explainer .dismiss:hover { color: var(--fg); }

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

  /* Tag chip row — pills, OR-combined among themselves, AND with the
     filters row above. Inactive chips show the live row-count next to
     the tag so the user can see what's worth toggling. Zero-count chips
     stay visible (strikethrough+muted) so vocabulary is discoverable. */
  /* Chip row caps at ~96px (≈3 wrap rows on desktop, ≈4 on mobile) with
     internal vertical scroll. Without the cap an inventory with 168
     distinct tags grew to 1489px on iPhone — five viewport heights of
     pills before the user reached any actionable card. */
  .tagchips {
    gap: 6px;
    align-items: flex-start;
    max-height: 96px;
    overflow-y: auto;
    align-content: flex-start;
  }
  .chip {
    background: transparent;
    color: var(--muted);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 4px 10px 4px 12px;
    font-size: 11px;
    letter-spacing: 0.02em;
    cursor: pointer;
    display: inline-flex;
    gap: 6px;
    align-items: center;
    font: inherit;
    font-size: 11px;
    line-height: 1.2;
    /* Tap-target — old 21px height failed iOS HIG / WCAG ≥ 24px. 28px
       leaves room without losing the pill aesthetic. */
    min-height: 28px;
    transition: color 120ms ease, border-color 120ms ease, background 120ms ease;
  }
  .chip:hover { color: var(--fg); border-color: var(--accent); }
  .chip.active {
    color: var(--accent);
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }
  .chip.zero {
    text-decoration: line-through;
    opacity: 0.45;
    cursor: default;
  }
  .chip-count {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 10px;
    color: var(--muted);
  }
  .chip.active .chip-count { color: var(--accent); }
  .chip-clear {
    background: transparent;
    border: none;
    color: var(--muted);
    font-size: 11px;
    cursor: pointer;
    padding: 3px 8px;
  }
  .chip-clear:hover { color: var(--bad); }
  .filters label {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    letter-spacing: 0.02em;
    color: var(--muted);
    text-transform: uppercase;
  }
  .filters input, .filters select { text-transform: none; letter-spacing: 0; }
  select {
    font: inherit;
    color: var(--fg);
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 5px 8px;
  }
  button.ghost {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--muted);
    font-size: 12px;
    padding: 4px 10px;
  }
  button.ghost:hover { background: var(--panel-2); color: var(--fg); }
  code { background: var(--panel-2); padding: 1px 6px; border-radius: 4px; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 0.93em; }

  /* Set-completion card — recommendation rows. Three reco kinds
     distinguished by a small uppercase pill so the user can scan and
     pick a strategy without reading every detail line. Net plat is the
     right-aligned headline number per row. */
  .set-recos { padding: 14px 16px; gap: 8px; }
  .reco {
    border-top: 1px solid var(--border);
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
    border-radius: 3px;
    padding: 1px 6px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .kind-near-complete       { color: var(--accent); border-color: color-mix(in srgb, var(--accent) 40%, var(--border)); }
  .kind-complete-with-extras { color: var(--good);   border-color: color-mix(in srgb, var(--good)   40%, var(--border)); }
  .kind-extras              { color: var(--warn);   border-color: color-mix(in srgb, var(--warn)   40%, var(--border)); }
  .reco-detail { font-size: 12.5px; margin: 0; line-height: 1.5; }
  .reco-detail strong { color: var(--fg); font-weight: 600; }
  .good-text { color: var(--good); }
  .bad-text { color: var(--bad); }
  /* The net plat is inline with the verb + set name, so the phrase reads
     "Complete Mesa Prime +95p" as a single declarative sentence rather
     than a verb-on-left / number-on-right two-column row. */
  .reco-net-inline {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
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
  .baro-title { display: flex; align-items: baseline; gap: 8px; flex-wrap: wrap; }
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
    border-radius: 8px;
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
  .relic-card {
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-radius: 8px;
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
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
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
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
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
  .card.empty button { flex-shrink: 0; }

  /* FAQ — native <details>, minimal chrome, custom marker. */
  .faq {
    display: flex;
    flex-direction: column;
    gap: 0;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 4px 18px;
    margin-top: 8px;
  }
  .faq h2 { padding: 14px 0 8px; margin: 0; }
  .faq details {
    border-top: 1px solid var(--border);
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
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
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

  footer {
    color: var(--muted);
    font-size: 11.5px;
    text-align: center;
    padding-top: 20px;
    letter-spacing: 0.02em;
  }

  /* Crypto dialogs — minimal, modal, escapes-to-close. */
  dialog.cryptobox {
    background: var(--panel);
    color: var(--fg);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 0;
    max-width: 420px;
    width: calc(100% - 32px);
  }
  dialog.cryptobox::backdrop {
    background: rgba(0, 0, 0, 0.55);
    backdrop-filter: blur(2px);
  }
  dialog.cryptobox form {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 20px 20px 16px;
  }
  dialog.cryptobox header { display: flex; flex-direction: column; gap: 6px; }
  dialog.cryptobox h3 {
    margin: 0;
    font-size: 13px;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--accent);
    font-weight: 600;
  }
  dialog.cryptobox header p { margin: 0; font-size: 12.5px; line-height: 1.5; }
  dialog.cryptobox label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 11.5px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--muted);
  }
  dialog.cryptobox input[type="password"] {
    font: inherit;
    color: var(--fg);
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 8px 10px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  dialog.cryptobox input[type="password"]:focus {
    outline: none;
    border-color: var(--accent);
  }
  dialog.cryptobox .err {
    color: var(--bad);
    font-size: 12px;
    background: color-mix(in srgb, var(--bad) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--bad) 40%, var(--border));
    padding: 8px 10px;
    border-radius: 6px;
  }
  dialog.cryptobox footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding-top: 4px;
    border: none;
  }
</style>
