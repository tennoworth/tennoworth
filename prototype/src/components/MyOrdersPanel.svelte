<script lang="ts">
  import { onDestroy } from 'svelte';
  import { DesktopCmdError, type Transport } from '../lib/transport';
  import { MAX_PLATINUM } from '../lib/limits';
  import { humanError } from '../lib/errors';
  import Toast from './Toast.svelte';

  // WFM order shape is open — many fields appear depending on the
  // endpoint version (v1 vs v2). We type only what we read.
  interface WfmOrder {
    id: string;
    platinum: number;
    visible: boolean;
    type?: 'sell' | 'buy';
    quantity?: number;
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
    /** Bumped by the parent when the WFM session unlocks — re-fetches so a
     *  fetch gated on needs_login/needs_unlock retries automatically. */
    sessionEpoch?: number;
    /** Desktop lock-state rejection → parent raises the auth dialog (which
     *  tries the OS-keyring silent unlock before showing the passphrase). */
    onauthrequired?: (code: 'needs_login' | 'needs_unlock') => void;
  }
  let { transport, sessionEpoch = 0, onauthrequired }: Props = $props();

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
      <button class="ghost" onclick={loadOrders} disabled={phase === 'loading'}>Refresh</button>
    </div>
  </header>

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
    border-radius: 6px;
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
    border-radius: 4px;
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
    border-radius: 4px;
    cursor: pointer;
  }
  .tiny:hover:not(:disabled) { color: var(--fg); }
  .tiny.bad { color: var(--bad); border-color: color-mix(in srgb, var(--bad) 40%, var(--border)); }
  .tiny.bad:hover:not(:disabled) { background: color-mix(in srgb, var(--bad) 12%, transparent); }
  input[type="number"] {
    font: inherit;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    background: var(--panel-2);
    border: 1px solid var(--border);
    color: var(--fg);
    border-radius: 4px;
    padding: 2px 4px;
  }
  .muted.bad { color: var(--bad); }
</style>
