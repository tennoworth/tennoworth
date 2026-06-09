<script lang="ts">
  import { submitPlan, bulkVisibility } from '../lib/companion';
  import type { CompanionConfig, ItemResult } from '../lib/types';

  /** Row shape passed in from ResultsTable / App.svelte. */
  interface InputRow {
    key?: string;
    slug: string;
    subtype?: string | null;
    name: string;
    owned: number;
    low_sell: number;
    avg_price: number;
  }

  interface PlanRow {
    key: string;
    slug: string;
    subtype: string | null;
    name: string;
    include: boolean;
    platinum: number;
    quantity: number;
    owned: number;
    reference_low_sell: number;
    avg: number;
  }

  interface Props {
    open?: boolean;
    rows: InputRow[];
    config: CompanionConfig | null;
    onclose?: () => void;
  }
  let { open = $bindable(false), rows, config, onclose }: Props = $props();

  let plan = $state<PlanRow[]>([]);
  type Phase = 'review' | 'sending' | 'results' | 'error';
  let phase = $state<Phase>('review');
  let serverResults = $state<ItemResult[]>([]);
  let networkError = $state<string | null>(null);

  function initialPlanFor(rows: InputRow[]): PlanRow[] {
    return rows.map((r) => {
      const target = r.low_sell > 0 ? r.low_sell : Math.round(r.avg_price);
      return {
        key: r.key ?? r.slug,
        slug: r.slug,
        subtype: r.subtype ?? null,
        name: r.name,
        include: true,
        platinum: Math.max(5, target),
        quantity: 1,
        owned: r.owned,
        reference_low_sell: r.low_sell || 0,
        avg: r.avg_price,
      };
    });
  }

  // Re-initialize when modal opens.
  $effect(() => {
    if (open) {
      plan = initialPlanFor(rows ?? []);
      phase = 'review';
      serverResults = [];
      networkError = null;
    }
  });

  let selectedCount = $derived(plan.filter((r) => r.include).length);
  let totalPlat = $derived(
    plan
      .filter((r) => r.include)
      .reduce((s, r) => s + r.platinum * r.quantity, 0)
  );
  // Max price matches the companion's MAX_PLATINUM. 999 was conservative
  // and silently blocked listings for maxed Arcane Energize / Galvanized
  // Aptitude etc. (real prices 1500–2500p). WFM's own UI caps at 3000.
  const MAX_PLATINUM = 3000;
  let canSubmit = $derived(
    selectedCount > 0 && selectedCount <= 50 && plan.every(
      (r) => !r.include || (r.platinum >= 5 && r.platinum <= MAX_PLATINUM && r.quantity >= 1 && r.quantity <= r.owned)
    )
  );

  function close(): void {
    open = false;
    onclose?.();
  }

  function errMsg(e: unknown): string {
    return e instanceof Error ? e.message : String(e);
  }

  async function send(): Promise<void> {
    if (!config) return;
    phase = 'sending';
    networkError = null;
    const items = plan
      .filter((r) => r.include)
      .map((r) => ({
        slug: r.slug,
        platinum: r.platinum,
        quantity: r.quantity,
        order_type: 'sell' as const,
        visible: false,
        subtype: r.subtype || undefined,
        reference_low_sell: r.reference_low_sell || undefined,
      }));
    try {
      const resp = await submitPlan(config, items);
      serverResults = resp.results || [];
      phase = 'results';
    } catch (e) {
      networkError = errMsg(e);
      phase = 'error';
    }
  }

  let okCount = $derived(serverResults.filter((r) => r.status === 'ok').length);
  let errCount = $derived(serverResults.filter((r) => r.status !== 'ok').length);

  let visibilityBusy = $state(false);
  let visibilityDone = $state(false);
  let visibilityResults = $state<ItemResult[]>([]);

  function setAll(include: boolean): void {
    for (let i = 0; i < plan.length; i++) plan[i].include = include;
  }

  async function makeAllVisible(): Promise<void> {
    if (!config) return;
    const ids = serverResults
      .filter((r) => r.status === 'ok' && r.order_id)
      .map((r) => r.order_id as string);
    if (ids.length === 0) return;
    visibilityBusy = true;
    try {
      const resp = await bulkVisibility(config, ids, true);
      visibilityResults = resp?.results || [];
      visibilityDone = true;
    } catch (e) {
      networkError = errMsg(e);
      phase = 'error';
    } finally {
      visibilityBusy = false;
    }
  }

  let visibleOkCount = $derived(visibilityResults.filter((r) => r.status === 'ok').length);
  let visibleErrCount = $derived(visibilityResults.filter((r) => r.status !== 'ok').length);
