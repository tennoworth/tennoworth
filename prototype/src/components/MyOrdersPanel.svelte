<script lang="ts">
  import { fetchOrders, updateOrder, deleteOrder } from '../lib/companion';
  import type { CompanionConfig } from '../lib/types';

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

  interface Props { config: CompanionConfig | null; }
  let { config }: Props = $props();

  type Phase = 'idle' | 'loading' | 'done' | 'error';
  let phase = $state<Phase>('idle');
  let error = $state<string | null>(null);
  let orders = $state<WfmOrder[]>([]);
  let busyIds = $state<Set<string>>(new Set());
  let editingId = $state<string | null>(null);
  let editValue = $state(0);

  function loadOrders(): void {
    if (!config) return;
    phase = 'loading';
    error = null;
    fetchOrders(config)
      .then((r) => {
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
        error = e instanceof Error ? e.message : String(e);
        phase = 'error';
      });
  }

  // Initial load + reload when the config reference changes (reconnect → fresh
  // fetch). This $effect alone covers mount when config is already set; a
  // separate onMount(loadOrders) just double-fired GET /orders on mount.
  $effect(() => {
    if (config) loadOrders();
  });

  function markBusy(id: string, on: boolean): void {
    const next = new Set(busyIds);
    if (on) next.add(id); else next.delete(id);
    busyIds = next;
  }

  function errMsg(e: unknown): string {
    return e instanceof Error ? e.message : String(e);
  }

  async function toggleVisible(o: WfmOrder): Promise<void> {
    if (!config) return;
    markBusy(o.id, true);
    try {
      await updateOrder(config, o.id, { visible: !o.visible });
      o.visible = !o.visible;
      orders = [...orders];
    } catch (e) {
      alert(`Couldn't toggle: ${errMsg(e)}`);
    } finally {
      markBusy(o.id, false);
    }
  }

  function startEdit(o: WfmOrder): void {
    editingId = o.id;
    editValue = o.platinum;
  }

  async function saveEdit(o: WfmOrder): Promise<void> {
    if (!config) return;
    const newPrice = Number(editValue);
    if (!newPrice || newPrice < 1) return;
    markBusy(o.id, true);
    try {
      await updateOrder(config, o.id, { platinum: newPrice });
      o.platinum = newPrice;
      orders = [...orders];
      editingId = null;
    } catch (e) {
      alert(`Couldn't update: ${errMsg(e)}`);
    } finally {
      markBusy(o.id, false);
    }
  }

  async function removeOne(o: WfmOrder): Promise<void> {
    if (!config) return;
    if (!confirm(`Delete this listing? (${itemName(o)} at ${o.platinum}p)`)) return;
    markBusy(o.id, true);
    try {
      await deleteOrder(config, o.id);
      orders = orders.filter((x) => x.id !== o.id);
    } catch (e) {
      alert(`Couldn't delete: ${errMsg(e)}`);
    } finally {
      markBusy(o.id, false);
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
        {#if phase === 'loading'}loading…
        {:else if phase === 'done'}{orders.length} active
        {/if}
      </span>
      <button class="ghost" onclick={loadOrders} disabled={phase === 'loading'}>Refresh</button>
    </div>
  </header>

  {#if phase === 'error'}
    <div class="muted bad">Couldn't load orders: {error}</div>
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
            <tr class:dim={busy}>
              <td>{itemName(o)}</td>
              <td class:sell={o.type === 'sell'} class:buy={o.type === 'buy'}>
                {o.type ?? '?'}
              </td>
              <td class="right">{o.quantity ?? '?'}</td>
              <td class="right">
                {#if editingId === o.id}
                  <input type="number" bind:value={editValue} min="1" max="9999" style="width:64px" />
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
                  title={o.visible ? 'Click to make invisible' : 'Click to make visible'}
                >{o.visible ? 'on' : 'off'}</button>
              </td>
              <td>
                <button class="tiny bad" onclick={() => removeOne(o)} disabled={busy} title="Delete">✕</button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>

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
