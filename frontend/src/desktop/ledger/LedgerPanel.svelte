<script lang="ts">
  import { busy } from '../../ui/keyboard-actions';
  import { fitTable } from '../../ui/table-fit';
  import { useDesktopServices } from '../shell/desktop-context';
  const { desktopEelogStatus, desktopListTrades, desktopListingOutcomes, desktopSetListingOutcomes, listenForTauriEvent } = useDesktopServices();
  import { onMount, onDestroy } from 'svelte';

import { RECORDING_CHANGED_EVENT, TRADE_DETECTED_EVENT } from '../../contracts/events';
import { type EeLogStatus, type ListingOutcomes, type TradeDetected, type TradeRow } from '../../contracts/desktop';

  import { totals, since, soldByItem, describeItems, topSellers, weeklySales, medianWeek } from '../../domain/ledger';
  import { outcomeRows, outcomeHeadline } from './outcomes';
  import { formatLocale, t as tr } from '../../i18n/i18n.svelte';
  import Rich from '../../ui/Rich.svelte';
  import { humanError } from '../../contracts/errors';
  import Toast from '../../ui/Toast.svelte';
  import Pager from '../../ui/Pager.svelte';
  import Meter from '../../ui/Meter.svelte';
  import { pageWindow } from '../../ui/paging';
  import { plat, shortStamp } from '../../ui/format';
  import { createToastQueue } from '../../ui/toast-queue.svelte';

  interface Props {
    /** Persist the auto-close preference (App writes the `auto-close-sold`
     *  setting the Rust tailer reads). */
    onsetautoclose?: (on: boolean) => Promise<void> | void;
  }
  let { onsetautoclose }: Props = $props();

  let trades = $state<TradeRow[]>([]);
  let status = $state<EeLogStatus | null>(null);
  let loadError = $state<string | null>(null);
  let autoClose = $state(true);
  let savingAutoClose = $state(false);
  /** Null until read, and when the read failed: the ledger must not fail with it. */
  let outcomes = $state<ListingOutcomes | null>(null);
  let savingOutcomes = $state(false);
  let loadGeneration = 0;
  let disposed = false;

  const toastQueue = createToastQueue(5000);
  const pushToast = toastQueue.push;

  /**
   * Recording health, so a pause is visible to someone who opens this surface
   * after it happened - the notification is sent once and cannot answer for the
   * present. Never a toast: a pause lasts until it is fixed, and a toast would
   * expire while the condition did not.
   */
  const paused = $derived(
    status && typeof status.recording === 'object' ? status.recording.paused : null,
  );
  const ledgerPaused = $derived(paused !== null && 'ledger' in paused);
  const pausedReason = $derived(
    paused === null ? '' : 'ledger' in paused ? paused.ledger.error : paused.log.error,
  );
  onDestroy(() => {
    disposed = true;
    toastQueue.dispose();
  });

  async function load(): Promise<void> {
    const generation = ++loadGeneration;
    try {
      const [rows, st, listed] = await Promise.all([
        desktopListTrades(500),
        desktopEelogStatus(),
        desktopListingOutcomes().catch(() => null),
      ]);
      if (disposed || generation !== loadGeneration) return;
      trades = rows;
      status = st;
      outcomes = listed;
      autoClose = st.auto_close;
      loadError = null;
    } catch (e) {
      if (disposed || generation !== loadGeneration) return;
      loadError = humanError(e);
    }
  }

  onMount(() => {
    void load();
    const offTrade = listenForTauriEvent<TradeDetected>(TRADE_DETECTED_EVENT, (d) => {
      const kind = d.trade.kind;
      const head = kind === 'sale' ? tr('ledger.soldFor', { price: d.trade.plat }) : kind === 'purchase' ? tr('ledger.boughtFor', { price: d.trade.plat }) : tr('ledger.tradeDone');
      const adjusted = d.adjusted.length ? tr('ledger.listingsUpdated', { count: d.adjusted.length }) : '';
      pushToast(tr('ledger.tradeToast', { head, items: describeItems(d.trade), partner: d.trade.partner, adjusted }));
      void load();
    });
    // A pause emits no trade event, so without this the notice would only appear
    // on the next mount - after the user had already completed trades it missed.
    const offRecording = listenForTauriEvent(RECORDING_CHANGED_EVENT, () => void load());
    return () => {
      disposed = true;
      offTrade();
      offRecording();
    };
  });

  async function toggleAutoClose(): Promise<void> {
    if (!onsetautoclose || savingAutoClose) return;
    savingAutoClose = true;
    const next = !autoClose;
    try {
      await onsetautoclose(next);
      autoClose = next;
    } catch (e) {
      pushToast(tr('ledger.saveFailed', { error: humanError(e) }), 'error');
    } finally {
      savingAutoClose = false;
    }
  }

  async function toggleOutcomes(): Promise<void> {
    if (!outcomes || savingOutcomes) return;
    savingOutcomes = true;
    try {
      await desktopSetListingOutcomes(!outcomes.enabled);
      outcomes = await desktopListingOutcomes();
    } catch (e) {
      pushToast(tr('ledger.saveFailed', { error: humanError(e) }), 'error');
    } finally {
      savingOutcomes = false;
    }
  }

  let outcomeTable = $derived(outcomes ? outcomeRows(outcomes) : []);

  const nowSecs = () => Math.floor(Date.now() / 1000);
  let all = $derived(totals(trades));
  let week = $derived(totals(since(trades, 7, nowSecs())));
  let weeks = $derived(weeklySales(trades, 12, Date.now()));
  let weekMedian = $derived(medianWeek(weeks));
  let weekMax = $derived(Math.max(1, ...weeks.map((w) => w.plat)));
  let sellers = $derived(topSellers(soldByItem(trades), 8));
  let sellerMax = $derived(Math.max(1, sellers.top[0]?.plat ?? 0));
  const weekLabel = (ms: number) => new Date(ms).toLocaleDateString(formatLocale(), { month: 'short', day: 'numeric' });
  const weekTitle = (w: (typeof weeks)[number]) =>
    tr(w.partial ? 'ledger.weekTitlePartial' : 'ledger.weekTitle', { date: weekLabel(w.start), plat: plat(w.plat), count: w.sales });
  let page = $state(0);
  let historyEl = $state<HTMLElement>();
  let win = $derived(pageWindow(trades.length, page));
  let shown = $derived(trades.slice(win.start, win.end));

  const when = (unixSecs: number) => shortStamp(unixSecs * 1000);
  function kindLabel(k: TradeRow['kind']): string {
    return tr(k === 'sale' ? 'ledger.kind.sale' : k === 'purchase' ? 'ledger.kind.purchase' : 'ledger.kind.trade');
  }
  function platCell(t: TradeRow): string {
    if (t.kind === 'sale') return `+${plat(t.plat)}p`;
    if (t.kind === 'purchase') return `−${plat(t.plat)}p`;
    return '-';
  }
  const signed = (n: number) => `${n > 0 ? '+' : n < 0 ? '−' : ''}${plat(Math.abs(n))}p`;
