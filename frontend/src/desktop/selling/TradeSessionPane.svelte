<script lang="ts">
  import { fitTable } from '../../ui/table-fit';
  import { useDesktopServices } from '../shell/desktop-context';
  const { desktopTradeSessionState, desktopLiveTopPrices, listenForTauriEvent, evaluateTradeSession } = useDesktopServices();
  import { onMount, onDestroy, untrack, type Snippet } from 'svelte';
  import BuyerAlternatives from './BuyerAlternatives.svelte';
  import { busy, focusLater, roving } from '../../ui/keyboard-actions';
  import { LiveTopController } from './live-top.svelte';
  const liveTop = new LiveTopController({ desktopLiveTopPrices, listenForTauriEvent });
  onDestroy(() => liveTop.dispose());
  import { computeResults } from '../../domain/filter-engine';
  import { SESSION_MODES, type SessionPlan, type SessionMode, type SessionRow, type SessionCandidate } from '../../domain/trade-session';
  
import { ALLOWANCE_CHANGED_EVENT } from '../../contracts/events';
  
  import { humanError } from '../../contracts/errors';
  import { shortStamp } from '../../ui/format';
  import { formatLocale, t } from '../../i18n/i18n.svelte';
  import type { MessageKey } from '../../i18n/en';
  const MODE_TEXT: Record<SessionMode, [MessageKey, MessageKey]> = {
    fast: ['session.mode.fast', 'session.mode.fast.what'],
    'per-trade': ['session.mode.per-trade', 'session.mode.per-trade.what'],
    clear: ['session.mode.clear', 'session.mode.clear.what'],
    max: ['session.mode.max', 'session.mode.max.what'],
  };
  const CONFIDENCE: Record<string, MessageKey> = { scanned: 'session.confidence.scanned', tracked: 'session.confidence.tracked', estimated: 'session.confidence.estimated', unknown: 'session.confidence.unknown' };
  const confidenceWord = (value: string) => (CONFIDENCE[value] ? t(CONFIDENCE[value]) : value);
  import { MAX_PLAN_ITEMS } from '../../domain/limits';
  import { cachedUnitMarket } from '../../domain/order-prices';
  import type { Market, MarketItemEntry, OwnedRecord, TradeSessionState } from '../../contracts/data';
  import type { ScoredInventoryFact } from '../../contracts/generated/domain';
  import type { Verdict } from '../../contracts/generated/domain';

  let { owned, market, reserveCopies, advice, scanning, onscan, onreview, nativeFacts = new Map(), availability, listingBlockReason = null, onrecheck, listingActionLabel, keep, active = true, sessionEpoch = 0 }: {
    keep?: Snippet;
    /** Bumped when the WFM session changes; live prices belong to one account. */
    sessionEpoch?: number;
    /** Whether the view is on screen; it stays mounted while hidden. */
    active?: boolean;
    owned: Map<string, OwnedRecord>; market: Market | null; reserveCopies: number;
    nativeFacts?: Map<string, ScoredInventoryFact>;
    advice: Map<string, Verdict>; scanning: boolean; onscan: () => Promise<void>;
    onreview: (rows: SessionRow[], budget: number, state: TradeSessionState) => void;
    availability?: ReadonlyMap<string, number>;
    listingBlockReason?: string | null;
    onrecheck?: () => void;
    listingActionLabel?: string;
  } = $props();
  let mode = $state<SessionMode>('fast');
  let budget = $state<number | undefined>();
  let target = $state<number | undefined>();
  let sessionData = $state<TradeSessionState | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(true);
  let liveBusy = $derived(liveTop.phase === 'running');
  let priceNote = $state<string | null>(null);
  let livePrices = $state<Record<string, Partial<MarketItemEntry>>>({});
  let priceSources = $state<Record<string, string>>({});
  let buyerSelection = $state<SessionRow | null>(null);
  const buyerButton = (key: string) => () => [...document.querySelectorAll<HTMLElement>('.compare-buyers')].find((el) => el.dataset.row === key);
  // The comparison opens below the table; focus goes to it so it is seen and
  // read, and back to the row's button when it closes.
  function compareBuyers(row: SessionRow) {
    buyerSelection = { ...row };
    focusLater(() => document.querySelector<HTMLElement>('.buyer-alternatives h3'));
  }
  function closeBuyers() {
    const key = buyerSelection?.key;
    buyerSelection = null;
    if (key) focusLater(buyerButton(key));
  }
  let disposed = false;
  let request = 0;
  let budgetInitialized = false;
  let lastPositiveBudget = 8;
  let includeSets = $state(true);
  const filters = { minPrice: 0, minOwned: 0, typeFilter: 'all', hideAtLvl: Infinity,
    activeTags: new Set<string>(), vaultOnly: false, ducatsOnly: false, minVol: 0,
    minMedian: 0, typesAny: [], sparesOnly: false, adviceOnly: false };
  let parts = $derived(computeResults(owned, market, filters, reserveCopies, advice, availability, nativeFacts).filter(r => !r.slug.endsWith('_set')).map(r => ({
    key: r.key, slug: r.slug, name: r.name, owned: r.owned, leveled: r.leveled,
    sellable: Math.min(r.sellable, sessionData?.quantities[r.slug] ?? 0), subtype: r.subtype,
    type: r.type, hold: r.timing === 'hold' || r.advice === 'hold',
    bulk: sessionData?.bulk_slugs.includes(r.slug) ?? false,
    supported: sessionData?.supported_slugs == null ? undefined : sessionData.supported_slugs.includes(r.slug),
    market: { ...cachedUnitMarket(market!.items[r.slug], sessionData?.bulk_slugs.includes(r.slug)), ...livePrices[r.slug] },
  })));
  let candidates = $derived.by<SessionCandidate[]>(() => {
    if (!includeSets || !market || !sessionData) return parts;
    const sets: SessionCandidate[] = [];
    for (const [slug, components] of Object.entries(sessionData.set_recipes ?? {})) {
      const recipe = Object.entries(components);
      const rows = recipe.map(([part]) => parts.filter(row => row.slug === part && !row.subtype));
      if (!recipe.length || rows.some(rows => rows.length !== 1) || !market.items[slug]) continue;
      const sellable = Math.min(...recipe.map(([, count], i) => Math.floor(rows[i][0].sellable / count)));
      if (sellable < 1) continue;
      sets.push({ key: slug, slug, name: `${market.set_to_parts?.[slug]?.name ?? slug} Set`,
        owned: Math.min(...recipe.map(([, count], i) => Math.floor(rows[i][0].owned / count))),
        sellable, leveled: 0, subtype: null, type: 'Set', hold: advice.get(slug)?.advice === 'hold',
        bulk: false, supported: true, components, market: { ...market.items[slug], ...livePrices[slug] } });
    }
    return [...parts, ...sets];
  });
  let plan = $state<SessionPlan>({ rows: [], trades: 0, total: 0, excluded: [], target: null, shortfall: null });
  let planning = $state(false);
  let planningError = $state<string | null>(null);
  $effect(() => {
    // Until the session loads every quantity is 0, so there is nothing to plan.
    if (!sessionData) return;
    const input = { candidates, mode, budget: budget ?? 0, target: target ?? null };
    let cancelled = false;
    planning = true;
    planningError = null;
    void evaluateTradeSession(input).then(next => {
      if (!cancelled) plan = next;
    }).catch(e => {
      if (!cancelled) planningError = humanError(e);
    }).finally(() => {
      if (!cancelled) planning = false;
    });
    return () => { cancelled = true; };
  });
  let cap = $derived(Math.min(MAX_PLAN_ITEMS, sessionData?.allowance.remaining ?? 0));
  let retained = $state<SessionPlan | null>(null);
  $effect(() => { if (plan.rows.length > 0) retained = plan; });
  let display = $derived(cap === 0 && retained ? retained : plan);
  let allowance = $derived(sessionData?.allowance);

  function partsAsk(components: Record<string, number>): number | null {
    let total = 0;
    for (const [slug, count] of Object.entries(components)) {
      const ask = parts.find(part => part.slug === slug)?.market.low_sell;
      if (ask == null || !Number.isFinite(ask) || ask <= 0) return null;
      total += ask * count;
    }
    return total;
  }

  async function refresh() {
    const current = ++request;
    try {
      const next = await desktopTradeSessionState();
      if (disposed || current !== request) return;
      if (!next?.allowance || !next.quantities || !Array.isArray(next.bulk_slugs)) throw new Error(t('session.dataUnavailable'));
      if (sessionData && sessionData.allowance.snapshot_id !== next.allowance.snapshot_id) retained = null;
      sessionData = next;
      const maximum = Math.min(MAX_PLAN_ITEMS, next.allowance.remaining ?? 0);
      if (!budgetInitialized && maximum > 0) {
        budget = Math.min(8, maximum);
        budgetInitialized = true;
      } else if (budget != null) {
        budget = Math.min(budget === 0 ? lastPositiveBudget : budget, maximum);
      }
      if (budget != null && budget > 0) lastPositiveBudget = budget;
      error = null;
    } catch (e) {
      if (!disposed && current === request) error = humanError(e);
    } finally { if (!disposed && current === request) loading = false; }
  }

  async function scan() { await onscan(); await refresh(); }

  async function refreshPrices() {
    priceNote = null;
    const rows = plan.rows;
    const quotes = await liveTop.check(rows.map(r => ({ slug: r.slug, rank: 0, subtype: null })));
    if (disposed) return;
    if (!quotes) { priceNote = liveTop.error; return; }
    let failed = 0;
    const next = { ...livePrices };
    const sources = { ...priceSources };
    for (const row of rows) {
      const q = quotes.find(q => q.slug === row.slug && (q.rank ?? 0) === 0 && q.subtype == null);
      if (!q || q.error) { failed++; continue; }
      next[q.slug] = { low_sell: q.low_sell ?? 0, top_buy: q.top_buy ?? 0, price_basis: 'unit' };
      sources[q.slug] = t(q.low_sell == null ? 'session.noLiveAsk' : 'session.liveChecked', { time: new Date().toLocaleTimeString(formatLocale()) });
    }
    livePrices = next;
    priceSources = sources;
    priceNote = failed ? t('session.checksFailed', { count: failed }) : t('session.competitorsChecked');
  }

  // Live prices were fetched for the previous account (its platform, its own
  // orders excluded), so a new session starts from the cached snapshot again.
  let pricesEpoch = untrack(() => sessionEpoch);
  $effect(() => {
    const epoch = sessionEpoch;
    untrack(() => {
      if (epoch === pricesEpoch) return;
      pricesEpoch = epoch;
      liveTop.reset();
      livePrices = {};
      priceSources = {};
      priceNote = null;
    });
  });

  // Back on screen after being hidden: refresh at once rather than at the next poll.
  let wasActive = untrack(() => active);
  $effect(() => {
    const onScreen = active;
    untrack(() => { if (onScreen && !wasActive) void refresh(); wasActive = onScreen; });
  });

  onMount(() => {
    void refresh();
    const stop = listenForTauriEvent(ALLOWANCE_CHANGED_EVENT, () => void refresh());
    const timer = setInterval(() => { if (active) void refresh(); }, 10_000);
    return () => { disposed = true; clearInterval(timer); stop(); };
  });
