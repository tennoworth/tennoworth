import { untrack } from 'svelte';
import { sellableQty } from '../../domain/sell-priority';
import { listingBlockReason as listingEligibilityBlockReason, listingActionLabel as listingEligibilityActionLabel } from './eligibility';
import { advisorInput, relicInput, scoreInput, setInput, type CalcInputs } from './calc-inputs';
import type { InventoryController } from '../inventory/controller.svelte';
import type { ProtectionController } from './protection.svelte';
import type { ListingController } from './controller.svelte';
import type { FilterController, View } from './filters.svelte';
import type { DesktopCapabilities } from '../../contracts/desktop';
import type { DesktopServices } from '../../contracts/services';
import type { SellRow } from '../../contracts/selling';
import type { RelicPlanEntry, SetReco, Verdict, ScoredInventoryFact } from '../../contracts/generated/domain';
import type { History } from '../../domain/history';
import { DomainResult } from './domain-result.svelte';
import { computeResults as computeFilteredResults, computeAvailableTags, computeEmptyReason, type FilterState } from '../../domain/filter-engine';
import { PRESETS, presetStillMatches } from '../../domain/presets';
import { baroPhase } from '../../domain/baro-board';
import { baroLocation } from '../../ui/format';
import { lookup } from '../../domain/market';

