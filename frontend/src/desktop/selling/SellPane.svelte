<script lang="ts">
  import ResultsTable from './ResultsTable.svelte';
  import Select from '../../ui/Select.svelte';
  import { PRESETS } from '../../domain/presets';
  import { selectPicks, MIN_PICK_SCORE, LIQUID_VOL } from '../../domain/sell-priority';


  import type { Snippet } from 'svelte';
  import type { ProtectionState } from '../../contracts/protection';
  import type { OwnedRecord, TradeAllowance } from '../../contracts/data';
  import { splitByTrades, tradesToSell } from '../../domain/trade-session';
  import type { ListingCandidate } from '../../contracts/listing';
  import type { EmptyReason } from '../../domain/filter-engine';
  import type { SellRow } from '../../contracts/selling';
  import type { ListingReceipt, ListingReceipts } from './receipts.svelte';
  import { untrack } from 'svelte';
  import { focusLater, roving } from '../../ui/keyboard-actions';
  import Rich from '../../ui/Rich.svelte';
  import { formatLocale, t } from '../../i18n/i18n.svelte';
  import type { MessageKey } from '../../i18n/en';
  interface Props {
    minPrice: number; minOwned: number; typeFilter: string; hideAtLvl: number;
    activeTags: Set<string>; tableView: { rows: SellRow[]; active: boolean };
    resolved: { owned: Map<string, OwnedRecord>; unresolved: Record<string, number> };
    allocation?: ProtectionState | null;
    results: SellRow[]; deltas: Map<string, number>; totalPotential: number;
    /** Every sellable item, before this view's filters: the sidebar's count. */
    allSellable?: number | null;
    prevSummary?: { owned: number; sellable: number; potential: number } | null;
    sinceScan?: { added: number; changed: number; removed: number } | null;
    ordersSummary?: { live: number; issues: number } | null;
    marketFreshness: 'unknown' | 'fresh' | 'aging' | 'stale';
    marketStaleness: string | null; marketLoadError: string | null;
    listableRows: SellRow[]; availableTags: Array<[string, number]>; availableTypes: string[];
    visibleColumns: string[] | null; presetSort: { key: string; dir: number } | null;
    columnsCustomized?: boolean; oncolumnschange?: (columns: string[] | null) => void;
    emptyReason: EmptyReason | null; activePreset: string | null; reserveCopies: number;
    filtersOpen: boolean; sellOnboardingDismissed: boolean;
    keepCopiesNudgeDismissed: boolean;
    /** What this session listed, so a listed pick says so in its own row. */
    receipts?: ListingReceipts;
    onmakevisible?: (receipt: ListingReceipt) => void;
    onopenorders?: () => void;
    applyPreset(name: string): void; setReserveCopies(value: number | Event): void;
    toggleFiltersOpen(event: Event): void;
    dismissSellOnboarding(): void; dismissKeepCopiesNudge(): void;
    openListingFlow(rows?: ListingCandidate | ListingCandidate[]): void;
    pendingBanner: Snippet;
    // The keep-rules strip: rendered under the view title, after stale-data notices.
    keep?: Snippet;
    calculationPending?: boolean;
    calculationError?: string | null;
    calculationErrorShown?: boolean;
    estimatedGuidance?: boolean;
    canList?: boolean;
    unavailableCount?: number;
    listingActionLabel?: string;
    oncheckListings?: () => void;
    onretryCalculation?: () => void;
    /** Today's trades left, from the last scan; null while unknown. */
    tradeAllowance?: TradeAllowance | null;
    /** Items warframe.market trades in bulk (up to six per trade); null while
     *  unknown (warframe.market locked), so trade counts are ranges. */
    bulkSlugs?: ReadonlySet<string> | null;
  }

  let {
    minPrice = $bindable(),
    minOwned = $bindable(),
    typeFilter = $bindable(),
    hideAtLvl = $bindable(),
    activeTags = $bindable(),
    tableView = $bindable(),

    resolved, results, deltas, totalPotential, allSellable = null, allocation = null,
    prevSummary = null, sinceScan = null, ordersSummary = null,
    marketFreshness, marketStaleness, marketLoadError,
    listableRows, availableTags, availableTypes,
    visibleColumns, presetSort, emptyReason, columnsCustomized = false, oncolumnschange,
    activePreset, reserveCopies, filtersOpen,
    sellOnboardingDismissed, keepCopiesNudgeDismissed,

    applyPreset, setReserveCopies, toggleFiltersOpen, 
    dismissSellOnboarding, dismissKeepCopiesNudge,
    openListingFlow, receipts, onmakevisible, onopenorders,
    pendingBanner, keep, estimatedGuidance = false, canList = true, unavailableCount = 0, listingActionLabel, oncheckListings, calculationPending = false, calculationError = null, onretryCalculation, calculationErrorShown = false,
  tradeAllowance = null, bulkSlugs = null,
  }: Props = $props();

  // Was inline in the template, so it re-filtered the whole results array on
  // EVERY render of this pane - including every keystroke in the name filter,
  // which cannot change the answer. $derived recomputes only when results does.
  let calculationReady = $derived(!calculationPending && !calculationError);
  let sellableCount = $derived(results.filter((r) => r.sellable > 0).length);

  // Since-last-scan deltas for the summary cells. prevSummary is the previous
  // snapshot pushed through the same filter cascade (App.svelte), so each Δ is
  // in the cell's own units. Null (no previous scan this session) hides them.
  let ownedDelta = $derived(prevSummary ? resolved.owned.size - prevSummary.owned : null);
  let sellableDelta = $derived(prevSummary ? sellableCount - prevSummary.sellable : null);
  let potentialDelta = $derived(prevSummary ? Math.round(totalPotential - prevSummary.potential) : null);
  function fmtDelta(d: number | null, unit = '') {
    if (d == null || d === 0) return null;
    const n = Math.abs(d).toLocaleString(formatLocale(), { maximumFractionDigits: 0 });
    return `${d > 0 ? '▲' : '▼'}${n}${unit}`;
  }
  // "Changes only" - session-only view of the rows whose count moved since the
  // last scan (the Δ column's non-zero rows). Not persisted: it is a glance,
  // not a preference, and it means nothing after a reload (no deltas then).
  let changesOnly = $state(false);
  let tableRows = $derived(
    changesOnly ? results.filter((r) => (deltas.get(r.key ?? r.slug) ?? 0) !== 0) : results,
  );

  // Bulk "List on WFM" staging copy. listableRows is already the filtered (or
  // unfiltered) set minus relics (subtyped rows) and no-spare rows; the batch
  // is capped at the first 50 either way. The label stays count-free: the
  // table has no row selection, so a number there read as a fixed batch size
  // (always "50" on a large inventory); the title carries the staged count.
  let stagedCount = $derived(Math.min(listableRows.length, 50));
  let listTitle = $derived(t(tableView.active ? 'sell.listTitleView' : 'sell.listTitleTop', { count: stagedCount }));

  // Auto-derived options for the type dropdown label map. The Type filter
  // mixes warframestat catalog categories (Mods, Arcanes, Relics…) with raw
  // inventory.json keys used as fallback (MiscItems, RawUpgrades…). The
  // internal names leak as-is without this map.
  const TYPE_LABELS: Record<string, MessageKey> = {
    MiscItems: 'sell.type.MiscItems',
    Recipes: 'sell.type.Recipes',
    RawUpgrades: 'sell.type.RawUpgrades',
    Suits: 'sell.type.Suits',
    LongGuns: 'sell.type.LongGuns',
    Pistols: 'sell.type.Pistols',
    Melee: 'sell.type.Melee',
    SpaceGuns: 'sell.type.SpaceGuns',
    SpaceMelee: 'sell.type.SpaceMelee',
    SentinelWeapons: 'sell.type.SentinelWeapons',
  };
  const typeLabel = (type: string) => (TYPE_LABELS[type] ? t(TYPE_LABELS[type]) : type);
  // Preset names and hints come from the catalog by preset name; domain/ keeps
  // its English labels for itself.
  const PRESET_TEXT: Record<string, [MessageKey, MessageKey]> = {
    default: ['sell.preset.default', 'sell.preset.default.hint'],
    spares: ['sell.preset.spares', 'sell.preset.spares.hint'],
    ducats: ['sell.preset.ducats', 'sell.preset.ducats.hint'],
    trending: ['sell.preset.trending', 'sell.preset.trending.hint'],
    sets: ['sell.preset.sets', 'sell.preset.sets.hint'],
    holdsell: ['sell.preset.holdsell', 'sell.preset.holdsell.hint'],
    vault: ['sell.preset.vault', 'sell.preset.vault.hint'],
  };
  const presetLabel = (name: string) => (PRESET_TEXT[name] ? t(PRESET_TEXT[name][0]) : PRESETS[name].label);
  const presetHint = (name: string) => (PRESET_TEXT[name] ? t(PRESET_TEXT[name][1]) : PRESETS[name].hint);

  function toggleTag(tag: string) {
    const next = new Set(activeTags);
    if (next.has(tag)) next.delete(tag); else next.add(tag);
    activeTags = next;
  }

  // Top picks strip. Built from `results`, which is already sorted best-first,
  // so selectPicks is a cheap filter+slice - no rescoring. Deliberately reads
  // `results` rather than `tableView.rows`: the table's own local name-search
  // box and pill-filter chips only narrow `tableView.rows`, so picks stay
  // "global" with respect to those. They are NOT independent of the filter
  // rail / active preset, though - App.svelte's computeResults() reads
  // minPrice/minOwned/typeFilter/hideAtLvl/activeTags and the active
  // preset's own floors (vaultOnly/ducatsOnly/minVol/minMedian) directly
  // rather than taking them as parameters, so `results` (and therefore
  // picks) narrows along with whatever preset the user has selected. True
  // preset-independence would need computeResults to accept filter
  // overrides - out of scope here.
  let allPicks = $derived.by(() => selectPicks(results));
  // Snooze is session-only, no persistence and no new localStorage key - a
  // dismissed pick reappearing on reload is the honest, cheap behaviour; it
  // isn't worth a storage-key version bump for a "hide until refresh" nicety.
  let snoozedPicks = $state(new Set<string>());
  // A pick hidden here keeps its slot as an Undo receipt until the pick set
  // itself changes (a new scan or scope), so the next List never moves under
  // the pointer. Keyed on the picks' identities, not the array: background
  // recalculation rebuilds the array without changing what is picked.
  let receiptKeys = $state(new Set<string>());
  let pickSignature = $derived(allPicks.map((p) => p.key ?? p.slug).join('|'));
  let shownSignature = '';
  $effect(() => {
    const signature = pickSignature;
    untrack(() => { if (signature !== shownSignature) { shownSignature = signature; if (receiptKeys.size) receiptKeys = new Set(); } });
  });
  let picks = $derived(allPicks.filter((p) => !snoozedPicks.has(p.key ?? p.slug) || receiptKeys.has(p.key ?? p.slug)));
  // Hiding and undoing swap the row's buttons, so focus follows to the
  // replacement in the same row rather than falling to the page.
  const pickControl = (key: string, cls: string) => () => [...document.querySelectorAll<HTMLElement>(`.pick-actions .${cls}[data-pick]`)].find((el) => el.dataset.pick === key);
  function snoozePick(key: string) {
    snoozedPicks = new Set([...snoozedPicks, key]);
    receiptKeys = new Set([...receiptKeys, key]);
    focusLater(pickControl(key, 'pick-list'));
  }
  function undoSnooze(key: string) {
    const next = new Set(snoozedPicks);
    next.delete(key);
    snoozedPicks = next;
    const shown = new Set(receiptKeys);
    shown.delete(key);
    receiptKeys = shown;
    focusLater(pickControl(key, 'pick-list'));
  }
  const isSnoozed = (p: SellRow) => snoozedPicks.has(p.key ?? p.slug);
  // Trades: each pick's count under the lot rule, and where today's trades run
  // out. Hidden picks spend nothing; the split keeps rank order.
  // Without the bulk list a count is a range (all bulk … none bulk), and a
  // range cannot place the divider, so there is none.
  const pickTrades = (p: SellRow) => tradesToSell(p.sellable, bulkSlugs?.has(p.slug) ?? false);
  const pickTradeRange = (p: SellRow): [number, number] => bulkSlugs ? [pickTrades(p), pickTrades(p)] : [tradesToSell(p.sellable, true), tradesToSell(p.sellable, false)];
  let tradesLeft = $derived(tradeAllowance?.remaining ?? null);
  let tradeSplit = $derived(splitByTrades(picks.filter((p) => !isSnoozed(p)), pickTrades, bulkSlugs ? tradesLeft : null));
  let picksCut = $derived(tradeSplit.later.length ? picks.indexOf(tradeSplit.later[0]) : null);
  // The day as cells: what the picks use, what stays free, what was already
  // traded. A day holds as many trades as the mastery rank, so without the rank
  // there is no gauge; without bulk status the picks' share is unknown and
  // only spent and free are drawn.
  let gauge = $derived.by(() => {
    const max = tradeAllowance?.mastery_rank ?? null;
    if (max == null || max <= 0 || tradesLeft == null || tradesLeft > max) return null;
    const plan = bulkSlugs ? tradeSplit.used : 0;
    return [...Array(plan).fill('plan'), ...Array(tradesLeft - plan).fill('free'), ...Array(max - tradesLeft).fill('spent')] as string[];
  });
  function resetIn(now = Date.now()): string {
    const d = new Date(now), left = Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate() + 1) - now;
    const h = Math.floor(left / 3_600_000), m = Math.floor(left / 60_000) % 60;
    return h ? t('sell.resetHoursMinutes', { h, m }) : t('sell.resetMinutes', { m });
  }
  const listedReceipt = (p: SellRow) => receipts?.forRow(p.slug, p.subtype);
  const clock = (at: number) => new Date(at).toLocaleTimeString(formatLocale(), { hour: '2-digit', minute: '2-digit' });

  // Plain-language reason line for a pick, built only from fields the row
  // already carries (timing / delta_90d_pct / clearing_price / volume_48h) -
  // no invented ETAs or destinations. Order of checks mirrors how confident
  // each signal is: a corroborated peak is the strongest "act now" case,
  // then whatever 90d trend exists, then the plain fallback for a
  // flat/illiquid-trend row. Hold rows never become picks.
  // The reason says only what the columns beside it cannot: where the price
  // sits against its 90-day baseline. Low ask and Vol 48h are already on the
  // row; the clearing price is named only when the troll-undercut clamp made it
  // differ from the low ask the row shows.
  function pickReason(p: SellRow) {
    const price = Math.round(p.clearing_price ?? p.low_sell);
    const clamp = price !== Math.round(p.low_sell) ? t('sell.reasonClears', { price }) : '';
    const d = p.delta_90d_pct;
    if (p.timing === 'peak') return (d != null && d > 0 ? t('sell.reasonPeakUp', { pct: Math.round(d) }) : t('sell.reasonPeak')) + clamp;
    if (d != null && d >= 1) return t('sell.reasonUp', { pct: Math.round(d) }) + clamp;
    if (d != null && d <= -1) return t('sell.reasonDown', { pct: Math.abs(Math.round(d)) }) + clamp;
    return t('sell.reasonFlat') + clamp;
  }


  // Row B "active filter" chips - the Filters popover's state surfaced as
  // removable chips so a narrowed table never reads as a mysteriously short
  // one. Each × resets that one filter to its wide-open value.
  let activeFilterChips = $derived.by(() => {
    const out = [];
    if (minPrice > 0) out.push({ key: 'price', label: t('sell.chipMinPrice', { price: minPrice }), clear: () => (minPrice = 0) });
    if (minOwned > 1) out.push({ key: 'owned', label: t('sell.chipMinOwned', { count: minOwned }), clear: () => (minOwned = 1) });
    if (reserveCopies > 0) out.push({ key: 'reserve', label: t('sell.chipKeep', { count: reserveCopies }), clear: () => setReserveCopies(0) });
    if (typeFilter !== 'all') out.push({ key: 'type', label: t('sell.chipType', { type: typeLabel(typeFilter) }), clear: () => (typeFilter = 'all') });
    if (hideAtLvl < 11) out.push({ key: 'kept', label: t('sell.chipRanked', { rank: hideAtLvl }), clear: () => (hideAtLvl = 11) });
    return out;
  });

  // Preset-driven empty states reset the preset; hand-filter ones relax the
  // offending slider. A price slider can't rescue a "no vaulted parts" empty.
  const PRESET_EMPTY_KINDS = new Set(['tag', 'vault', 'ducats', 'vol', 'median', 'spares', 'advice']);
  // One-shot quick-fix actions the empty state can offer.
  function relaxFilters({ kind }: { kind: string }) {
    if (kind === 'price') minPrice = 1;
    if (kind === 'owned') minOwned = 1;
    if (kind === 'type')  typeFilter = 'all';
    if (kind === 'kept')  hideAtLvl = 11;
    if (PRESET_EMPTY_KINDS.has(kind)) applyPreset('default');
  }