</script>

<section class="view-header">
  <h2>{t('session.title')}</h2>
  <p class="lede">{t('session.lede')}</p>
</section>
{@render keep?.()}

<div class="ui-stack">
  <section class="ui-panel session-setup" aria-label={t('session.setup')}>
    <div class="ui-toolbar planning-controls">
      <div class="ui-field">
        <span>{t('session.goal')}</span>
        <div class="ui-segmented modes" role="group" aria-label={t('session.intent')} use:roving>
          {#each SESSION_MODES as choice}
            <button aria-pressed={mode === choice.id} title={t(MODE_TEXT[choice.id][1])}
              disabled={cap === 0 && !!retained} onclick={() => mode = choice.id}>{t(MODE_TEXT[choice.id][0])}</button>
          {/each}
        </div>
      </div>
      <label class="ui-field budget-field">{t('session.budget')}
        <input class="ui-input" aria-label={t('session.budget')} type="number" min="1" max={cap} step="1"
          bind:value={budget} disabled={cap === 0} aria-describedby="budget-limit"
          aria-invalid={cap > 0 && (budget == null || budget < 1 || budget > cap) ? 'true' : undefined}
          onchange={() => { if (budget != null && cap > 0) budget = Math.min(cap, Math.max(1, Math.round(budget))); }} />
      </label>
      <span class="muted" id="budget-limit">{t(cap > 0 && cap === allowance?.remaining ? 'session.cantExceed' : 'session.upTo', { count: cap })}</span>
    </div>
    <p class="muted">{t(MODE_TEXT[mode][1])}</p>
    <details class="session-options">
      <summary>{t('session.targetAndSets')}</summary>
      <div class="ui-toolbar">
        <label class="ui-field">{t('session.target')}
          <input class="ui-input" aria-label={t('session.targetLabel')} type="number" min="1" step="1" bind:value={target} placeholder={t('session.noTarget')} disabled={cap === 0 && !!retained} />
        </label>
        <label class="sets-check"><input type="checkbox" bind:checked={includeSets} disabled={cap === 0 && !!retained} /> {t('session.includeSets')}</label>
      </div>
    </details>
    <div class="ui-toolbar allowance-status">
      <strong>
        {#if loading}{t('session.readingAllowance')}
        {:else if allowance?.remaining != null}
          {t(allowance.confidence === 'estimated' ? 'session.aboutTradesLeft' : 'session.tradesLeft', { count: allowance.remaining })}{#if allowance.mastery_rank != null}{t('session.mr', { rank: allowance.mastery_rank })}{/if}
        {:else}{t('session.allowanceUnknown')}{/if}
      </strong>
      <button class="btn" onclick={scan} disabled={scanning}>{t(scanning ? 'session.scanning' : 'session.scan')}</button>
      {#if error}<button class="btn ghost" onclick={refresh}>{t('session.retry')}</button>{/if}
    </div>
    {#if allowance?.observed_at}
      <p class="muted">{t('session.lastScan', { time: shortStamp(allowance.observed_at * 1000), confidence: confidenceWord(allowance.confidence) })}{#if allowance.monitoring}{t('session.tracking')}{/if}</p>
    {/if}
    {#if allowance?.reason}<p class="muted">{allowance.reason}</p>{/if}
    {#if error}<p class="ui-notice" data-tone="bad" role="alert">{error}</p>{/if}
    {#if !loading && allowance?.remaining === 0}
      <p class="ui-notice" data-tone="warn">{t('session.noTrades')}</p>
    {/if}
  </section>

  <section class="wrap tw session-batch" aria-label={t('session.batch')}>
    <div class="rail"><h3>{t('session.batch')}</h3><span class="exp">{t('session.reviewFirst')}</span></div>
    <div class="ui-stack session-body">
    {#if listingBlockReason}<p class="ui-notice" data-tone="warn" role="status">{listingBlockReason} {#if onrecheck}<button class="btn" onclick={onrecheck}>{listingActionLabel ?? t('sell.checkListings')}</button>{/if}</p>{/if}
    <div class="ui-toolbar">
      <h3>{t('session.summary', { listings: display.rows.length, trades: display.trades, total: display.total.toLocaleString(formatLocale()) })}</h3>
      <button class="btn ghost" onclick={refreshPrices} disabled={planning || !!planningError || !plan.rows.length || cap === 0} use:busy={liveBusy}>
        {t(liveBusy ? 'session.checkingPrices' : 'session.checkPrices')}
      </button>
      <button class="btn primary" disabled={!!listingBlockReason || loading || planning || !!planningError || !!error || cap === 0 || !plan.rows.length || (budget ?? 0) > cap || liveBusy}
        onclick={() => !listingBlockReason && sessionData && onreview(plan.rows, budget ?? 0, sessionData)}>{t('session.review')}</button>
    </div>
    <p class="muted">{t('session.potentialNote', { age: market?.updated_at ? shortStamp(market.updated_at) : t('session.unknownAge') })}</p>
    {#if planning}<p role="status">{t('session.updating')}</p>{/if}
    {#if planningError}<p class="ui-notice" data-tone="bad" role="alert">{t('session.planError', { error: planningError })}</p>{/if}
    {#if priceNote}<p role="status">{priceNote}</p>{/if}
    {#if display.target != null}
      <p class="ui-notice" data-tone={display.shortfall === 0 ? 'good' : 'warn'}>
        {display.shortfall === 0 ? t('session.covered') : t('session.short', { amount: display.shortfall?.toLocaleString(formatLocale()) ?? '' })}
      </p>
    {/if}
    {#if display.rows.length}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex (Keyboard focus is needed to scroll the wide table.) -->
      <div class="wrap tw"><div class="scroll" use:fitTable tabindex="0" role="region" aria-label={t('session.tableLabel')}>
        <table class="tw session-table">
          <thead><tr><th class="l">{t('session.col.item')}</th><th title={t('session.col.qtyTitle')}>{t('session.col.qty')}</th><th title={t('session.col.perTradeTitle')}>{t('session.col.perTrade')}</th><th title={t('session.col.tradesTitle')}>{t('session.col.trades')}</th><th>{t('session.col.ask')}</th><th data-drop="1">{t('session.col.bid')}</th><th>{t('session.col.potential')}</th><th class="l">{t('session.col.why')}</th></tr></thead>
          <tbody>
            {#each display.rows as row (row.key)}
              <tr>
                <td class="l">{row.name}<small>{t('session.rowCounts', { sellable: row.sellable, owned: row.owned })}</small>
                  {#if row.components}
                    {@const reference = partsAsk(row.components)}
                    <small>{t('session.perSet', { parts: Object.entries(row.components).map(([slug, count]) => `${parts.find(part => part.slug === slug)?.name ?? slug} ×${count}`).join(', ') })}</small>
                    <small>{t('session.partsReference', { reference: reference == null ? t('session.unknown') : `${Number(reference.toFixed(2))}p`, ask: row.platinum })}</small>
                  {/if}
                  <button class="btn ghost xs compare-buyers" data-row={row.key} aria-expanded={buyerSelection?.key === row.key} aria-label={t('session.compareBuyersFor', { item: row.name })} onclick={() => compareBuyers(row)}>{t('session.compareBuyers')}</button>
                </td>
                <td>{row.quantity}</td><td>{row.per_trade}</td><td>{row.trades}</td><td>{row.platinum}p<small>{priceSources[row.slug] ?? t('session.cachedReference')}</small></td>
                <td>{row.bid == null ? t('session.unknown') : `${Number(row.bid.toFixed(2))}p`}</td><td>{row.quantity * row.platinum}p</td><td class="reason">{row.reason}</td>
              </tr>
            {/each}
          </tbody>
        </table></div>
      </div>
      <p class="muted">{t('session.tradesNote')}</p>
    {:else if !loading && !planning && !planningError}
      <p>{t('session.noBatch')}</p>
    {/if}
    {#if plan.excluded.length}
      <details><summary>{t('session.excluded', { count: plan.excluded.length })}</summary>
        <ul>{#each plan.excluded as row}<li>{row.name}: {row.reason}</li>{/each}</ul>
      </details>
    {/if}
    </div>
  </section>
  {#if buyerSelection}
    {#key buyerSelection}
      <BuyerAlternatives slug={buyerSelection.slug} name={buyerSelection.name} quantity={buyerSelection.quantity}
        ask={buyerSelection.platinum} onclose={closeBuyers} />
    {/key}
  {/if}
</div>

<style>
  .planning-controls { align-items: end; gap: var(--s3) var(--s4); }
  .session-setup { gap: var(--s2); }
  .budget-field input { width: 5rem; }
  .session-options { padding-block: var(--s2); border-block: 1px var(--rule) var(--hairline); }
  .session-options > .ui-toolbar { margin-top: var(--s2); gap: var(--s4); align-items: end; }
  .sets-check { display: flex; align-items: center; gap: var(--s2); min-height: var(--ctl-lg); }
  .allowance-status { font-size: var(--text-control); }
  .session-body { padding: var(--s3) var(--inset); }
  /* Sized by content, so headers and figures are never cut off; Item takes
     the rest and Why wraps when room runs short. Below the 1200px window the
     table scrolls in its box. */
  .session-table th.l:first-child { width: 100%; min-width: 9rem; }
  .session-table th.l:last-child { width: auto; min-width: 8rem; }
  .reason { min-width: 18rem; white-space: normal; font-family: var(--font-body); }
  small { display: block; font-family: var(--font-body); font-size: var(--text-caption); color: var(--muted); line-height: var(--leading-body); white-space: normal; }
  h3 { font-size: var(--text-section); margin: 0; }
  p { margin: 0; }
  .ui-input { max-width: 12rem; }
</style>
