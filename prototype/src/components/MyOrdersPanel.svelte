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

  // ---- Show / Narrow controls on the orders table's bar ----
  type Show = 'all' | 'sell' | 'buy' | 'hidden' | 'issues';
  let show = $state<Show>('all');
  let nameFilter = $state('');
  // Orders with something in the fix queue (a live/scan health issue, or the
  // snapshot-drift fallback), so the "Issues" segment narrows to them.
  let issueIds = $derived.by(() => {
    const ids = new Set<string>();
    for (const h of health) ids.add(h.id);
    if (live.size === 0) for (const d of drifted) ids.add(d.id);
    return ids;
  });
  let counts = $derived({
    all: orders.length,
    sell: orders.filter((o) => o.type !== 'buy').length,
    buy: orders.filter((o) => o.type === 'buy').length,
    hidden: orders.filter((o) => !o.visible).length,
    issues: issueIds.size,
  });
  let shown = $derived.by(() => {
    const f = nameFilter.trim().toLowerCase();
    return orders.filter((o) => {
      if (show === 'sell' && o.type === 'buy') return false;
      if (show === 'buy' && o.type !== 'buy') return false;
      if (show === 'hidden' && o.visible) return false;
      if (show === 'issues' && !issueIds.has(o.id)) return false;
      return !f || itemName(o).toLowerCase().includes(f);
    });
  });
  let listedValue = $derived(orders.filter((o) => o.type !== 'buy').reduce((a, o) => a + o.platinum * (o.quantity ?? 1), 0));

  // The fix queue: live/scan health issues first, then (only while no live
  // check has run) the snapshot-drift fallback for the remaining sell orders.
  // Once a live check has run, Listing health covers the same orders with
  // exact figures, so the two never show together.
  type QueueRow =
    | { key: string; id: string; name: string; slug: string; kind: 'health'; h: HealthIssue }
    | { key: string; id: string; name: string; slug: string; kind: 'drift'; d: DriftRow };
  let queue = $derived.by((): QueueRow[] => {
    const rows: QueueRow[] = health.map((h) => ({ key: `h:${h.id}:${h.kind}`, id: h.id, name: h.name, slug: h.slug, kind: 'health', h }));
    if (live.size === 0) {
      for (const d of drifted) rows.push({ key: `d:${d.id}`, id: d.id, name: d.name, slug: d.slug, kind: 'drift', d });
    }
    return rows;
  });
  function orderById(id: string): WfmOrder | undefined {
    return orders.find((o) => o.id === id);
  }
  function healthAction(h: HealthIssue): string {
    return h.kind === 'not-owned' ? 'Delete' : h.kind === 'excess-qty' ? 'Set qty' : 'Reprice';
  }
  function driftWhy(d: DriftRow): string {
    const pct = Math.round(d.delta_pct * 100);
    return d.kind === 'overpriced'
      ? `${pct}% above the last snapshot's clearing price — a starting point, not a quote${d.thin ? ' (thin book)' : ''}.`
      : `${pct}% under the last snapshot's clearing price — you may be leaving plat on the table${d.thin ? ' (thin book)' : ''}.`;
  }
</script>

<!-- Slot 2: Listing health as a fix queue — one decision, one button per row.
     Slot 3/4: controls row inside the orders table's border, then the table. -->