</script>

<section class="view-header">
  <h2>{t(estimatedGuidance ? 'sell.titleEstimated' : 'sell.title')}</h2>
  <p class="lede">{t(estimatedGuidance ? 'sell.ledeEstimated' : 'sell.lede')}</p>
  <!-- Totals with since-last-scan deltas; Listed / Needs fixing appear once
       the orders panel has reported; the last cell is the since-scan state
       with the session-only "Changes only" toggle. -->
  <div class="vh-end">
  <div class="ui-totals" role="group" aria-label={t('sell.summary')}>
    <div class="cell">
      <span class="k">{t('sell.owned')}</span>
      <span class="v">{resolved.owned.size.toLocaleString(formatLocale())}</span>
      {#if fmtDelta(ownedDelta)}<span class="d" class:up={(ownedDelta ?? 0) > 0} class:down={(ownedDelta ?? 0) < 0}>{fmtDelta(ownedDelta)}</span>{/if}
    </div>
    <div class="cell">
      <span class="k">{t(estimatedGuidance ? 'sell.opportunities' : 'sell.sellable')}</span>
      <span class="v" title={allSellable != null ? t('sell.sellableTitleOfAll', { all: allSellable }) : t('sell.sellableTitle')}>{calculationReady ? sellableCount.toLocaleString(formatLocale()) : t('sell.unavailable')}{#if calculationReady && allSellable != null && allSellable !== sellableCount}<span class="unit">{t('sell.ofAll', { all: allSellable })}</span>{/if}</span>
      {#if calculationReady && fmtDelta(sellableDelta)}<span class="d" class:up={(sellableDelta ?? 0) > 0} class:down={(sellableDelta ?? 0) < 0}>{fmtDelta(sellableDelta)}</span>{/if}
    </div>
    <div class="cell">
      <span class="k">{t(unavailableCount ? 'sell.knownEstimatedValue' : estimatedGuidance ? 'sell.estimatedValue' : 'sell.stackValue')}</span>
      <span class="v">{calculationReady ? totalPotential.toLocaleString(formatLocale(), { maximumFractionDigits: 0 }) : t('sell.unavailable')}{#if calculationReady}<span class="unit">p</span>{/if}</span>
      {#if calculationReady && fmtDelta(potentialDelta)}<span class="d" class:up={(potentialDelta ?? 0) > 0} class:down={(potentialDelta ?? 0) < 0}>{fmtDelta(potentialDelta, 'p')}</span>{/if}
    </div>
    {#if ordersSummary}
      <div class="cell">
        <span class="k">{t('sell.listed')}</span>
        <span class="v">{ordersSummary.live.toLocaleString(formatLocale())}</span>
      </div>
      {#if ordersSummary.issues > 0}
        <div class="cell attn">
          <span class="k">{t('sell.needsFixing')}</span>
          <span class="v">{ordersSummary.issues.toLocaleString(formatLocale())}</span>
        </div>
      {/if}
    {/if}
    {#if sinceScan}
      <div class="cell since">
        <span><Rich text={t('sell.sinceScan', { added: sinceScan.added, changed: sinceScan.changed, removed: sinceScan.removed })} /></span>
        <button
          type="button"
          class="changes-toggle"
          class:on={changesOnly}
          aria-pressed={changesOnly}
          onclick={() => (changesOnly = !changesOnly)}
          title={t('sell.changesOnlyTitle')}
        >{changesOnly ? '☑' : '☐'} {t('sell.changesOnly')}</button>
      </div>
    {/if}
  </div>
  </div>
</section>

{#if !sellOnboardingDismissed}
  <div class="card sell-onboarding">
    <div class="so-body">
      <strong>{t('sell.howTitle')}</strong>
      <p class="muted"><Rich text={t('sell.howBody')} /></p>
    </div>
    <button class="dismiss" onclick={dismissSellOnboarding} aria-label={t('sell.dismiss')}>×</button>
  </div>
{/if}

{#if sellOnboardingDismissed && !keepCopiesNudgeDismissed && reserveCopies === 0}
  <div class="card warn-banner keep-nudge">
    <div class="kn-body">
      <strong>{t('sell.keepNudgeTitle')}</strong>
      <span class="muted"><Rich text={t('sell.keepNudgeBody')} /></span>
    </div>
    <button class="dismiss" onclick={dismissKeepCopiesNudge} aria-label={t('sell.dismiss')}>×</button>
  </div>
{/if}

{#if calculationError && !calculationErrorShown && !calculationPending}
  <div class="ui-notice" data-tone="bad" role="alert">
    {t('sell.calculationError', { error: calculationError })}
    {#if onretryCalculation}<button class="btn" onclick={onretryCalculation}>{t('sell.retryCalculations')}</button>{/if}
  </div>
{/if}

{#if marketLoadError}
  <div class="card warn-banner">⚠ {marketLoadError}</div>
{:else if marketFreshness === 'stale'}
  <p class="ui-notice" data-tone="warn">
    {marketStaleness ? t('sell.stalePrices', { age: marketStaleness }) : t('sell.stalePricesUnknown')}
  </p>
{/if}

{@render keep?.()}

{#if calculationReady && results.length > 0 && allPicks.length === 0}
  <div class="card empty">
    <div>
      <strong>{t('sell.noPicksTitle')}</strong>
      <p class="muted">{t('sell.noPicksBody')}</p>
    </div>
  </div>
{/if}


{#snippet picksHead()}
  <div class="picks-title">
    <h3>{t('sell.topPicks')}</h3>
    <span class="muted picks-count">
      {picks.length !== allPicks.length ? t('sell.picksCountOf', { shown: picks.length, all: allPicks.length }) : t('sell.picksCount', { shown: allPicks.length })}
      {#if snoozedPicks.size > 0}
        · {t('sell.snoozed', { count: snoozedPicks.size })} <button type="button" class="link" onclick={() => (snoozedPicks = new Set())}>{t('sell.restore')}</button>
      {/if}
    </span>
  </div>
  <span
    class="muted picks-exp"
    title={t('sell.picksExplainTitle', { minScore: MIN_PICK_SCORE })}
  >{t('sell.picksExplain')}</span>
  {#if tradesLeft != null}
    <span class="muted picks-trades" role="status" title={tradesLeft > 0 && !bulkSlugs ? t('sell.unlockToCount') : undefined}>
      {#if gauge}<span class="trade-gauge" aria-hidden="true">{#each gauge as cell}<i class={cell}></i>{/each}</span>{/if}
      <span>{#if tradesLeft === 0}{t('sell.noTradesLeft')}{:else if !bulkSlugs}{t(tradeAllowance?.confidence === 'estimated' ? 'sell.aboutTradesLeft' : 'sell.tradesLeft', { count: tradesLeft })}{:else}{t(tradeAllowance?.confidence === 'estimated' ? 'sell.usesAboutTrades' : 'sell.usesTrades', { count: tradesLeft, used: tradeSplit.used })}{/if}{#if tradeSplit.later.length}{' · '}{t('sell.afterResetCount', { count: tradeSplit.later.length })}{/if}</span>
    </span>
  {/if}
{/snippet}

{#snippet picksCutRow()}
  {@const unlisted = tradeSplit.later.filter((p) => !listedReceipt(p))}
  <span class="cut-label">{t('sell.afterReset', { time: resetIn() })}</span>
  <span class="muted">{tradesLeft === 0 ? t('sell.noTradesLeftSentence') : ''}{t(tradeSplit.later.length === 1 ? 'sell.laterNeedOne' : 'sell.laterNeedMany', { count: tradeSplit.laterTrades })}{tradesLeft ? t('sell.moreThanFree', { count: tradesLeft - tradeSplit.used }) : ''}{t('sell.listingIsFree')}</span>
  {#if unlisted.length}
    <button type="button" class="pick-list cut-list" disabled={!calculationReady || !canList} onclick={() => { if (calculationReady && canList) openListingFlow(unlisted); }}>{unlisted.length === 1 ? t('sell.listItNow') : t('sell.listTheseNow', { count: unlisted.length })}</button>
  {/if}
{/snippet}

{#snippet pickReasonCell(p: SellRow)}
  {@const listed = listedReceipt(p)}
  {#if isSnoozed(p)}
    <div class="rs receipt">{t('sell.hiddenForSession')}</div>
  {:else if listed}
    <div class="rs receipt">
      <Rich text={t(listed.action === 'updated' ? 'sell.updatedReceipt' : 'sell.listedReceipt', { quantity: listed.quantity, price: listed.platinum, time: clock(listed.at) })} />
      {#if onopenorders}· <button type="button" class="link" data-opens-view data-pick={p.key ?? p.slug} onclick={onopenorders}>{t('sell.myOrdersLink')}</button>{/if}
    </div>
  {:else}
  {@const [lo, hi] = pickTradeRange(p)}
  <div class="rs" class:hold={p.timing === 'hold'} class:peak={p.timing === 'peak'}>
    <span class="t">
      {pickReason(p)}
      {#if lo > 0 && lo === hi}
        <span class="pick-trade-count" title={t(bulkSlugs?.has(p.slug) ? 'sell.bulkTrade' : 'sell.oneCopyPerTrade')}>{t('sell.tradeCount', { count: lo })}{#if lo > 1}{' · '}{t('sell.perTrade', { price: Math.round((p.clearing_price ?? p.low_sell) * p.sellable / lo) })}{/if}</span>
      {:else if lo > 0}
        <span class="pick-trade-count" title={t('sell.tradeRangeTitle')}>{t('sell.tradeRange', { lo, hi })}</span>
      {/if}
    </span>
    {#if p.volume_48h < LIQUID_VOL}
      <span class="tag thin" title={t('sell.thinTitle', { floor: LIQUID_VOL })}>{t('sell.thin')}</span>
    {/if}
  </div>
  {/if}
{/snippet}

{#snippet pickActionsCell(p: SellRow)}
  {@const listed = listedReceipt(p)}
  {#if isSnoozed(p)}
    <span class="pick-actions"><button type="button" class="pick-list" data-pick={p.key ?? p.slug} onclick={() => undoSnooze(p.key ?? p.slug)} aria-label={t('sell.undoHiding', { item: p.name })}>{t('sell.undo')}</button></span>
  {:else if listed}
    <span class="pick-actions">
      <span class="tag" class:hold={listed.visible === false} title={t(listed.visible === false ? 'sell.listedHiddenTitle' : listed.visible ? 'sell.listedVisibleTitle' : 'sell.listedUpdatedTitle')}>{t('sell.listedTag', { state: t(listed.visible === false ? 'sell.listedHidden' : listed.visible ? 'sell.listedVisible' : 'sell.listedUpdated') })}</span>
      {#if listed.visible === false && listed.orderId && onmakevisible}<button type="button" class="pick-list" data-pick={p.key ?? p.slug} onclick={() => onmakevisible(listed)} aria-label={t('sell.makeVisibleLabel', { item: p.name })}>{t('sell.makeVisible')}</button>{/if}
    </span>
  {:else}
  <span class="pick-actions">
    <button class="pick-list" data-pick={p.key ?? p.slug} disabled={!calculationReady || !canList} onclick={() => { if (calculationReady && canList) openListingFlow(p); }} aria-label={t('sell.listItem', { item: p.name })}>{t('sell.list')}</button>
    <button
      type="button"
      class="pick-snooze"
      onclick={() => snoozePick(p.key ?? p.slug)}
      aria-label={t('sell.hideItem', { item: p.name })}
      title={t('sell.hideForSession')}
    >×</button>
  </span>
  {/if}
{/snippet}

{#snippet picksEmpty()}
  <p class="muted picks-all-snoozed">{t('sell.allSnoozed')}</p>
{/snippet}

<!-- Bar 1 · scope: presets and their hint; the table adds the name filter
     and this Filters popover at the bar's end. -->
{#snippet scopeRow()}
  <div class="ui-segmented presets-row" role="group" aria-label={t('sell.scopePreset')} use:roving>
    {#each Object.keys(PRESETS) as name}
      <button
        type="button"
        aria-pressed={activePreset === name}
        onclick={() => applyPreset(name)}
        title={presetHint(name)}
      >{presetLabel(name)}</button>
    {/each}
  </div>
  <span class="muted preset-hint">
    {activePreset ? presetHint(activePreset) : t('sell.customPreset')}
  </span>
{/snippet}

{#snippet filtersControl()}
  <details class="filter-disclosure" open={filtersOpen} ontoggle={toggleFiltersOpen}>
    <summary>
      <span class="dis-label">{t('sell.filters')}</span>
      {#if activeFilterChips.length > 0}<span class="dis-count">{activeFilterChips.length}</span>{/if}
      <span class="muted small">▾</span>
    </summary>
    <div class="filters-panel">
      <button type="button" class="link" onclick={(event) => { const disclosure = event.currentTarget.closest('details'); if (disclosure) { disclosure.querySelector('summary')?.focus(); disclosure.open = false; } }}>{t('sell.closeFilters')}</button>
      <div class="filters">
        <label>
          {t('sell.minAvgPrice')}
          <input type="number" bind:value={minPrice} min="0" step="1" style="width:60px" />
          <span class="muted">p</span>
        </label>
        <label title={t('sell.minOwnedTitle')}>
          {t('sell.minOwned')}
          <input type="number" bind:value={minOwned} min="1" step="1" style="width:50px" />
        </label>
        <label title={t('sell.keepCopiesTitle')}>
          {t('sell.keepCopies')}
          <input type="number" data-testid="reserve-copies" value={reserveCopies} oninput={setReserveCopies} min="0" step="1" style="width:50px" />
        </label>
        <span class="field">
          {t('sell.type')}
          <Select label={t('sell.type')} value={typeFilter} options={[{ value: 'all', label: t('sell.allTypes') }, ...availableTypes.map((type) => ({ value: type, label: typeLabel(type) }))]} onchange={(next) => (typeFilter = next)} />
        </span>
        <label title={t('sell.hideRankedTitle')}>
          {t('sell.hideRanked')}
          <input type="number" bind:value={hideAtLvl} min="0" max="11" step="1" style="width:55px" />
        </label>
      </div>
    </div>
  </details>
{/snippet}

<!-- Bar 2 · categories as quiet text toggles, so the outlined active-filter
     chips beside them are the only outlined things on the line. -->
{#snippet categoriesRow()}
  {#if availableTags.length > 0}
    <div class="toolbar-group tagchips" role="group" aria-label={t('sell.itemTypes')} use:roving={':scope > button.chip'}>
      {#each availableTags as [tag, count]}
        <button
          type="button"
          class="chip"
          class:active={activeTags.has(tag)}
          class:zero={count === 0}
          aria-pressed={activeTags.has(tag)}
          onclick={() => toggleTag(tag)}
          title={count === 0 ? t('sell.tagNoRows') : t('sell.tagRows', { count })}
        >
          {tag === 'arcane_enhancement' ? 'arcane' : tag}
          <span class="chip-count">{count}</span>
        </button>
      {/each}
      {#if activeTags.size > 0}
        <button type="button" class="chip-clear" onclick={() => { activeTags = new Set(); focusLater(() => document.querySelector<HTMLElement>('.tagchips > button.chip')); }}>
          {t('sell.clearTags', { count: activeTags.size })}
        </button>
      {/if}
    </div>
    {#if activeTags.size >= 2}
      <span class="tag-or-note">{t('sell.tagsOr')}</span>
    {/if}
  {/if}
{/snippet}

<!-- Row B · NARROW extras: the Filters popover's state as removable chips. -->
{#snippet narrowChips()}
  {#if activeFilterChips.length > 0}
    <div class="toolbar-group active-filters" role="group" aria-label={t('sell.activeFilters')} use:roving>
      {#each activeFilterChips as f, index (f.key)}
        <button type="button" class="chip fchip" aria-label={t('sell.removeFilter', { filter: f.label })} title={t('sell.removeFilterTitle')} onclick={() => {
          f.clear();
          // The chip is gone; the next one, or the Filters summary, takes focus.
          focusLater(() => {
            const left = [...document.querySelectorAll<HTMLElement>('.active-filters .fchip')];
            return left[Math.min(index, left.length - 1)] ?? document.querySelector<HTMLElement>('.filter-disclosure summary');
          });
        }}>
          {f.label}<span class="rm" aria-hidden="true">×</span>
        </button>
      {/each}
    </div>
    <div class="toolbar-divider" aria-hidden="true"></div>
  {/if}
{/snippet}

{#snippet listCta()}
  {#if !canList}
    <button class="list-cta" data-testid="desktop-list-check" onclick={oncheckListings}>{listingActionLabel ?? t('sell.checkListings')}</button>
  {:else}
    <!-- Stays focusable when there is nothing to list, so it can say why. -->
    <button
      class="list-cta"
      data-testid="desktop-list"
      onclick={() => { if (calculationReady && listableRows.length > 0) openListingFlow(); }}
      aria-disabled={!calculationReady || listableRows.length === 0 ? 'true' : undefined}
      aria-describedby="list-cta-scope"
      title={listTitle}
    >{t('sell.listOnWfm')}</button>
    <span id="list-cta-scope" class="muted list-scope">{!calculationReady ? t('sell.stillCalculating') : listableRows.length === 0 ? t('sell.nothingToList') : t('sell.rowsToReview', { count: Math.min(stagedCount, 50) })}</span>
  {/if}
{/snippet}

{@render pendingBanner()}



{#snippet emptyState()}
  {#if calculationReady && emptyReason}
  <div class="card empty flush">
    {#if emptyReason.kind === 'no-market'}
      <div>
        <strong>{t('sell.emptyNoMarketTitle')}</strong>
        <p class="muted"><Rich text={t('sell.emptyNoMarketBody', { age: marketStaleness ?? t('sell.neverUpdated') })} /></p>
      </div>
    {:else if emptyReason.kind === 'price'}
      <div>
        <strong>{t('sell.emptyPriceTitle', { count: emptyReason.excluded ?? 0, price: minPrice })}</strong>
        <p class="muted">{t('sell.emptyPriceBody')}</p>
      </div>
      <button onclick={() => relaxFilters({ kind: 'price' })}>{t('sell.emptyPriceAction')}</button>
    {:else if emptyReason.kind === 'owned'}
      <div>
        <strong>{t('sell.emptyOwnedTitle', { count: emptyReason.excluded ?? 0, min: minOwned })}</strong>
        <p class="muted">{t('sell.emptyOwnedBody')}</p>
      </div>
      <button onclick={() => relaxFilters({ kind: 'owned' })}>{t('sell.emptyOwnedAction')}</button>
    {:else if emptyReason.kind === 'type' && activePreset !== 'spares'}
      <div>
        <strong>{t('sell.emptyTypeTitle', { type: typeLabel(typeFilter) })}</strong>
        <p class="muted">{t('sell.emptyTypeBody')}</p>
      </div>
      <button onclick={() => relaxFilters({ kind: 'type' })}>{t('sell.emptyTypeAction')}</button>
    {:else if emptyReason.kind === 'kept'}
      <div>
        <strong><Rich text={t('sell.emptyKeptTitle', { count: emptyReason.excluded ?? 0, rank: hideAtLvl })} /></strong>
        <p class="muted">{t('sell.emptyKeptBody')}</p>
      </div>
      <button onclick={() => relaxFilters({ kind: 'kept' })}>{t('sell.emptyKeptAction')}</button>
    {:else if emptyReason.kind === 'vault'}
      <div>
        <strong>{t('sell.emptyVaultTitle')}</strong>
        <p class="muted">{t('sell.emptyVaultBody')}</p>
      </div>
      <button onclick={() => relaxFilters({ kind: 'vault' })}>{t('sell.backToDefault')}</button>
    {:else if emptyReason.kind === 'vol' || emptyReason.kind === 'median'}
      <div>
        <strong>{t('sell.emptyTrendingTitle')}</strong>
        <p class="muted">{t('sell.emptyTrendingBody')}</p>
      </div>
      <button onclick={() => relaxFilters({ kind: emptyReason.kind })}>{t('sell.backToDefault')}</button>
    {:else if emptyReason.kind === 'spares' || (emptyReason.kind === 'type' && activePreset === 'spares')}
      <div>
        <strong>{t('sell.emptySparesTitle')}</strong>
        <p class="muted">{t('sell.emptySparesBody')}</p>
      </div>
      <button onclick={() => relaxFilters({ kind: 'spares' })}>{t('sell.backToDefault')}</button>
    {:else if emptyReason.kind === 'advice'}
      <div>
        <strong>{t('sell.emptyAdviceTitle')}</strong>
        <p class="muted">{t('sell.emptyAdviceBody')}</p>
      </div>
      <button onclick={() => relaxFilters({ kind: 'advice' })}>{t('sell.backToDefault')}</button>
    {:else if emptyReason.kind === 'ducats'}
      <div>
        <strong>{t('sell.emptyDucatsTitle')}</strong>
        <p class="muted">{t('sell.emptyDucatsBody')}</p>
      </div>
      <button onclick={() => relaxFilters({ kind: 'ducats' })}>{t('sell.backToDefault')}</button>
    {:else if emptyReason.kind === 'tag' && activePreset === 'sets'}
      <div>
        <strong>{t('sell.emptySetsTitle')}</strong>
        <p class="muted">{t('sell.emptySetsBody')}</p>
      </div>
      <button onclick={() => relaxFilters({ kind: 'tag' })}>{t('sell.backToDefault')}</button>
    {:else if emptyReason.kind === 'tag'}
      <div>
        <strong>{t('sell.emptyTagTitle', { count: activeTags.size, tags: [...activeTags].join(', ') })}</strong>
        <p class="muted">{t('sell.emptyTagBody')}</p>
      </div>
      <button onclick={() => relaxFilters({ kind: 'tag' })}>{t('sell.backToDefault')}</button>
    {/if}
  </div>
  {/if}
{/snippet}

<!-- Controls live inside the table border (row A SCOPE · row B NARROW); the
     picks panel is rendered by the table so both share one colgroup. When the
     cascade leaves nothing, the empty-state card takes the table body's place
     and the presets stay reachable in row A. -->
<ResultsTable {allocation} quantityStatus {estimatedGuidance} results={tableRows} {deltas} {visibleColumns} {presetSort} {columnsCustomized} {oncolumnschange}
  onfiltered={(rows, active) => (tableView = { rows, active: active || changesOnly })}
  scope={scopeRow} filters={filtersControl} categories={categoriesRow} narrow={narrowChips} cta={listCta}
  picks={calculationReady && results.length > 0 && allPicks.length > 0 ? picks : null}
  picksPending={calculationPending}
  pickReceipt={(p) => isSnoozed(p) || !!listedReceipt(p)}
  listedFor={(r) => receipts?.listedFor(r.slug, r.subtype) || null}
  {picksHead} {picksCut} {picksCutRow} pickActions={pickActionsCell} pickReason={pickReasonCell} {picksEmpty}
  empty={emptyState} />

<style>
  .picks-title { display: inline-flex; align-items: center; flex-wrap: wrap; gap: var(--s2); }
  .picks-title h3 { margin: 0; }




  .cell.since { color: var(--muted); }
  .cell.since :global(b) { color: var(--fg); font-weight: 600; font-family: var(--font-mono); }
  .changes-toggle {
    font: inherit;
    font-size: var(--text-caption);
    height: var(--ctl-xs);
    padding: 0 var(--s2);
    color: var(--muted);
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-ctl);
    cursor: pointer;
    white-space: nowrap;
  }
  .changes-toggle:hover { color: var(--fg); background: var(--panel-2); }
  .changes-toggle.on { color: var(--accent); border-color: var(--accent); background: transparent; }
  .card {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--radius-panel);
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .card.empty {
    flex-direction: row;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }
  .card.empty strong { font-weight: 600; }
  .card.empty p { margin: 4px 0 0 0; }
  .card.empty button { flex-shrink: 0; }
  .card :global(code) { background: var(--panel-2); padding: 1px 6px; border-radius: var(--radius-input); font-family: var(--font-mono); font-size: 1em; }

  /* Below this point: sell-view-exclusive, moved (not duplicated) from
     App.svelte - nothing else in the app used these selectors. */


  /* Controls inside the table border (rendered through ResultsTable's SCOPE /
     NARROW snippets). Groups wrap; heights are 28 (presets, chips). */
  .toolbar-group { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .toolbar-divider { width: 1px; height: 1rem; background: var(--hairline); flex: 0 0 auto; margin: 0 var(--s1); }
  .link { font: inherit; color: var(--accent); background: transparent; border: none; padding: 0 var(--s1); cursor: pointer; }
  .link:hover { text-decoration: underline; background: transparent; }

  .preset-hint { font-size: var(--text-caption); min-width: 0; white-space: normal; }

  /* Filters disclosure - an inline toolbar item whose panel floats as a
     popover below the summary instead of pushing the toolbar's other
     groups down. Open state persists in localStorage so power users
     don't re-click every session. */
  .filter-disclosure { position: relative; }
  .filter-disclosure > summary {
    cursor: pointer;
    list-style: none;
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-caption);
    color: var(--muted);
    height: var(--ctl);
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-ctl);
    user-select: none;
  }
  .filter-disclosure > summary:hover { color: var(--fg); background: var(--panel-2); }
  .filter-disclosure > summary::-webkit-details-marker { display: none; }
  .filter-disclosure > summary::before {
    content: '+';
    font-family: var(--font-mono);
    font-size: var(--text-control);
    color: var(--muted);
    width: 10px;
    display: inline-block;
  }
  .filter-disclosure[open] > summary::before { content: '−'; color: var(--accent); }
  .filter-disclosure[open] > summary { color: var(--accent); border-color: var(--accent); }
  .filter-disclosure > summary .dis-label {
    color: var(--fg);
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    font-size: var(--text-caption);
  }
  .filter-disclosure[open] > summary .dis-label { color: var(--accent); }
  .dis-count {
    font-family: var(--font-mono);
    font-size: var(--text-caption);
    color: var(--accent);
    border: 1px solid currentColor;
    border-radius: var(--radius-tag);
    padding: 0 4px;
    line-height: 14px;
  }
  /* Viewport anchoring keeps the panel reachable when the toolbar rewraps. */
  .filters-panel {
    position: fixed;
    bottom: 1rem;
    right: 1rem;
    z-index: var(--layer-popover);
    width: min(52rem, calc(100vw - 2rem));
    max-width: none;
    max-height: calc(100dvh - 2rem);
    overflow: auto;
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-panel);
    padding: 12px;
    box-shadow: var(--shadow-pop);
  }
  .filters { display: flex; gap: 14px; align-items: center; flex-wrap: wrap; }
  .filters :is(label, .field) {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: var(--text-caption);
    letter-spacing: 0.02em;
    color: var(--muted);
    text-transform: uppercase;
  }
  .filters input, .filters :global(.ui-select) { text-transform: none; letter-spacing: 0; }
  .filters :global(.ui-select) { min-height: var(--ctl); background: var(--bg); }


  /* First-session sell onboarding - above the stats strip, dismissed once. */
  .sell-onboarding {
    flex-direction: row;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }
  .sell-onboarding .so-body { min-width: 0; }
  .sell-onboarding strong { color: var(--fg); font-weight: 600; font-size: var(--text-control); }
  .sell-onboarding p :global(b) { color: var(--fg); font-weight: 600; font-size: var(--text-control); }
  .sell-onboarding p { margin: 4px 0 0; font-size: var(--text-control); line-height: 1.5; }
  .sell-onboarding .dismiss {
    background: transparent;
    border: none;
    color: var(--muted);
    font-size: var(--text-section);
    line-height: 1;
    cursor: pointer;
    padding: 3px;
    flex-shrink: 0;
  }
  .sell-onboarding .dismiss:hover { color: var(--fg); }

  /* Keep-copies nudge - one-time education after the onboarding card is
     dismissed; reappearing until dismissed is the intent (it's a habit, not
     a task). */
  .keep-nudge {
    flex-direction: row;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    border-left: 3px solid var(--warn);
  }
  .keep-nudge .kn-body { min-width: 0; }
  .keep-nudge strong { color: var(--fg); font-weight: 600; font-size: var(--text-control); }
  .keep-nudge .muted { font-size: var(--text-control); line-height: 1.5; }
  .keep-nudge :global(code) { font-family: var(--font-mono); font-size: 1em; }
  .keep-nudge .dismiss {
    background: transparent;
    border: none;
    color: var(--muted);
    font-size: var(--text-section);
    line-height: 1;
    cursor: pointer;
    padding: 3px;
    flex-shrink: 0;
  }
  .keep-nudge .dismiss:hover { color: var(--fg); }

  /* Tag chip row - pills, OR-combined among themselves, AND with the
     filters row above. Inactive chips show the live row-count next to
     the tag so the user can see what's worth toggling. Zero-count chips
     stay visible (strikethrough+muted) so vocabulary is discoverable.
     Chip row caps at ~96px (≈3 wrap rows on desktop, ≈4 on mobile) with
     internal vertical scroll. */
  /* Type chips take their own line under the presets - real inventories carry
     10–20 tags, which cannot share a 40px line with six presets. */
  .tagchips { gap: 0 var(--s2); }
  .chip {
    font: inherit;
    display: inline-flex;
    align-items: center;
    gap: var(--s1);
    /* Keeps a 24px target while drawing as text. */
    min-height: var(--ctl-xs);
    padding: 0 var(--s1);
    font-size: var(--text-control);
    color: var(--fg);
    background: transparent;
    border: 0;
    border-bottom: 2px solid transparent;
    border-radius: 0;
    cursor: pointer;
  }
  .chip:hover { border-bottom-color: var(--border); }
  .chip.active { border-bottom-color: var(--fg); font-weight: 600; }
  .chip.zero { color: var(--muted); text-decoration: line-through; cursor: default; }
  /* Active filters are state you can remove, so they keep an outline. */
  .chip.fchip { border: 1px solid var(--border); padding: 0 var(--s2); font-size: var(--text-caption); }
  .chip.fchip:hover { border-color: var(--fg); }
  .chip-count { font: var(--text-caption) var(--font-mono); color: var(--muted); }
  .chip-clear {
    background: transparent;
    border: none;
    color: var(--muted);
    font-size: var(--text-caption);
    cursor: pointer;
    padding: 3px 8px;
  }
  .chip-clear:hover { color: var(--bad); }

  /* Primary CTA inside the presets row - pushed to the far right via
     margin-left:auto so it doesn't visually mix with the chip-style
     presets next to it. Same colour family as the accent. */
  .list-cta {
    height: var(--ctl-lg);
    padding: 0 var(--s4);
    background: var(--accent);
    color: var(--on-accent);
    border: 1px solid var(--accent);
    font-weight: 600;
    white-space: nowrap;
  }
  .list-cta:hover:not(:disabled) { filter: brightness(1.1); }


  .picks-exp { font-size: var(--text-caption); }
  .picks-count { font-size: var(--text-caption); white-space: nowrap; }
  .picks-trades { font-size: var(--text-caption); display: inline-flex; flex-wrap: wrap; align-items: center; gap: var(--s1) var(--s2); }
  /* One cell per trade of the day: filled = the picks, outlined = free, faint = already traded. */
  .trade-gauge { display: inline-flex; flex-wrap: wrap; gap: 2px; }
  .trade-gauge i { display: block; width: 0.25rem; height: 0.75rem; }
  .trade-gauge i.plan { background: var(--accent); }
  .trade-gauge i.free { box-shadow: inset 0 0 0 1px var(--accent); }
  .trade-gauge i.spent { background: var(--hairline); }
  .cut-list { margin-left: var(--s3); }
  .pick-trade-count { display: block; font-family: var(--font-mono); font-size: var(--text-caption); color: var(--muted); }
  .cut-label { font-family: var(--font-ui); font-size: var(--text-caption); font-weight: 600; letter-spacing: 0.12em; text-transform: uppercase; margin-right: var(--s3); }
  .picks-all-snoozed { margin: 0; }
  /* Reasons wrap while the adjacent actions retain their own space. */
  .rs { display: flex; align-items: center; gap: var(--s2); min-height: var(--row); font-size: var(--text-control); color: var(--muted); }
  .rs .t { flex: 1 1 0; min-width: 0; white-space: normal; }
  /* Timing tint mirrors .tag.hold/.tag.peak in ResultsTable - same signal,
     same colour, so the picks and the table below read as one vocabulary. */
  .rs.hold .t { color: var(--warn); }
  .rs.peak .t { color: var(--good); }
  /* Only the number is data; the unit reads as prose. */
  .rs > .tag { flex-shrink: 0; margin-inline-start: 0; }
  .pick-actions { display: inline-flex; align-items: center; gap: 4px; flex-shrink: 0; }
  .pick-list { font-size: var(--text-caption); height: var(--ctl-xs); padding: 0 10px; }
  .pick-snooze {
    background: transparent;
    border: none;
    color: var(--muted);
    font-size: var(--text-section);
    line-height: 1;
    cursor: pointer;
    padding: 0 6px;
    height: var(--ctl-xs);
    min-width: 24px;
  }
  .pick-snooze:hover { color: var(--bad); }
  .card.empty.flush { border: none; padding: var(--s2) 0; background: transparent; }

  @media (max-width: 35rem) {
    .cell.since { flex-basis: 100%; }
    .card.empty { align-items: flex-start; flex-direction: column; }
    .card.empty button { align-self: stretch; }
    .filters-panel {
      position: fixed;
      top: auto;
      right: 1rem;
      bottom: 1rem;
      left: 1rem;
      width: auto;
      max-height: calc(100dvh - 2rem);
    }
    .filters { align-items: stretch; flex-direction: column; }
    .filters label { justify-content: space-between; }
    .list-cta { width: 100%; }
    .rs { height: auto; min-height: var(--row); flex-wrap: wrap; padding: var(--s1) 0; }
    .rs .t { flex-basis: calc(100% - 2rem); white-space: normal; overflow: visible; }
  }
</style>
