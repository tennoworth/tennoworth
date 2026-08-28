<script lang="ts">
  import { onMount } from 'svelte';
  import {
    DesktopCmdError, desktopEelogStatus, desktopListTrades, TRADE_DETECTED_EVENT,
    type EeLogStatus, type TradeDetected, type TradeRow,
  } from '../lib/transport';
  import { listenForTauriEvent } from '../lib/desktop-update';
  import { totals, since, soldByItem, describeItems } from '../lib/ledger';
  import { humanError } from '../lib/errors';
  import Toast from './Toast.svelte';

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

  interface ToastMsg { id: number; kind: 'error' | 'success'; text: string }
  let toasts = $state<ToastMsg[]>([]);
  let toastSeq = 0;
  function pushToast(text: string, kind: 'error' | 'success' = 'success'): void {
    const id = ++toastSeq;
    toasts = [...toasts, { id, kind, text }];
    window.setTimeout(() => (toasts = toasts.filter((t) => t.id !== id)), 5000);
  }

  async function load(): Promise<void> {
    try {
      const [rows, st] = await Promise.all([desktopListTrades(500), desktopEelogStatus()]);
      trades = rows;
      status = st;
      autoClose = st.auto_close;
      loadError = null;
    } catch (e) {
      loadError = e instanceof DesktopCmdError ? e.message : humanError(e);
    }
  }

  onMount(() => {
    void load();
    return listenForTauriEvent<TradeDetected>(TRADE_DETECTED_EVENT, (d) => {
      const kind = d.trade.kind;
      const head = kind === 'sale' ? `Sold for ${d.trade.plat}p` : kind === 'purchase' ? `Bought for ${d.trade.plat}p` : 'Trade completed';
      const adj = d.adjusted.length ? ` · ${d.adjusted.length} listing${d.adjusted.length === 1 ? '' : 's'} updated` : '';
      pushToast(`${head}: ${describeItems(d.trade)} — ${d.trade.partner}${adj}`);
      void load();
    });
  });

  async function toggleAutoClose(): Promise<void> {
    if (!onsetautoclose || savingAutoClose) return;
    savingAutoClose = true;
    const next = !autoClose;
    try {
      await onsetautoclose(next);
      autoClose = next;
    } catch (e) {
      pushToast(`Couldn't save: ${humanError(e)}`, 'error');
    } finally {
      savingAutoClose = false;
    }
  }

  const nowSecs = () => Math.floor(Date.now() / 1000);
  let all = $derived(totals(trades));
  let week = $derived(totals(since(trades, 7, nowSecs())));
  let byItem = $derived(soldByItem(trades).slice(0, 10));

  function when(unixSecs: number): string {
    const d = new Date(unixSecs * 1000);
    return d.toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
  }
  function kindLabel(k: TradeRow['kind']): string {
    return k === 'sale' ? 'Sold' : k === 'purchase' ? 'Bought' : 'Traded';
  }
  function platCell(t: TradeRow): string {
    if (t.kind === 'sale') return `+${t.plat}p`;
    if (t.kind === 'purchase') return `−${t.plat}p`;
    return '—';
  }
</script>