export function createSellWorkspace({ inventory, filters, protection, listing, transport, services, getView, getNow }: { inventory: InventoryController; filters: FilterController; protection: ProtectionController; listing: ListingController; transport: DesktopCapabilities; services: Pick<DesktopServices, 'scoreInventoryNative' | 'evaluateAdvisor' | 'setRecos' | 'relicPlan'>; getView: () => View; getNow: () => number }) {
  const { scoreInventoryNative, evaluateAdvisor, setRecos: loadSetRecos, relicPlan: loadRelicPlan } = services;
  let supportedOwned = $derived(new Map([...inventory.resolved.owned].filter(([, row]) => !row.subtype && !row.slug.endsWith('_set') && !row.slug.endsWith('_relic') && inventory.market?.items[row.slug])));
  let allocationMatches = $derived(protection.matchesInventory(inventory.resolved.owned, inventory.nativeSnapshotId));
  let unknownSlugs = $derived(new Set([...supportedOwned.values()].filter(row => !allocationMatches || protection.state?.items[row.slug]?.estimated == null).map(row => row.slug)));
  let guidanceUnavailable = $derived(supportedOwned.size > 0 && unknownSlugs.size === supportedOwned.size);
  // The gate's rules live in features/selling/eligibility so they can be tested
  // without mounting this component; the shell only supplies their inputs.
  let eligibilityInputs = $derived({
    hasSnapshot: !!inventory.nativeSnapshotId,
    pullingInventory: inventory.pullingInventory,
    allocationMatches,
    hasProtection: !!protection.state,
    protectionSnapshotId: protection.state?.snapshot_id ?? null,
    nativeSnapshotId: inventory.nativeSnapshotId,
    protectionError: protection.error,
    wfmUnlocked: listing.wfmStatus?.unlocked ?? false,
    availableQuantities: Object.fromEntries(Object.entries(protection.state?.items ?? {}).map(([slug, item]) => [slug, item.available])),
    supportedSlugs: [...supportedOwned.values()].map(row => row.slug),
    // An allocation computed against a different inventory places nothing, so
    // every item reads as unknown.
    known: allocationMatches ? [...protection.state?.items ? Object.keys(protection.state.items).filter(slug => protection.state?.items[slug]?.estimated != null) : []] : [],
  });
  let listingBlockReason = $derived(listingEligibilityBlockReason(eligibilityInputs));
  let listingQuantitiesKnown = $derived(!listingBlockReason);
  let estimatedGuidance = $derived(!listingQuantitiesKnown);
  let listingActionLabel = $derived(listingEligibilityActionLabel(eligibilityInputs));
  let availability = $derived(new Map([...inventory.resolved.owned].map(([key, row]) => [key,
    listingQuantitiesKnown && supportedOwned.has(key) ? Math.min(sellableQty(row.count, filters.reserveCopies, row.leveled ?? 0), protection.state?.items[row.slug]?.available ?? 0) : 0,
  ])));
  let guidanceAvailability = $derived(new Map([...inventory.resolved.owned].map(([key, row]) => [key,
    supportedOwned.has(key) && !unknownSlugs.has(row.slug) ? Math.min(sellableQty(row.count, filters.reserveCopies, row.leveled ?? 0),
      (estimatedGuidance ? protection.state?.items[row.slug]?.estimated : protection.state?.items[row.slug]?.available) ?? 0) : 0,
  ])));
  let guidanceOwned = $derived(new Map([...inventory.resolved.owned].filter(([, row]) => !unknownSlugs.has(row.slug))));
  let availableOwned = $derived(new Map([...guidanceOwned].map(([key, row]) => [key, { ...row, count: guidanceAvailability.get(key) ?? 0, leveled: 0 }])));

  let results = $state<SellRow[]>([]);
  // The Sell table pushes its filtered+sorted rows up here so the "List on WFM"
  // CTA stages exactly what the user sees (name filter + badge chips), not the
  // unfiltered preset results.
  let tableView = $state<{ rows: SellRow[]; active: boolean }>({ rows: [], active: false });

  let visibleColumns = $derived<string[] | null>(filters.columnChoice[filters.columnKey] ?? (filters.activePreset ? PRESETS[filters.activePreset]?.columns ?? null : null));
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

  let sparesOnly = $derived(!!PRESETS[filters.activePreset ?? '']?.sparesOnly);
  let filterState = $derived<FilterState>({
    minPrice: filters.minPrice, minOwned: filters.minOwned, typeFilter: filters.typeFilter, hideAtLvl: filters.hideAtLvl, activeTags: filters.activeTags,
    vaultOnly: !!PRESETS[filters.activePreset ?? '']?.vaultOnly,
    ducatsOnly: !!PRESETS[filters.activePreset ?? '']?.ducatsOnly,
    minVol: PRESETS[filters.activePreset ?? '']?.minVol ?? 0,
    minMedian: PRESETS[filters.activePreset ?? '']?.minMedian ?? 0,
    typesAny: PRESETS[filters.activePreset ?? '']?.typesAny ?? [],
    sparesOnly,
    adviceOnly: !!PRESETS[filters.activePreset ?? '']?.adviceOnly,
  });

  // ---- Hold-or-sell advisor inputs ----
  // The year-long history loads once, on demand, the first time a surface
  // that uses advice opens (the Hold/Sell preset or the Set picks view) -
  // same lazy pattern as the market browser's 1-year toggle. Verdicts
  // degrade gracefully to the calendar-only rules until it lands.
  const historyResult = new DomainResult<History | null>(() => null);
  let advisorHistory = $derived(historyResult.value);
  // The per-calculation gates live in features/selling/calc-inputs so they can be
  // tested without mounting this component. The effects below supply the inputs
  // and the recompute epoch; the policies decide whether there is anything to run.
  // Getters, not a $derived object: each effect then depends only on the fields
  // its policy reads. One derived object made every calculation restart when any
  // input changed, so toggling "spares only" blanked the default scoring.
  const calcInputs: CalcInputs = {
    get owned() { return inventory.resolved.owned; },
    get previousOwned() { return inventory.previousOwned; },
    get availableOwned() { return availableOwned; },
    get market() { return inventory.market; },
    get reserve() { return filters.reserveCopies; },
    get available() { return guidanceAvailability; },
    get sparesOnly() { return sparesOnly; },
    get advisorHistory() { return advisorHistory; },
  };

  $effect(() => {
    const wanted = filters.activePreset === 'holdsell' || getView() === 'sets';
    if (!wanted || untrack(() => historyResult.phase === 'done')) return;
    return untrack(() => historyResult.start(() => transport.loadHistory()));
  });
  let calculationEpoch = $state(0);
  const advisorResult = new DomainResult<Map<string, Verdict>>(() => new Map());
  $effect(() => {
    void calculationEpoch;
    const run = advisorInput(calcInputs);
    if (!run) {
      untrack(() => advisorResult.clear());
      return;
    }
    return untrack(() => advisorResult.start(async () => new Map(Object.entries(await evaluateAdvisor({ slugs: run.slugs, market: run.market, history: run.history, now_ms: Date.now() })))));
  });
  let adviceMap = $derived(advisorResult.value);

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


  function clear() {
    void inventory.clear();
    results = [];
    tableView = { rows: [], active: false };
    for (const result of [defaultFacts, spareFacts, previousFacts, advisorResult, setResult, relicResult]) result.clear();
  }

  const defaultFacts = new DomainResult<Map<string, ScoredInventoryFact>>(() => new Map());
  const spareFacts = new DomainResult<Map<string, ScoredInventoryFact>>(() => new Map());
  const previousFacts = new DomainResult<Map<string, ScoredInventoryFact>>(() => new Map());
  const setResult = new DomainResult<SetReco[]>(() => []);
  const relicResult = new DomainResult<RelicPlanEntry[]>(() => []);

  $effect(() => {
    void calculationEpoch;
    const run = scoreInput(calcInputs, 'default');
    if (!run) { untrack(() => defaultFacts.clear()); return; }
    return untrack(() => defaultFacts.start(() => scoreInventoryNative(run.owned, run.market, run.reserve, run.sparesOnly, run.available)));
  });
  $effect(() => {
    void calculationEpoch;
    const run = scoreInput(calcInputs, 'spare');
    if (!run) { untrack(() => spareFacts.clear()); return; }
    return untrack(() => spareFacts.start(() => scoreInventoryNative(run.owned, run.market, run.reserve, run.sparesOnly, run.available)));
  });
  $effect(() => {
    void calculationEpoch;
    const run = scoreInput(calcInputs, 'previous');
    if (!run) { untrack(() => previousFacts.clear()); return; }
    return untrack(() => previousFacts.start(() => scoreInventoryNative(run.owned, run.market, run.reserve, run.sparesOnly)));
  });
  let currentFacts = $derived(sparesOnly ? spareFacts : defaultFacts);




  let calculationError = $derived((guidanceUnavailable ? 'Protection rules are unavailable. Refresh allocation before using quantity estimates.' : null) ?? currentFacts.error ?? (filterState.adviceOnly ? advisorResult.error : null));
  let calculationPending = $derived(inventory.resolved.owned.size > 0 && !!inventory.market &&
    ((!protection.state && protection.loading) || currentFacts.phase === 'idle' || currentFacts.phase === 'loading' || (filterState.adviceOnly && advisorResult.phase === 'loading')));
  let calculationsReady = $derived(!calculationPending && !calculationError && currentFacts.phase === 'done');
  // Rows eligible for the bulk "List on WFM" action: the table-filtered set when
  // a table filter is active, else all results - minus relics (subtyped rows),
  // since selling an intact relic at a few plat usually loses to cracking it
  // (the Relic planner ranks those), so they shouldn't be staged by default.
  let listableRows = $derived.by(() => {
    if (!calculationsReady || estimatedGuidance) return [];
    const current = new Map(results.map(row => [row.key ?? row.slug, row]));
    const visible = tableView.active ? tableView.rows.flatMap(row => {
      const match = current.get(row.key ?? row.slug);
      return match ? [match] : [];
    }) : results;
    return visible.filter(row => !row.subtype && row.sellable > 0);
  });
  $effect(() => {
    results = computeFilteredResults(guidanceOwned, inventory.market, filterState, filters.reserveCopies, adviceMap, guidanceAvailability, currentFacts.value);
  });
  $effect(() => {
    void calculationEpoch;
    const run = setInput(calcInputs);
    if (!run || guidanceUnavailable) { untrack(() => setResult.clear()); return; }
    return untrack(() => setResult.start(() => loadSetRecos(run.owned, run.market)));
  });
  $effect(() => {
    void calculationEpoch;
    const run = relicInput(calcInputs);
    if (!run) { untrack(() => relicResult.clear()); return; }
    return untrack(() => relicResult.start(() => loadRelicPlan(run.owned, run.market, Number.MAX_SAFE_INTEGER)));
  });
  let setRecos = $derived(setResult.value.filter(reco => !(inventory.market?.set_to_parts?.[reco.set_slug]?.parts ?? []).some(part => unknownSlugs.has(part.slug))));


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
    if (!inventory.previousOwned || !inventory.market || previousFacts.phase !== 'done' || currentFacts.phase !== 'done') return null;
    const rows = computeFilteredResults(inventory.previousOwned, inventory.market, filterState, filters.reserveCopies, undefined, undefined, previousFacts.value);
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

  // Friendly diagnosis of WHY the table is empty so we don't just shrug.
  let emptyReason = $derived.by(() =>
    currentFacts.phase !== 'done' || (filterState.adviceOnly && advisorResult.phase !== 'done') ? null : computeEmptyReason(inventory.resolved.owned, inventory.market, filterState, results.length, filters.activePreset, adviceMap)
  );


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
  let showBaroCard = $derived(voidTrader != null);

  let baroState = $derived(voidTrader ? baroPhase(voidTrader.activation, voidTrader.expiry, getNow()) : null);
  // Ticks each minute and on focus (see the onMount below). Anything that shows
  // time left reads this: `Date.now()` inside a `$derived` is not tracked.

  return { clear,
    get voidTrader() { return voidTrader; },
    get ducatStats() { return ducatStats; },
    get showBaroCard() { return showBaroCard; },
    get baroState() { return baroState; },

    get supportedOwned() { return supportedOwned; },
    get allocationMatches() { return allocationMatches; },
    get unknownSlugs() { return unknownSlugs; },
    get guidanceUnavailable() { return guidanceUnavailable; },
    get eligibilityInputs() { return eligibilityInputs; },
    get listingBlockReason() { return listingBlockReason; },
    get listingQuantitiesKnown() { return listingQuantitiesKnown; },
    get estimatedGuidance() { return estimatedGuidance; },
    get listingActionLabel() { return listingActionLabel; },
    get availability() { return availability; },
    get guidanceAvailability() { return guidanceAvailability; },
    get guidanceOwned() { return guidanceOwned; },
    get availableOwned() { return availableOwned; },
    get results() { return results; },
    get tableView() { return tableView; },
    set tableView(value: typeof tableView) { tableView = value; },
    get visibleColumns() { return visibleColumns; },
    get presetSort() { return presetSort; },
    get ownedQtyForOrders() { return ownedQtyForOrders; },
    get sparesOnly() { return sparesOnly; },
    get filterState() { return filterState; },
    get advisorHistory() { return advisorHistory; },
    get calculationEpoch() { return calculationEpoch; },
    set calculationEpoch(value: typeof calculationEpoch) { calculationEpoch = value; },
    get adviceMap() { return adviceMap; },
    get currentFacts() { return currentFacts; },
    get calculationError() { return calculationError; },
    get calculationPending() { return calculationPending; },
    get calculationsReady() { return calculationsReady; },
    get listableRows() { return listableRows; },
    get setRecos() { return setRecos; },
    get availableTags() { return availableTags; },
    get availableTypes() { return availableTypes; },
    get totalPotential() { return totalPotential; },
    get prevSummary() { return prevSummary; },
    get sinceScan() { return sinceScan; },
    get emptyReason() { return emptyReason; },
    get historyResult() { return historyResult; },
    get advisorResult() { return advisorResult; },
    get defaultFacts() { return defaultFacts; },
    get spareFacts() { return spareFacts; },
    get previousFacts() { return previousFacts; },
    get setResult() { return setResult; },
    get relicResult() { return relicResult; },
  };
}