</script>

<section class="ui-stack ledger" data-testid="ledger">
  <!-- Totals sit in the header strip like every other view's; the sold and
       bought counts behind them are on the history rail. -->
  <header class="view-header">
    <h2>{tr('ledger.title')}</h2>
    <p class="lede">{tr('ledger.lede')}</p>
    <div class="vh-end">
      {#if !loadError && trades.length}
        <div class="ui-totals" role="group" aria-label={tr('ledger.summary')}>
          <div class="cell" title={tr('ledger.soldBought', { sold: all.sales, bought: all.purchases })}><span class="k">{tr('ledger.netAllTime')}</span><span class="v" class:good={all.net > 0} class:bad={all.net < 0}>{signed(all.net)}</span></div>
          <div class="cell" title={tr('ledger.soldBought', { sold: week.sales, bought: week.purchases })}><span class="k">{tr('ledger.last7')}</span><span class="v" class:good={week.net > 0} class:bad={week.net < 0}>{signed(week.net)}</span></div>
          <div class="cell" title={tr('ledger.inOutTitle')}><span class="k">{tr('ledger.inOut')}</span><span class="v">{plat(all.platIn)}p / {plat(all.platOut)}p</span></div>
        </div>
      {/if}
      <button class="btn ghost" onclick={load}>{tr('ledger.refresh')}</button>
    </div>
  </header>
  {#if status}
    {#if !status.path}
      <p class="ui-notice" data-tone="warn">
        <Rich text={tr('ledger.noLog')} />
      </p>
    {/if}
    {#if paused}
      <p class="ui-notice" data-tone="warn" role="status">
        {#if ledgerPaused}
          <Rich text={tr('ledger.pausedSave')} /> <span class="muted">({pausedReason})</span>
        {:else}
          <Rich text={tr('ledger.pausedRead', { reason: pausedReason ?? '' })} /> <span class="muted">{tr('ledger.restartHint')}</span>
        {/if}
      </p>
    {/if}
    <!-- One setting, so one line rather than a titled panel. -->
    <div class="setting-line" role="group" aria-label={tr('ledger.automation')}>
      <label class="check" for="ledger-auto-close"><input id="ledger-auto-close" type="checkbox" checked={autoClose} onchange={toggleAutoClose} disabled={!status.path} use:busy={savingAutoClose} /> {tr('ledger.autoClose')}</label>
      <span class="muted">{tr('ledger.autoCloseNote')}</span>
    </div>
  {/if}


  <!-- Answers "is selling working?" without opening anything. Bars start at
       zero on one scale; the running week is dimmed so a partial week does
       not read as a drop. The table view carries the exact values. -->
  {#if !loadError && trades.length}
    <div class="dash">
      <section class="wrap tw" aria-labelledby="ledger-weeks-title">
        <div class="rail"><h3 id="ledger-weeks-title">{tr('ledger.byWeek')}</h3><span class="exp">{weeks.length === 1 ? tr('ledger.thisWeek') : tr('ledger.lastWeeks', { count: weeks.length })}</span></div>
        <div class="dash-body">
          <div class="weeks" role="img" aria-label={tr('ledger.chartLabel', { count: weeks.length })}>
            {#if weekMedian !== null && weekMedian > 0}<div class="median" style:bottom="{(weekMedian / weekMax) * 100}%"></div>{/if}
            {#each weeks as w (w.start)}
              <div class="slot" title={weekTitle(w)}><div class="wbar" class:partial={w.partial} style:height="{(w.plat / weekMax) * 100}%"></div></div>
            {/each}
          </div>
          <div class="axis" aria-hidden="true">{#if weeks.length > 1}<span>{weekLabel(weeks[0]!.start)}</span>{/if}<span class="now">{tr('ledger.thisWeekSoFar')}</span></div>
          {#if weekMedian !== null && weekMedian > 0}<p class="caption">{tr('ledger.medianNote', { plat: plat(weekMedian) })}</p>{/if}
          <details class="table-view">
            <summary>{tr('ledger.showTable')}</summary>
            <div class="scroll" use:fitTable>
              <table class="tw week-table">
                <thead><tr><th class="l" scope="col">{tr('ledger.col.week')}</th><th class="r" scope="col">{tr('ledger.col.realised')}</th><th class="r" scope="col">{tr('ledger.col.sales')}</th></tr></thead>
                <tbody>
                  {#each weeks as w (w.start)}
                    <tr><td class="l">{weekLabel(w.start)}{w.partial ? tr('ledger.soFar') : ''}</td><td class="mono r">{plat(w.plat)}p</td><td class="mono r">{w.sales}</td></tr>
                  {/each}
                </tbody>
              </table>
            </div>
          </details>
        </div>
      </section>
      <section class="wrap tw" aria-labelledby="ledger-sellers-title">
        <div class="rail"><h3 id="ledger-sellers-title">{tr('ledger.topSellers')}</h3><span class="exp">{tr('ledger.topSellersNote')}</span></div>
        {#if sellers.top.length}
          <ol class="sellers">
            {#each sellers.top as r (r.name)}
              <li>
                <span class="sname">{r.name}</span>
                <span class="sbar" aria-hidden="true"><span style:width="{(r.plat / sellerMax) * 100}%"></span></span>
                <span class="mono sval">{plat(r.plat)}p</span>
                <span class="mono muted sqty">×{r.qty}</span>
              </li>
            {/each}
            {#if sellers.other}
              <li class="other">
                <span class="sname">{tr('ledger.other', { count: sellers.other.items })}</span>
                <span class="sbar" aria-hidden="true"><span style:width="{Math.min(100, (sellers.other.plat / sellerMax) * 100)}%"></span></span>
                <span class="mono sval">{plat(sellers.other.plat)}p</span>
                <span class="sqty"></span>
              </li>
            {/if}
          </ol>
        {:else}
          <p class="dash-body caption">{tr('ledger.noSales')}</p>
        {/if}
      </section>
    </div>
  {/if}

  <section class="wrap tw" aria-labelledby="ledger-history-title">
    <div class="rail"><h3 id="ledger-history-title">{tr('ledger.history')}</h3><span class="exp">{tr('ledger.confirmed', { count: trades.length })}{#if trades.length}{` · ${tr('ledger.soldBought', { sold: all.sales, bought: all.purchases })}`}{/if}</span></div>
  <!-- Open, as meters: it answers "did listing through TennoWorth work?" at a
       glance. Never announced; an empty or disabled record stays one line. -->
  {#if outcomes && !loadError}
    <details class="byitem outcomes" data-testid="listing-outcomes" open={outcomes.enabled && outcomeTable.length > 0}>
      <summary>{tr('ledger.outcomes')} <span class="muted">· {outcomeHeadline(outcomes)}</span></summary>
      {#if outcomes.enabled && outcomeTable.length}
        <p class="muted note">{tr('ledger.outcomesNote', { days: outcomes.window_days })}</p>
        <div class="outcome-groups">
          {#each outcomeTable as row (row.label)}
            <div class="og">
              <h4>{row.label} <span class="muted">· {tr('ledger.listings', { count: row.listings })}</span></h4>
              {#each row.meters as m (m.label)}<Meter label={m.label} value={m.value} max={m.max} text={m.text} />{/each}
              <p class="facts"><span>{tr('ledger.firstSale')} <b class="mono">{row.wait}</b></span><span title={row.priceTitle}>{tr('ledger.priceVsListed')} <b class="mono">{row.price}</b></span></p>
            </div>
          {/each}
        </div>
      {/if}
      <div class="setting-line">
        <label class="check" for="ledger-outcomes"><input id="ledger-outcomes" type="checkbox" checked={outcomes.enabled} onchange={toggleOutcomes} use:busy={savingOutcomes} /> {tr('ledger.keepRecord')}</label>
        <span class="muted">{tr('ledger.keepRecordNote')}</span>
      </div>
    </details>
  {/if}
  {#if loadError}
    <div class="ui-notice" data-tone="bad" role="alert">{tr('ledger.loadFailed', { error: loadError })}</div>
  {:else if trades.length === 0}
    <div class="ui-notice">{tr('ledger.empty')}</div>
  {:else}
    <div class="scroll" use:fitTable bind:this={historyEl}>
      <table class="tw fixed trade-history">
        <colgroup><col style="width:11rem" /><col style="width:6rem" /><col /><col style="width:11rem" /><col style="width:7rem" /><col style="width:8rem" /></colgroup>
        <thead><tr><th>{tr('ledger.col.when')}</th><th>{tr('ledger.col.trade')}</th><th class="l">{tr('ledger.col.items')}</th><th class="l">{tr('ledger.col.with')}</th><th class="r">{tr('ledger.col.plat')}</th><th>{tr('ledger.col.listing')}</th></tr></thead>
        <tbody>
          {#each shown as t (t.id)}
            <tr>
              <td class="mono muted">{when(t.at)}</td>
              <td><span class="kind {t.kind}">{kindLabel(t.kind)}</span></td>
              <td class="l">{describeItems(t)}</td>
              <td class="l muted">{t.partner}</td>
              <td class="mono r" class:good={t.kind === 'sale'} class:bad={t.kind === 'purchase'}>{platCell(t)}</td>
              <td>{#if t.wfm_closed}<span class="muted" title={tr('ledger.listingUpdatedTitle')}>{tr('ledger.listingUpdated')}</span>{/if}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <Pager window={win} total={trades.length} label={tr('ledger.pages')} target={historyEl} onpage={(p) => (page = p)} />
  {/if}

  </section>
  <Toast toasts={toastQueue.toasts} ondismiss={toastQueue.dismiss} />
</section>

<style>
  .setting-line { display: flex; flex-wrap: wrap; align-items: center; gap: var(--s1) var(--s3); padding: var(--s1) 0; border-block: 1px var(--rule) var(--hairline); font-size: var(--text-control); }
  .setting-line .check { display: inline-flex; align-items: flex-start; gap: var(--s2); color: var(--fg); font-weight: 500; }
  .setting-line .muted { color: var(--muted); }
  .ledger { gap: var(--s4); }
  .ui-notice { margin: var(--s4) var(--inset); }
  .byitem { padding: var(--s3) var(--inset); }
  .ui-notice :global(code) { font-family: var(--font-mono); font-size: var(--text-caption); }
  .byitem summary { cursor: pointer; font-size: var(--text-control); }
  /* minmax(0, …): the wide table scrolls in its own region instead of widening
     the column, which clipped the note and the toggle in narrow windows. */
  .outcomes { display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--s2); }
  .outcomes .note { margin: var(--s2) 0 0; font-size: var(--text-control); max-width: 70ch; }
  .outcomes .setting-line { border-block-end: 0; }
  /* Fits the 1200px window; below that it scrolls in its box. */
  table.trade-history { min-width: 56rem; }
  .outcome-groups { margin-top: var(--s2); display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 16rem), 1fr)); gap: var(--s3) var(--s5); }
  .og { display: grid; gap: var(--s2); align-content: start; min-width: 0; }
  .og h4 { margin: 0; font-size: var(--text-control); font-weight: 600; color: var(--fg); }
  .og .facts { display: flex; flex-wrap: wrap; gap: var(--s1) var(--s3); margin: 0; font-size: var(--text-caption); color: var(--muted); }
  .og .facts b { color: var(--fg); font-weight: 600; }
  .dash { display: grid; grid-template-columns: minmax(0, 3fr) minmax(0, 2fr); gap: var(--s4); align-items: start; }
  @media (max-width: 47.5rem) { .dash { grid-template-columns: minmax(0, 1fr); } }
  .dash > section { margin: 0; }
  .dash-body { padding: var(--s3) var(--inset); display: grid; gap: var(--s2); }
  .weeks { position: relative; height: 9rem; display: flex; align-items: stretch; gap: 2px; border-bottom: 1px solid var(--border); }
  .weeks .slot { flex: 1; min-width: 0; display: flex; align-items: end; }
  .weeks .wbar { width: 100%; background: var(--ink-bar); }
  .weeks .wbar.partial { background: var(--muted); opacity: 0.55; }
  /* Same dotted --faint rule as Sparkline's 90-day median. */
  .weeks .median { position: absolute; left: 0; right: 0; height: 0; border-top: 1px dotted var(--faint); pointer-events: none; z-index: 1; }
  .axis .now { margin-left: auto; }
  .axis { display: flex; justify-content: space-between; gap: var(--s2); font: var(--text-caption)/1rem var(--font-mono); color: var(--muted); }
  .caption { margin: 0; font-size: var(--text-caption); color: var(--muted); }
  .table-view summary { cursor: pointer; font-size: var(--text-control); }
  .week-table { margin-top: var(--s2); }
  .sellers { list-style: none; margin: 0; padding: var(--s3) var(--inset); display: grid; gap: 6px; font-size: var(--text-control); }
  .sellers li { display: grid; grid-template-columns: minmax(0, 1fr) minmax(3rem, 6rem) auto 2.5rem; align-items: center; gap: var(--s2); }
  .sellers .sname { min-width: 0; overflow-wrap: anywhere; color: var(--fg); }
  .sellers .sbar { height: 8px; background: var(--panel-2); }
  .sellers .sbar > span { display: block; height: 100%; background: var(--ink-bar); }
  .sellers .other .sname { color: var(--muted); }
  .sellers .other .sbar > span { background: var(--muted); }
  .sellers .sval { text-align: right; color: var(--fg); }
  .sellers .sqty { text-align: right; }
  .r { text-align: right; }
  .mono { font-family: var(--font-mono); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .kind { font-size: var(--text-caption); padding: 1px 7px; border-radius: var(--radius-pill); border: 1px solid var(--border); color: var(--muted); }
  .kind.sale { color: var(--good); border-color: var(--good); }
  .kind.purchase { color: var(--warn); border-color: var(--warn); }
  .good, .v.good { color: var(--good); }
  .bad, .v.bad { color: var(--bad); }
  .muted { color: var(--muted); font-family: var(--font-body); }
  .empty { padding: 10px 0; }
  .scroll { overflow: auto; }
</style>
