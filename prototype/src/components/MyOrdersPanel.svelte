<script lang="ts">
  import { onDestroy } from 'svelte';
  import {
    DesktopCmdError, desktopLiveTopPrices, isDesktopRuntime, LIVE_TOP_PROGRESS_EVENT,
    type LiveTop, type Transport,
  } from '../lib/transport';
  import { listenForTauriEvent } from '../lib/desktop-update';
  import { MAX_PLATINUM, MIN_PLATINUM } from '../lib/limits';
  import { humanError } from '../lib/errors';
  import { selectDrifted, type DriftRow } from '../lib/order-drift';
  import { assessListings, summarize, ownedKey, type HealthIssue } from '../lib/listing-health';
  import { LIQUID_VOL } from '../lib/sell-priority';
  import type { Market } from '../lib/types';
  import Toast from './Toast.svelte';

  // WFM order shape is open — many fields appear depending on the
  // endpoint version (v1 vs v2). We type only what we read.
  interface WfmOrder {
    id: string;
    platinum: number;
    visible: boolean;
    type?: 'sell' | 'buy';
    quantity?: number;
    rank?: number;
    subtype?: string | null;
    item?: { i18n?: { en?: { name?: string } }; en?: { name?: string }; name?: string; slug?: string };
    slug?: string;
    itemId?: string;
  }

  interface ToastMsg {
    id: number;
    kind: 'error' | 'success';
    text: string;
  }

  interface Props {
    transport: Transport;
    /** Price reference for the drift check. Null on a snapshot-less load —
     *  the drift section simply does not render. */
    market?: Market | null;
    /** Bumped by the parent when the WFM session unlocks — re-fetches so a
     *  fetch gated on needs_login/needs_unlock retries automatically. */
    sessionEpoch?: number;
    /** Desktop lock-state rejection → parent raises the auth dialog (which
     *  tries the OS-keyring silent unlock before showing the passphrase). */
    onauthrequired?: (code: 'needs_login' | 'needs_unlock') => void;
    /** Tradeable copies owned per the latest scan, keyed by `ownedKey(slug,
     *  subtype)`. Null when there is no scan — the quantity checks stay off. */
    ownedQty?: Map<string, number> | null;
    /** Live-orders summary for the shell strip / Sell summary cells: fired
     *  once orders are loaded (and whenever the count or the health issues
     *  change); null while nothing is loaded so those cells stay hidden. */
    onsummary?: (s: { live: number; issues: number } | null) => void;
  }
  let { transport, market = null, sessionEpoch = 0, onauthrequired, ownedQty = null, onsummary }: Props = $props();

  type Phase = 'idle' | 'loading' | 'locked' | 'done' | 'error';
  let phase = $state<Phase>('idle');
  let error = $state<string | null>(null);
  let orders = $state<WfmOrder[]>([]);
  let busyIds = $state<Set<string>>(new Set());
  let editingId = $state<string | null>(null);
  let editValue = $state(0);
  // Inline delete confirmation — the destructive click is one tap, the row
  // tints and the button becomes "Confirm"; a second tap (or the ×) resolves.
  let confirmId = $state<string | null>(null);
  let bulkBusy = $state(false);

  // Toasts are component-local. Each toast owns its auto-dismiss timer id so
  // a manual dismiss can cancel it, and onDestroy clears everything pending —
  // this panel is conditionally rendered, so a stray timer would otherwise
  // fire after unmount.
  let toasts = $state<ToastMsg[]>([]);
  let toastSeq = 0;
  const toastTimers = new Map<number, number>();

  function pushToast(text: string, kind: 'error' | 'success' = 'success'): void {
    const id = ++toastSeq;
    toasts = [...toasts, { id, kind, text }];
    toastTimers.set(id, window.setTimeout(() => dismissToast(id), 4500));
  }

  function dismissToast(id: number): void {
    const timer = toastTimers.get(id);
    if (timer !== undefined) {
      window.clearTimeout(timer);
      toastTimers.delete(id);
    }
    toasts = toasts.filter((t) => t.id !== id);
  }

  onDestroy(() => {
    for (const timer of toastTimers.values()) window.clearTimeout(timer);
    toastTimers.clear();
  });

  // Stale-async guard, same shape as App.svelte's verifyGen: only the newest
  // load may commit. Two GET /orders can be in flight at once (e.g. a manual
  // Refresh during a slow load), and without this an older response landing
  // second overwrites the newer one.
  let loadGen = 0;

  function loadOrders(): void {
    const gen = ++loadGen;
    phase = 'loading';
    error = null;
    transport.fetchOrders()
      .then((r) => {
        if (gen !== loadGen) return;
        // WFM v2 returns { data: { sell: [...], buy: [...] } } OR a flat array.
        // Normalize to a flat list with both order types tagged.
        const respObj = r as { data?: unknown } | null | undefined;
        const data = (respObj?.data ?? r) as
          | { sell?: WfmOrder[]; buy?: WfmOrder[] }
          | WfmOrder[]
          | null
          | undefined;
        const out: WfmOrder[] = [];
        const splitData = data as { sell?: WfmOrder[]; buy?: WfmOrder[] } | null | undefined;
        if (Array.isArray(splitData?.sell)) {
          for (const o of splitData.sell) out.push({ ...o, type: 'sell' });
        }
        if (Array.isArray(splitData?.buy)) {
          for (const o of splitData.buy) out.push({ ...o, type: 'buy' });
        }
        // Some endpoints flatten already
        if (out.length === 0 && Array.isArray(data)) {
          for (const o of data) out.push(o);
        }
        orders = out;
        phase = 'done';
      })
      .catch((e: unknown) => {
        if (gen !== loadGen) return;
        // A locked/no-login session is an auth hand-off, not a load error: the
        // parent opens the dialog (silent keyring unlock first), and the
        // sessionEpoch bump re-fires this fetch on success. Kept distinct from
        // 'error' so a cancelled dialog leaves an actionable "unlock required"
        // state instead of a stale failure.
        if (e instanceof DesktopCmdError && (e.code === 'needs_login' || e.code === 'needs_unlock')) {
          phase = 'locked';
          onauthrequired?.(e.code);
          return;
        }
        error = humanError(e);
        phase = 'error';
      });
  }

  // Load on mount and again when the parent bumps sessionEpoch — a fetch that
  // was gated on needs_login/needs_unlock retries the moment the session is
  // unlocked (the transport is a boot-time constant, so nothing else retriggers).
  $effect(() => {
    void sessionEpoch;
    loadOrders();
  });

  function markBusy(id: string, on: boolean): void {
    const next = new Set(busyIds);
    if (on) next.add(id); else next.delete(id);
    busyIds = next;
  }

  // The desktop command relays WFM rejections as a per-order
  // {status:"error", message} body. Treating "no throw" as success applied
  // the edit locally while WFM kept the old value — silent desync.
  function assertOrderOk(r: unknown): void {
    const res = r as { status?: string; message?: string } | null;
    if (res?.status === 'error') throw new Error(res.message || 'WFM rejected the update');
  }

  async function toggleVisible(o: WfmOrder): Promise<void> {
    markBusy(o.id, true);
    try {
      assertOrderOk(await transport.updateOrder(o.id, { visible: !o.visible }));
      o.visible = !o.visible;
      orders = [...orders];
    } catch (e) {
      pushToast(`Couldn't toggle: ${humanError(e)}`, 'error');
    } finally {
      markBusy(o.id, false);
    }
  }

  function startEdit(o: WfmOrder): void {
    editingId = o.id;
    editValue = o.platinum;
  }

  async function saveEdit(o: WfmOrder): Promise<void> {
    const newPrice = Number(editValue);
    if (!newPrice || newPrice < 1) return;
    if (newPrice > MAX_PLATINUM) {
      pushToast(`Price ${newPrice}p is above the ${MAX_PLATINUM}p cap.`, 'error');
      return;
    }
    markBusy(o.id, true);
    try {
      assertOrderOk(await transport.updateOrder(o.id, { platinum: newPrice }));
      o.platinum = newPrice;
      orders = [...orders];
      editingId = null;
    } catch (e) {
      pushToast(`Couldn't update: ${humanError(e)}`, 'error');
    } finally {
      markBusy(o.id, false);
    }
  }

  async function removeOne(o: WfmOrder): Promise<void> {
    if (confirmId !== o.id) {
      confirmId = o.id;
      return;
    }
    confirmId = null;
    markBusy(o.id, true);
    try {
      await transport.deleteOrder(o.id);
      orders = orders.filter((x) => x.id !== o.id);
      pushToast(`Deleted ${itemName(o)}.`);
    } catch (e) {
      pushToast(`Couldn't delete: ${humanError(e)}`, 'error');
    } finally {
      markBusy(o.id, false);
    }
  }

  async function bulkSetVisible(visible: boolean): Promise<void> {
    if (bulkBusy) return;
    const ids = orders.filter((o) => o.visible !== visible).map((o) => o.id);
    if (ids.length === 0) return;
    bulkBusy = true;
    try {
      const resp = await transport.bulkVisibility(ids, visible);
      // Count status==='ok' — the server can skip rows (already in that state,
      // gone since fetch), so ids.length would over-report.
      const ok = (resp?.results ?? []).filter((r) => r.status === 'ok').length;
      for (const o of orders) if (ids.includes(o.id)) o.visible = visible;
      orders = [...orders];
      pushToast(
        visible
          ? `${ok} listing${ok === 1 ? '' : 's'} made visible.`
          : `${ok} listing${ok === 1 ? '' : 's'} made hidden.`,
      );
    } catch (e) {
      pushToast(`Couldn't update visibility: ${humanError(e)}`, 'error');
    } finally {
      bulkBusy = false;
    }
  }

  // The market snapshot keys on slug, so an order that never resolves to one
  // simply cannot be price-checked. Same defensive shape as itemName.
  function itemSlug(o: WfmOrder): string {
    return o.item?.slug || o.slug || '';
  }

  // Orders whose price has drifted from the market. Recomputed whenever the
  // orders list or the snapshot changes — repricing one row drops it out.
  let drifted = $derived.by((): DriftRow[] => {
    if (!market?.items) return [];
    return selectDrifted(
      orders
        .filter((o) => o.type !== 'buy')
        .map((o) => {
          const slug = itemSlug(o);
          return {
            id: o.id,
            slug,
            name: itemName(o),
            platinum: o.platinum,
            type: o.type,
            m: slug ? market.items[slug] : null,
          };
        })
        .filter((r) => r.slug !== ''),
    );
  });

  // Applies the suggestion as a normal price edit — same transport call, same
  // success assertion, so a WFM rejection cannot silently desync the row.
  async function reprice(row: DriftRow): Promise<void> {
    const o = orders.find((x) => x.id === row.id);
    if (!o) return;
    markBusy(row.id, true);
    try {
      assertOrderOk(await transport.updateOrder(row.id, { platinum: row.suggested }));
      o.platinum = row.suggested;
      orders = [...orders];
      pushToast(`${row.name} repriced to ${row.suggested}p.`);
    } catch (e) {
      pushToast(`Couldn't reprice: ${humanError(e)}`, 'error');
    } finally {
      markBusy(row.id, false);
    }
  }

  // ---- Listing health (live top-of-book + owned quantities) ----
  // "Check live" asks WFM for the exact-tier top-of-book of every SELL listing
  // with the user's own order already excluded (wfm-core does that by
  // username), then `assessListings` turns it plus the last scan's owned
  // counts into concrete fixes. Desktop only: the hosted site has no IPC.
  const canLive = isDesktopRuntime();
  type LiveState = 'idle' | 'running' | 'done' | 'error';
  let liveState = $state<LiveState>('idle');
  let liveProgress = $state({ done: 0, total: 0 });
  let liveError = $state<string | null>(null);
  let live = $state<Map<string, LiveTop>>(new Map());
  let liveListenerArmed = false;
  let fixAllBusy = $state(false);

  function liveKey(slug: string, rank: number, subtype: string | null): string {
    return `${slug}|${rank}|${subtype ?? ''}`;
  }
  function liveForOrder(o: WfmOrder): LiveTop | null {
    return live.get(liveKey(itemSlug(o), o.rank ?? 0, o.subtype ?? null)) ?? null;
  }

  async function checkLive(): Promise<void> {
    const targets = orders.filter((o) => o.type !== 'buy' && itemSlug(o) !== '');
    if (targets.length === 0) return;
    if (!liveListenerArmed) {
      liveListenerArmed = true;
      listenForTauriEvent<{ done: number; total: number }>(LIVE_TOP_PROGRESS_EVENT, (p) => {
        liveProgress = p;
      });
    }
    liveState = 'running';
    liveError = null;
    liveProgress = { done: 0, total: targets.length };
    try {
      const res = await desktopLiveTopPrices(
        targets.map((o) => ({ slug: itemSlug(o), rank: o.rank ?? 0, subtype: o.subtype ?? null })),
      );
      const next = new Map(live);
      for (const t of res) next.set(liveKey(t.slug, t.rank ?? 0, t.subtype ?? null), t);
      live = next;
      liveState = 'done';
    } catch (e) {
      liveState = 'error';
      liveError = e instanceof DesktopCmdError ? e.message : humanError(e);
    }
  }

  let health = $derived.by((): HealthIssue[] => {
    if (live.size === 0 && !ownedQty) return [];
    return assessListings(
      orders
        .filter((o) => o.type !== 'buy')
        .map((o) => {
          const slug = itemSlug(o);
          return {
            id: o.id, slug, name: itemName(o),
            platinum: o.platinum, quantity: o.quantity ?? 1, type: 'sell' as const,
            live: slug ? liveForOrder(o) : null,
            owned: ownedQty && slug ? (ownedQty.get(ownedKey(slug, o.subtype ?? null)) ?? 0) : null,
          };
        })
        .filter((r) => r.slug !== ''),
    );
  });
  let healthSummary = $derived(summarize(health));
  $effect(() => {
    onsummary?.(phase === 'done' ? { live: orders.length, issues: health.length } : null);
  });

  async function applyFix(issue: HealthIssue): Promise<void> {
    const o = orders.find((x) => x.id === issue.id);
    if (!o) return;
    markBusy(issue.id, true);
    try {
      if (issue.kind === 'overpriced' || issue.kind === 'underbid') {
        const p = Math.min(MAX_PLATINUM, Math.max(MIN_PLATINUM, issue.suggested));
        assertOrderOk(await transport.updateOrder(issue.id, { platinum: p }));
        o.platinum = p;
        pushToast(`${issue.name} repriced to ${p}p.`);
      } else if (issue.kind === 'excess-qty') {
        assertOrderOk(await transport.updateOrder(issue.id, { quantity: issue.suggested }));
        o.quantity = issue.suggested;
        pushToast(`${issue.name} quantity set to ${issue.suggested}.`);
      } else if (issue.kind === 'not-owned') {
        await transport.deleteOrder(issue.id);
        orders = orders.filter((x) => x.id !== issue.id);
        pushToast(`Deleted ${issue.name}.`);
        return;
      }
      orders = [...orders];
    } catch (e) {
      pushToast(`Couldn't fix ${issue.name}: ${humanError(e)}`, 'error');
    } finally {
      markBusy(issue.id, false);
    }
  }

  /** Apply every PRICE fix (match lowest ask / meet the bid). Quantity and
   *  delete fixes stay one-click-each — those change what's for sale. */
  async function fixAllPrices(): Promise<void> {
    if (fixAllBusy) return;
    fixAllBusy = true;
    try {
      for (const issue of health.filter((i) => i.kind === 'overpriced' || i.kind === 'underbid')) {
        await applyFix(issue);
      }
    } finally {
      fixAllBusy = false;
    }
  }

  // WFM order objects nest the item info — name lookup is defensive.
  function itemName(o: WfmOrder): string {
    return (
      o.item?.i18n?.en?.name ||
      o.item?.en?.name ||
      o.item?.name ||
      o.item?.slug ||
      o.slug ||
      o.itemId ||
      'unknown'
    );
  }