</script>

{#if open}
  <div class="backdrop" role="dialog" aria-modal="true" aria-labelledby="rm-title">
    <div class="modal">
      <header>
        <h2 id="rm-title">List on warframe.market</h2>
        <button class="x" onclick={close} aria-label="Close">×</button>
      </header>

      {#if phase === 'review'}
        <p class="lead">
          Review every row. Default price is the current lowest live sell
          (floored at 5p). Everything is created <strong>invisible</strong>
          — you toggle visible later, after spot-checking on warframe.market.
        </p>

        <div class="bulkrow">
          <button class="ghost" onclick={() => setAll(true)}>Select all</button>
          <button class="ghost" onclick={() => setAll(false)}>Deselect all</button>
        </div>

        <div class="scroll">
          <table>
            <thead>
              <tr>
                <th></th>
                <th>Item</th>
                <th>Qty</th>
                <th>Owned</th>
                <th>Price (p)</th>
                <th>Avg</th>
                <th>Subtotal</th>
              </tr>
            </thead>
            <tbody>
              {#each plan as row, i (row.key)}
                <tr class:dim={!row.include}>
                  <td><input type="checkbox" bind:checked={plan[i].include} /></td>
                  <td>{row.name}</td>
                  <td>
                    <input
                      type="number"
                      min="1"
                      max={row.owned}
                      bind:value={plan[i].quantity}
                      disabled={!row.include}
                    />
                  </td>
                  <td class="muted">{row.owned}</td>
                  <td>
                    <input
                      type="number"
                      min="5"
                      max={MAX_PLATINUM}
                      bind:value={plan[i].platinum}
                      disabled={!row.include}
                    />
                  </td>
                  <td class="muted">{row.avg.toFixed(0)}</td>
                  <td class="right">{(row.platinum * row.quantity).toLocaleString()}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>

        <footer>
          <div class="totals">
            <span><strong>{selectedCount}</strong> items</span>
            <span><strong>{totalPlat.toLocaleString()}</strong> plat total</span>
            {#if selectedCount > 50}
              <span class="warn">Batch cap is 50 — deselect some.</span>
            {/if}
          </div>
          <div class="actions">
            <button class="ghost" onclick={close}>Cancel</button>
            <button onclick={send} disabled={!canSubmit}>
              Send {selectedCount} listings (invisible)
            </button>
          </div>
        </footer>
      {:else if phase === 'sending'}
        <p class="lead">
          Posting to warframe.market via the companion. ~3 listings/second —
          this will take ~{Math.ceil((selectedCount * 0.35) + 1)} s.
        </p>
        <div class="spinner">Sending…</div>
      {:else if phase === 'results'}
        <p class="lead">
          Done. <span class="ok">{okCount} created</span>
          {#if errCount > 0}· <span class="bad">{errCount} failed</span>{/if}.
          New listings are <strong>invisible</strong> on WFM — log in to
          warframe.market, eyeball them, then make them visible.
        </p>
        <div class="scroll">
          <table>
            <thead><tr><th></th><th>Item</th><th>Detail</th></tr></thead>
            <tbody>
              {#each serverResults as r, i (i)}
                <tr>
                  <td class:ok={r.status === 'ok'} class:bad={r.status !== 'ok'}>
                    {r.status === 'ok' ? '✓' : '✗'}
                  </td>
                  <td>{r.slug}</td>
                  <td class="muted">{r.message ?? r.order_id ?? ''}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
        {#if visibilityDone}
          <p class="lead">
            Visibility toggled. <span class="ok">{visibleOkCount} now visible</span>
            {#if visibleErrCount > 0}· <span class="bad">{visibleErrCount} failed</span>{/if}.
          </p>
        {/if}

        <footer>
          <div></div>
          <div class="actions">
            {#if okCount > 0 && !visibilityDone}
              <button onclick={makeAllVisible} disabled={visibilityBusy}>
                {visibilityBusy ? 'Making visible…' : `Make ${okCount} visible`}
              </button>
            {/if}
            <button class={visibilityDone ? '' : 'ghost'} onclick={close}>Done</button>
          </div>
        </footer>
      {:else if phase === 'error'}
        <p class="lead bad">{networkError}</p>
        <footer>
          <div></div>
          <div class="actions">
            <button class="ghost" onclick={close}>Cancel</button>
            <button onclick={() => (phase = 'review')}>Back to review</button>
          </div>
        </footer>
      {/if}
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    backdrop-filter: blur(2px);
    display: grid;
    place-items: center;
    z-index: 1000;
    padding: 24px;
  }
  .modal {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 12px;
    width: min(900px, 100%);
    max-height: 88vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px;
    border-bottom: 1px solid var(--border);
  }
  header h2 {
    margin: 0;
    font-size: 13px;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--accent);
    font-weight: 600;
  }
  .x {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--muted);
    font-size: 16px;
    line-height: 1;
    width: 26px;
    height: 26px;
    border-radius: 6px;
    cursor: pointer;
  }
  .x:hover { color: var(--fg); }
  .lead {
    padding: 14px 18px 0;
    margin: 0;
    font-size: 13px;
    color: var(--muted);
    line-height: 1.5;
    max-width: 80ch;
  }
  .lead.bad { color: var(--bad); }
  .lead strong { color: var(--fg); }
  .bulkrow {
    display: flex;
    gap: 8px;
    padding: 8px 18px 0;
  }
  .scroll {
    overflow: auto;
    margin: 12px 0;
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-variant-numeric: tabular-nums;
  }
  th, td {
    padding: 7px 12px;
    text-align: left;
    border-bottom: 1px solid var(--border);
    font-size: 12.5px;
  }
  th {
    background: var(--panel-2);
    font-weight: 600;
    color: var(--muted);
    letter-spacing: 0.04em;
    text-transform: uppercase;
    font-size: 11px;
    position: sticky;
    top: 0;
  }
  td.right { text-align: right; }
  td.muted { color: var(--muted); }
  tr.dim { opacity: 0.45; }
  input[type="number"] {
    font: inherit;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12.5px;
    width: 64px;
    background: var(--panel-2);
    border: 1px solid var(--border);
    color: var(--fg);
    border-radius: 5px;
    padding: 3px 6px;
  }
  input[type="number"]:disabled { opacity: 0.4; }
  footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 12px 18px;
    border-top: 1px solid var(--border);
    gap: 12px;
    flex-wrap: wrap;
  }
  .totals {
    display: flex;
    gap: 18px;
    font-size: 13px;
    color: var(--muted);
  }
  .totals strong { color: var(--fg); font-weight: 600; }
  .totals .warn { color: var(--warn); }
  .actions { display: flex; gap: 8px; }
  button.ghost {
    background: transparent;
    color: var(--muted);
    border: 1px solid var(--border);
    padding: 4px 10px;
    border-radius: 6px;
    font-size: 12px;
    cursor: pointer;
  }
  button.ghost:hover { background: var(--panel-2); color: var(--fg); }
  td.ok { color: var(--good); font-weight: 600; }
  td.bad { color: var(--bad); font-weight: 600; }
  .ok { color: var(--good); }
  .bad { color: var(--bad); }
  .spinner {
    padding: 32px;
    text-align: center;
    color: var(--muted);
  }
</style>