<section class="card ledger" data-testid="ledger">
  <header class="row">
    <h2>Ledger</h2>
    <div class="row gap-sm">
      <span class="muted">{trades.length} trade{trades.length === 1 ? '' : 's'}</span>
      <button class="ghost" onclick={load}>Refresh</button>
    </div>
  </header>

  {#if status}
    {#if status.path}
      <p class="muted lead">
        Reading trades from the game's own log (<code title={status.path}>EE.log</code>). Every completed trade lands here with what changed hands, and sales can adjust the matching warframe.market listing.
      </p>
    {:else}
      <p class="muted lead warn-line">
        <strong>Game log not found</strong> — trade detection is off. TennoWorth looks in <code>%LOCALAPPDATA%\Warframe\EE.log</code> (Windows) and every Steam library's <code>compatdata/230410/…/Warframe/EE.log</code> (Linux). Run Warframe once, then restart TennoWorth; for an unusual install set <code>TENNOWORTH_EELOG=/path/to/EE.log</code> before launching.
      </p>
    {/if}
    <label class="toggle">
      <input type="checkbox" checked={autoClose} onchange={toggleAutoClose} disabled={savingAutoClose || !status.path} />
      <span>After a sale, reduce or remove the matching warframe.market listing automatically</span>
      <span class="muted hint">only ever lowers a quantity by what you sold; never touches price or visibility</span>
    </label>
  {/if}

  {#if loadError}
    <div class="muted bad">Couldn't load the ledger: {loadError}</div>
  {:else if trades.length === 0}
    <div class="muted empty">No trades recorded yet. Complete a trade in-game with TennoWorth running and it appears here.</div>
  {:else}
    <div class="tiles">
      <div class="tile"><span class="k">Net, all time</span><strong class:good={all.net > 0} class:bad={all.net < 0}>{all.net > 0 ? '+' : ''}{all.net}p</strong><span class="muted">{all.sales} sold · {all.purchases} bought</span></div>
      <div class="tile"><span class="k">Last 7 days</span><strong class:good={week.net > 0} class:bad={week.net < 0}>{week.net > 0 ? '+' : ''}{week.net}p</strong><span class="muted">{week.sales} sold · {week.purchases} bought</span></div>
      <div class="tile"><span class="k">Plat in / out</span><strong>{all.platIn}p / {all.platOut}p</strong><span class="muted">sales / purchases</span></div>
    </div>

    {#if byItem.length}
      <details class="byitem">
        <summary>Top sellers by realised plat <span class="muted">· multi-item sales split by quantity</span></summary>
        <ul>
          {#each byItem as r (r.name)}
            <li><span class="name">{r.name}</span><span class="mono">×{r.qty} · {r.plat}p · {r.trades} sale{r.trades === 1 ? '' : 's'}</span></li>
          {/each}
        </ul>
      </details>
    {/if}

    <div class="scroll">
      <table>
        <thead><tr><th>When</th><th></th><th>Items</th><th>With</th><th class="r">Plat</th><th></th></tr></thead>
        <tbody>
          {#each trades as t (t.id)}
            <tr>
              <td class="mono muted">{when(t.at)}</td>
              <td><span class="kind {t.kind}">{kindLabel(t.kind)}</span></td>
              <td>{describeItems(t)}</td>
              <td class="muted">{t.partner}</td>
              <td class="mono r" class:good={t.kind === 'sale'} class:bad={t.kind === 'purchase'}>{platCell(t)}</td>
              <td>{#if t.wfm_closed}<span class="muted" title="A warframe.market listing was reduced or removed after this sale">listing updated</span>{/if}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}

  <Toast {toasts} ondismiss={(id) => (toasts = toasts.filter((x) => x.id !== id))} />
</section>

<style>
  .ledger { display: flex; flex-direction: column; gap: 10px; }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  .gap-sm { gap: 8px; }
  h2 { margin: 0; font-size: 15px; }
  .lead { margin: 0; font-size: 12.5px; }
  .warn-line { border-left: 3px solid var(--warn); padding-left: 8px; }
  code { font-family: var(--font-mono); font-size: 11.5px; }
  .toggle { display: flex; align-items: center; gap: 8px; font-size: 12.5px; flex-wrap: wrap; }
  .toggle .hint { font-size: 11.5px; }
  .tiles { display: grid; grid-template-columns: repeat(auto-fit, minmax(160px, 1fr)); gap: 10px; }
  .tile { display: flex; flex-direction: column; gap: 2px; padding: 10px 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--panel-2); }
  .tile .k { font-size: 11px; letter-spacing: .04em; text-transform: uppercase; color: var(--muted); }
  .tile strong { font-size: 18px; font-variant-numeric: tabular-nums; }
  .byitem summary { cursor: pointer; font-size: 12.5px; }
  .byitem ul { list-style: none; margin: 6px 0 0; padding: 0; display: grid; gap: 4px; font-size: 12.5px; }
  .byitem li { display: flex; justify-content: space-between; gap: 10px; }
  table { width: 100%; border-collapse: collapse; font-size: 13px; }
  th { text-align: left; font-weight: 600; font-size: 11px; letter-spacing: .04em; text-transform: uppercase; color: var(--muted); padding: 6px 8px; border-bottom: 1px solid var(--border); }
  td { padding: 6px 8px; border-bottom: 1px solid var(--border); vertical-align: middle; }
  .r { text-align: right; }
  .mono { font-family: var(--font-mono); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .kind { font-size: 11px; padding: 1px 7px; border-radius: var(--radius-pill); border: 1px solid var(--border); color: var(--muted); }
  .kind.sale { color: var(--good); border-color: var(--good); }
  .kind.purchase { color: var(--warn); border-color: var(--warn); }
  .good { color: var(--good); }
  .bad { color: var(--bad); }
  .muted { color: var(--muted); }
  .empty { padding: 10px 0; }
  .scroll { overflow: auto; }
  button.ghost { background: transparent; color: var(--muted); border: 1px solid var(--border); padding: 4px 10px; border-radius: var(--radius-ctl); font-size: 12px; cursor: pointer; }
  button.ghost:hover:not(:disabled) { background: var(--panel-2); color: var(--fg); }
</style>