</script>

<section class="card orders">
  <header class="row">
    <h2>My WFM listings</h2>
    <div class="row gap-sm">
      <span class="muted">
        {#if phase === 'loading' || phase === 'idle'}Fetching orders…
        {:else if phase === 'done'}{orders.length} active
        {:else if phase === 'locked'}unlock required
        {/if}
      </span>
      <button
        class="tiny ghost"
        onclick={() => bulkSetVisible(true)}
        disabled={bulkBusy || orders.every((o) => o.visible)}
        title="Make every listing visible to buyers"
      >All visible</button>
      <button
        class="tiny ghost"
        onclick={() => bulkSetVisible(false)}
        disabled={bulkBusy || orders.every((o) => !o.visible)}
        title="Hide every listing from buyers"
      >All hidden</button>
      {#if canLive}
        <button
          class="ghost"
          onclick={checkLive}
          disabled={liveState === 'running' || phase !== 'done' || orders.length === 0}
          title="Ask warframe.market for the best online asks and bids on each of your sell listings' exact rank / refinement — your own order excluded — and flag what's worth fixing."
        >
          {#if liveState === 'running'}Checking… {liveProgress.done}/{liveProgress.total}
          {:else if liveState === 'done'}Re-check live
          {:else}Check live{/if}
        </button>
      {/if}
      <button class="ghost" onclick={loadOrders} disabled={phase === 'loading'}>Refresh</button>
    </div>
  </header>

  {#if liveState === 'error' && liveError}
    <div class="muted bad">Live check failed: {liveError}</div>
  {/if}

  {#if health.length > 0}
    <div class="health">
      <p class="health-lead">
        <strong>Listing health:</strong>
        {#if healthSummary.overpriced}<span class="chip warn">{healthSummary.overpriced} above the market</span>{/if}
        {#if healthSummary.underbid}<span class="chip warn">{healthSummary.underbid} under a live bid</span>{/if}
        {#if healthSummary.excessQty}<span class="chip">{healthSummary.excessQty} over-quantity</span>{/if}
        {#if healthSummary.notOwned}<span class="chip bad">{healthSummary.notOwned} not owned</span>{/if}
        {#if healthSummary.overpriced + healthSummary.underbid > 1}
          <button class="tiny" onclick={fixAllPrices} disabled={fixAllBusy} title="Reprice every flagged listing: match the lowest other ask, or meet the higher bid. Quantity fixes and deletions stay one click each.">Fix all prices</button>
        {/if}
      </p>
      <ul class="drift-list">
        {#each health as h (h.id + h.kind)}
          <li class="drift-row">
            <span class="drift-name" title={h.slug}>{h.name}</span>
            <span class="drift-move" title={h.why}>
              {#if h.kind === 'overpriced' || h.kind === 'underbid'}
                {h.current}p → <strong>{h.suggested}p</strong>
                <span class="drift-kind {h.kind === 'overpriced' ? 'overpriced' : 'underpriced'}">{h.kind === 'overpriced' ? 'above lowest ask' : 'under a bid'}</span>
              {:else if h.kind === 'excess-qty'}
                ×{h.current} → <strong>×{h.suggested}</strong>
                <span class="drift-kind">more listed than owned</span>
              {:else}
                ×{h.current}
                <span class="drift-kind overpriced">not in your inventory</span>
              {/if}
            </span>
            <button
              class="tiny"
              onclick={() => applyFix(h)}
              disabled={busyIds.has(h.id)}
              title={h.why}
            >{h.kind === 'not-owned' ? 'Delete' : h.kind === 'excess-qty' ? 'Set qty' : 'Reprice'}</button>
          </li>
        {/each}
      </ul>
    </div>
  {/if}

  <!-- Snapshot-based drift is the offline fallback; once a live check has run,
       Listing Health above covers the same orders with exact figures, so the
       two never show together. -->
  {#if drifted.length > 0 && live.size === 0}
    <div class="drift">
      <p class="drift-lead">
        The market moved under {drifted.length}
        {drifted.length === 1 ? 'listing' : 'listings'}. Prices come from the
        last snapshot, so treat them as a starting point, not a quote{#if canLive} — or <button class="linkish" onclick={checkLive} disabled={liveState === 'running'}>check live</button> for exact figures{/if}.
      </p>
      <ul class="drift-list">
        {#each drifted as d (d.id)}
          <li class="drift-row">
            <span class="drift-name" title={d.slug}>{d.name}</span>
            <span class="drift-move">
              {d.listed}p → <strong>{d.suggested}p</strong>
              <span class="drift-kind {d.kind}">
                {d.kind === 'overpriced' ? 'above market' : 'under market'}
                ({Math.round(d.delta_pct * 100)}%)
              </span>
              {#if d.thin}
                <span class="drift-thin" title="Below the {LIQUID_VOL}-trade/48h liquidity floor — thin books make this a weak signal.">thin</span>
              {/if}
            </span>
            <button
              class="tiny"
              onclick={() => reprice(d)}
              disabled={busyIds.has(d.id)}
              title="Update this listing to {d.suggested}p on warframe.market"
            >Reprice</button>
          </li>
        {/each}
      </ul>
    </div>
  {/if}

  {#if phase === 'error'}
    <div class="muted bad">Couldn't load orders: {error}</div>
  {:else if phase === 'locked'}
    <div class="muted">Unlock warframe.market to see your orders.</div>
  {:else if phase === 'done' && orders.length === 0}
    <div class="muted">No active listings.</div>
  {:else if orders.length > 0}
    <div class="scroll">
      <table>
        <thead>
          <tr>
            <th>Item</th>
            <th>Type</th>
            <th>Qty</th>
            <th>Price</th>
            <th>Visible</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each orders as o (o.id)}
            {@const busy = busyIds.has(o.id)}
            <tr class:dim={busy} class:confirming={confirmId === o.id}>
              <td>{itemName(o)}</td>
              <td class:sell={o.type === 'sell'} class:buy={o.type === 'buy'}>
                {o.type ?? '?'}
              </td>
              <td class="right">{o.quantity ?? '?'}</td>
              <td class="right">
                {#if editingId === o.id}
                  <input type="number" bind:value={editValue} min="1" max={MAX_PLATINUM} style="width:64px" />
                  <button class="tiny" onclick={() => saveEdit(o)} disabled={busy}>save</button>
                  <button class="tiny ghost" onclick={() => (editingId = null)}>×</button>
                {:else}
                  {o.platinum}p
                  <button class="tiny ghost" onclick={() => startEdit(o)} disabled={busy} title="Edit price">✎</button>
                {/if}
              </td>
              <td>
                <button
                  class="vis {o.visible ? 'on' : 'off'}"
                  onclick={() => toggleVisible(o)}
                  disabled={busy}
                  title={o.visible ? 'Click to make hidden' : 'Click to make visible'}
                >{o.visible ? 'on' : 'off'}</button>
              </td>
              <td>
                {#if confirmId === o.id}
                  <button class="confirm tiny bad" onclick={() => removeOne(o)} disabled={busy} title="Confirm delete">Confirm</button>
                  <button class="tiny ghost" onclick={() => (confirmId = null)} title="Cancel">×</button>
                {:else}
                  <button class="tiny bad" onclick={() => removeOne(o)} disabled={busy} title="Delete">✕</button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>

<Toast {toasts} ondismiss={dismissToast} />

<style>
  .orders { gap: 10px; }
  .orders h2 {
    margin: 0;
    font-size: 13px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--muted);
    font-weight: 600;
  }
  .scroll {
    overflow: auto;
    max-height: 360px;
    border: 1px solid var(--border);
    border-radius: var(--radius-ctl);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-variant-numeric: tabular-nums;
  }
  th, td {
    padding: 6px 10px;
    text-align: left;
    border-bottom: 1px solid var(--border);
    font-size: 12.5px;
  }
  th {
    background: var(--panel-2);
    color: var(--muted);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-size: 11px;
    position: sticky;
    top: 0;
  }
  td.right { text-align: right; }
  td.sell { color: var(--good); font-weight: 500; }
  td.buy { color: var(--accent); font-weight: 500; }
  tr.dim { opacity: 0.5; }
  /* Pending destructive row — the two-tap delete's armed state. The 6% tint is
     faint by design; the 3px left border is the primary signal. */
  tr.confirming {
    background: color-mix(in srgb, var(--bad) 6%, transparent);
    border-left: 3px solid var(--bad);
  }
  .confirm { color: var(--muted); font-size: 12px; white-space: nowrap; }
  .vis {
    appearance: none;
    border: 1px solid var(--border);
    background: var(--panel-2);
    color: var(--muted);
    font-size: 11px;
    padding: 2px 8px;
    border-radius: var(--radius-input);
    cursor: pointer;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .vis.on { color: var(--good); border-color: color-mix(in srgb, var(--good) 60%, var(--border)); }
  .vis.off { color: var(--muted); }
  .vis:hover:not(:disabled) { background: var(--panel); }
  .tiny {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--muted);
    font-size: 11px;
    padding: 1px 6px;
    border-radius: var(--radius-input);
    cursor: pointer;
  }
  .tiny:hover:not(:disabled) { color: var(--fg); }
  .tiny.bad { color: var(--bad); border-color: color-mix(in srgb, var(--bad) 40%, var(--border)); }
  .tiny.bad:hover:not(:disabled) { background: color-mix(in srgb, var(--bad) 12%, transparent); }
  input[type="number"] {
    font: inherit;
    font-family: var(--font-mono);
    font-size: 12px;
    background: var(--panel-2);
    border: 1px solid var(--border);
    color: var(--fg);
    border-radius: var(--radius-input);
    padding: 2px 4px;
  }
  .muted.bad { color: var(--bad); }
  .drift {
    border: 1px solid color-mix(in srgb, var(--warn) 35%, var(--border));
    background: color-mix(in srgb, var(--warn) 6%, transparent);
    border-radius: var(--radius-ctl);
    padding: 10px 12px;
    margin-bottom: 12px;
  }
  .drift-lead { margin: 0 0 8px; font-size: 12px; color: var(--muted); }
  button.linkish { background: none; border: 0; padding: 0; color: var(--fg); text-decoration: underline dotted; cursor: pointer; font: inherit; }
  .health {
    margin: 10px 0 0;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-panel);
    background: var(--panel-2);
  }
  .health-lead { margin: 0 0 8px; font-size: 12px; color: var(--muted); display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
  .health-lead strong { color: var(--fg); }
  .chip { border: 1px solid var(--border); border-radius: var(--radius-pill); padding: 1px 8px; font-size: 11px; color: var(--muted); }
  .chip.warn { color: var(--warn); border-color: var(--warn); }
  .chip.bad { color: var(--bad); border-color: var(--bad); }
  .drift-list { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; }
  .drift-row { display: flex; align-items: center; gap: 10px; font-size: 12px; }
  .drift-name {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .drift-move { flex: 0 0 auto; font-family: var(--font-mono); }
  .drift-kind { color: var(--muted); font-family: inherit; }
  .drift-kind.overpriced { color: var(--warn); }
  .drift-thin {
    padding: 1px 5px;
    font-size: 9.5px;
    font-weight: 600;
    letter-spacing: .1em;
    text-transform: uppercase;
    border: 1px solid currentColor;
    border-radius: var(--radius-tag);
    color: var(--warn);
    font-family: inherit;
  }
</style>