<section class="wrap tw health" aria-label="Listing health">
  <div class="bar">
    <h3>Listing health</h3>
    <span class="exp">
      {#if phase === 'loading' || phase === 'idle'}Fetching orders…
      {:else if phase === 'locked'}unlock required
      {:else if phase === 'error'}couldn't load orders
      {:else if queue.length > 0}{queue.length} of {orders.length} {orders.length === 1 ? 'listing needs' : 'listings need'} attention · fixes apply immediately
      {:else if live.size > 0}no issues in {orders.length} {orders.length === 1 ? 'listing' : 'listings'} · checked against the live top-of-book
      {:else if ownedQty}no issues in {orders.length} {orders.length === 1 ? 'listing' : 'listings'} · quantities checked against your last scan
      {:else}nothing flagged yet{#if canLive} — check live to compare your asks with the online top-of-book{/if}
      {/if}
    </span>
    {#if health.length > 0}
      <span class="chips">
        {#if healthSummary.overpriced}<span class="chip warn">{healthSummary.overpriced} above the market</span>{/if}
        {#if healthSummary.underbid}<span class="chip warn">{healthSummary.underbid} under a live bid</span>{/if}
        {#if healthSummary.excessQty}<span class="chip">{healthSummary.excessQty} over-quantity</span>{/if}
        {#if healthSummary.notOwned}<span class="chip bad">{healthSummary.notOwned} not owned</span>{/if}
      </span>
    {/if}
    <span class="grow"></span>
    {#if canLive}
      <button
        class="btn"
        onclick={checkLive}
        disabled={liveState === 'running' || phase !== 'done' || orders.length === 0}
        title="Ask warframe.market for the best online asks and bids on each of your sell listings' exact rank / refinement — your own order excluded — and flag what's worth fixing."
      >
        {#if liveState === 'running'}Checking… {liveProgress.done}/{liveProgress.total}
        {:else if liveState === 'done'}Re-check live
        {:else}Check live{/if}
      </button>
    {/if}
    {#if healthSummary.overpriced + healthSummary.underbid > 1}
      <button class="btn primary" onclick={fixAllPrices} disabled={fixAllBusy} title="Reprice every flagged listing: match the lowest other ask, or meet the higher bid. Quantity fixes and deletions stay one click each.">Fix all prices</button>
    {/if}
  </div>

  {#if liveState === 'error' && liveError}
    <div class="line bad">Live check failed: {liveError}</div>
  {/if}

  {#if queue.length > 0}
    <div class="scroll">
    <table class="tw fixed queue">
      <colgroup>
        <col style="width:16rem" />
        <col style="width:3rem" />
        <col style="width:4rem" />
        <col style="width:4.5rem" />
        <col style="width:4.5rem" />
        <col />
        <col style="width:6rem" />
      </colgroup>
      <thead><tr>
        <th class="l">Item</th>
        <th title="Listed quantity">Qty</th>
        <th title="Your ask">Listed</th>
        <th title="Lowest other online ask for this exact tier (live check)">Live ask</th>
        <th title="Highest online bid for this exact tier (live check)">Live bid</th>
        <th class="l">Why</th>
        <th></th>
      </tr></thead>
      <tbody>
        {#each queue as q (q.key)}
          {@const o = orderById(q.id)}
          {@const t = o ? liveForOrder(o) : null}
          {@const busy = busyIds.has(q.id)}
          <tr class:busy>
            <td class="l" title={q.slug}>{q.name}</td>
            <td>{o?.quantity ?? '?'}</td>
            <td class="fg">{o?.platinum ?? '?'}<span class="unit">p</span></td>
            <td>{#if t && !t.error && t.low_sell != null}{t.low_sell}<span class="unit">p</span>{:else}<span class="faint">—</span>{/if}</td>
            <td>{#if t && !t.error && t.top_buy != null}{t.top_buy}<span class="unit">p</span>{:else}<span class="faint">—</span>{/if}</td>
            {#if q.kind === 'health'}
              <td class="reason" class:warn={q.h.kind === 'overpriced' || q.h.kind === 'underbid'} class:bad={q.h.kind === 'not-owned'} title={q.h.why}>
                {#if q.h.kind === 'overpriced' || q.h.kind === 'underbid'}
                  <span class="to">{q.h.current}p → <b>{q.h.suggested}p</b></span>
                  {q.h.kind === 'overpriced' ? 'above the lowest other ask' : 'under a live bid'}
                {:else if q.h.kind === 'excess-qty'}
                  <span class="to">×{q.h.current} → <b>×{q.h.suggested}</b></span>
                  more listed than owned
                {:else}
                  <span class="to">×{q.h.current}</span>
                  not in your inventory
                {/if}
              </td>
              <td class="act">
                <button class="btn xs" class:bad={q.h.kind === 'not-owned'} onclick={() => applyFix(q.h)} disabled={busy} title={q.h.why}>{healthAction(q.h)}</button>
              </td>
            {:else}
              <td class="reason" class:warn={q.d.kind === 'overpriced'} title={driftWhy(q.d)}>
                <span class="to">{q.d.listed}p → <b>{q.d.suggested}p</b></span>
                {q.d.kind === 'overpriced' ? 'above market' : 'under market'} ({Math.round(q.d.delta_pct * 100)}%, snapshot)
                {#if q.d.thin}<span class="tag thin" title="Below the {LIQUID_VOL}-trade/48h liquidity floor — thin books make this a weak signal.">thin</span>{/if}
              </td>
              <td class="act">
                <button class="btn xs" onclick={() => reprice(q.d)} disabled={busy} title="Update this listing to {q.d.suggested}p on warframe.market">Reprice</button>
              </td>
            {/if}
          </tr>
        {/each}
      </tbody>
    </table>
    </div>
    {#if live.size === 0 && drifted.length > 0}
      <div class="line">
        <span class="exp">Snapshot rows compare against the last market snapshot (up to 2 h old) and can't tell whose order is whose{#if canLive}&nbsp;— <button class="linkish" onclick={checkLive} disabled={liveState === 'running'}>check live</button> for exact figures{/if}.</span>
      </div>
    {/if}
  {/if}
</section>

<section class="wrap tw orders" aria-label="My WFM listings">
  <div class="bar">
    <span class="lbl">Show</span>
    <span class="seg" role="group" aria-label="Show">
      <button class:on={show === 'all'} aria-pressed={show === 'all'} onclick={() => (show = 'all')}>All {counts.all}</button>
      <button class:on={show === 'sell'} aria-pressed={show === 'sell'} onclick={() => (show = 'sell')}>Sell {counts.sell}</button>
      <button class:on={show === 'buy'} aria-pressed={show === 'buy'} onclick={() => (show = 'buy')}>Buy {counts.buy}</button>
      <button class:on={show === 'hidden'} aria-pressed={show === 'hidden'} onclick={() => (show = 'hidden')}>Hidden {counts.hidden}</button>
      <button class:on={show === 'issues'} aria-pressed={show === 'issues'} onclick={() => (show = 'issues')}>Issues {counts.issues}</button>
    </span>
    <input class="input" type="text" placeholder="Filter by name…" bind:value={nameFilter} aria-label="Filter orders by name" />
    <span class="grow"></span>
    <span class="count">
      {#if phase === 'loading' || phase === 'idle'}Fetching orders…
      {:else if phase === 'done'}<b>{shown.length === orders.length ? orders.length : `${shown.length} of ${orders.length}`}</b> {orders.length === 1 ? 'order' : 'orders'}{#if listedValue > 0}&nbsp;· <b>{listedValue.toLocaleString()}</b>p listed{/if}
      {:else if phase === 'locked'}unlock required
      {/if}
    </span>
    <button
      class="btn ghost"
      onclick={() => bulkSetVisible(true)}
      disabled={bulkBusy || orders.every((o) => o.visible)}
      title="Make every listing visible to buyers"
    >All visible</button>
    <button
      class="btn ghost"
      onclick={() => bulkSetVisible(false)}
      disabled={bulkBusy || orders.every((o) => !o.visible)}
      title="Hide every listing from buyers"
    >All hidden</button>
    <button class="btn" onclick={loadOrders} disabled={phase === 'loading'}>Refresh</button>
  </div>

  {#if phase === 'error'}
    <div class="line bad">Couldn't load orders: {error}</div>
  {:else if phase === 'locked'}
    <div class="line"><span class="exp">Unlock warframe.market to see your orders.</span></div>
  {:else if phase === 'done' && orders.length === 0}
    <div class="line"><span class="exp">No active listings.</span></div>
  {:else if orders.length > 0}
    <div class="scroll">
      <table class="tw fixed">
        <colgroup>
          <col />
          <col style="width:4rem" />
          <col style="width:3rem" />
          <col style="width:10rem" />
          {#if live.size > 0}<col style="width:4.5rem" /><col style="width:4.5rem" />{/if}
          <col style="width:4.5rem" />
          <col style="width:6.5rem" />
        </colgroup>
        <thead>
          <tr>
            <th class="l">Item</th>
            <th>Type</th>
            <th>Qty</th>
            <th>Price</th>
            {#if live.size > 0}
              <th title="Lowest other online ask for this exact tier">Live ask</th>
              <th title="Highest online bid for this exact tier">Live bid</th>
            {/if}
            <th>Visible</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each shown as o (o.id)}
            {@const busy = busyIds.has(o.id)}
            {@const t = live.size > 0 ? liveForOrder(o) : null}
            <tr class:busy class:confirming={confirmId === o.id}>
              <td class="l">{itemName(o)}</td>
              <td><span class="type" class:buy={o.type === 'buy'}>{o.type ?? '?'}</span></td>
              <td>{o.quantity ?? '?'}</td>
              <td class="price">
                {#if editingId === o.id}
                  <input type="number" bind:value={editValue} min="1" max={MAX_PLATINUM} aria-label="New price" />
                  <button class="btn xs" onclick={() => saveEdit(o)} disabled={busy}>save</button>
                  <button class="btn xs x" onclick={() => (editingId = null)} title="Cancel">×</button>
                {:else}
                  <span class="fg">{o.platinum}<span class="unit">p</span></span>
                  <button class="btn xs ghost edit" onclick={() => startEdit(o)} disabled={busy} title="Edit price">✎</button>
                {/if}
              </td>
              {#if live.size > 0}
                <td>{#if t && !t.error && t.low_sell != null}{t.low_sell}<span class="unit">p</span>{:else}<span class="faint">—</span>{/if}</td>
                <td>{#if t && !t.error && t.top_buy != null}{t.top_buy}<span class="unit">p</span>{:else}<span class="faint">—</span>{/if}</td>
              {/if}
              <td>
                <button
                  class="visbtn {o.visible ? 'on' : 'off'}"
                  onclick={() => toggleVisible(o)}
                  disabled={busy}
                  title={o.visible ? 'Click to make hidden' : 'Click to make visible'}
                ><span class="vis" class:off={!o.visible}>{o.visible ? 'ON' : 'OFF'}</span></button>
              </td>
              <td class="act">
                {#if confirmId === o.id}
                  <button class="btn xs bad" onclick={() => removeOne(o)} disabled={busy} title="Confirm delete">Confirm</button>
                  <button class="btn xs x" onclick={() => (confirmId = null)} title="Cancel">×</button>
                {:else}
                  <button class="btn xs x" onclick={() => removeOne(o)} disabled={busy} title="Delete" aria-label="Delete {itemName(o)}">✕</button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    {#if shown.length === 0}
      <div class="line"><span class="exp">No orders match.</span> <button class="btn xs ghost" onclick={() => { show = 'all'; nameFilter = ''; }}>Clear</button></div>
    {/if}
  {/if}
</section>

<Toast {toasts} ondismiss={dismissToast} />

<style>
  /* Both panels stack on the workspace rhythm. */
  .health { margin-bottom: var(--stack); }
  /* Bars may wrap on a narrow desk (the seg + filter + three buttons). */
  .bar { flex-wrap: wrap; row-gap: var(--s1); padding-top: var(--s1); padding-bottom: var(--s1); }
  .chips { display: inline-flex; gap: var(--s1); }
  .chip {
    display: inline-flex; align-items: center;
    height: var(--ctl-xs);
    padding: 0 var(--s2);
    font-size: 11px; color: var(--muted);
    border: 1px solid var(--border); border-radius: var(--radius-tag);
  }
  .chip.warn { color: var(--warn); border-color: var(--warn); }
  .chip.bad { color: var(--bad); border-color: var(--bad); }
  .line.bad { color: var(--bad); }
  .orders .input { width: 12rem; }
  .queue { min-width: 48rem; }
  .queue td.reason .to { font-family: var(--font-mono); color: var(--muted); margin-right: var(--s2); }
  .queue td.reason .to b { color: var(--fg); font-weight: 600; }
  /* Price cell: number + a quiet ✎; the inline editor swaps in at 24px. */
  td.price { overflow: visible; }
  td.price .edit { margin-left: var(--s1); color: var(--muted); border-color: transparent; }
  td.price .edit:hover:not(:disabled) { color: var(--fg); border-color: var(--border); }
  td.price input[type="number"] {
    font: inherit; font-family: var(--font-mono); font-size: 12px;
    height: var(--ctl-xs); width: 4.5rem; padding: 0 var(--s1);
    background: var(--panel-2); color: var(--fg);
    border: 1px solid var(--border); border-radius: var(--radius-input);
    vertical-align: middle;
  }
  /* Visible toggle: a bare button around the ON/OFF pill. */
  .visbtn { appearance: none; background: transparent; border: none; padding: 0; height: var(--ctl-xs); cursor: pointer; }
  .visbtn:hover:not(:disabled) { background: transparent; }
  .visbtn:hover:not(:disabled) .vis { background: var(--panel-2); }
  button.linkish { background: none; border: 0; padding: 0; color: var(--fg); text-decoration: underline dotted; cursor: pointer; font: inherit; }
</style>
