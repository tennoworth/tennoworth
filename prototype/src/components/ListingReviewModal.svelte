<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import {
    DesktopCmdError, desktopLiveTopPrices, isDesktopRuntime, LIVE_TOP_PROGRESS_EVENT,
    type LiveTop, type Transport,
  } from '../lib/transport';
  import { listenForTauriEvent } from '../lib/desktop-update';
  import type { ItemResult } from '../lib/types';
  import { MAX_PLATINUM, MIN_PLATINUM, MAX_PLAN_ITEMS } from '../lib/limits';
  import { humanError } from '../lib/errors';
  import { plat, ownedBreakdown, LEVELED_NOTE_TITLE, keptNoteTitle } from '../lib/format';
  import DialogHeader from './DialogHeader.svelte';

  /** Row shape passed in from ResultsTable / App.svelte. */
  interface InputRow {
    key?: string;
    slug: string;
    subtype?: string | null;
    name: string;
    owned: number;
    sellable?: number;
    leveled?: number;
    low_sell: number;
    avg_price: number;
    clearing_price?: number;
    kept_lvl?: number | null;
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
    sellable: number;
    leveled: number;
    rank: number;
    reference_low_sell: number;
    avg: number;
  }

  interface Props {
    open?: boolean;
    rows: InputRow[];
    /** The app's boot-selected transport — Tauri IPC into wfm-core. */
    transport: Transport;
    /** Desktop only: a listing call came back `needs_login` / `needs_unlock`.
     *  The app opens the matching auth dialog on top of this modal; the user
     *  resends after authenticating. */
    onauthrequired?: (code: 'needs_login' | 'needs_unlock') => void;
    onclose?: () => void;
  }
  let { open = $bindable(false), rows, transport, onauthrequired, onclose }: Props = $props();

  let plan = $state<PlanRow[]>([]);
  type Phase = 'review' | 'sending' | 'results' | 'error';
  let phase = $state<Phase>('review');
  let serverResults = $state<ItemResult[]>([]);
  let networkError = $state<string | null>(null);

  function initialPlanFor(rows: InputRow[]): PlanRow[] {
    return rows.map((r) => {
      // Prefill from the clamped clearing price, not raw low_sell — the raw
      // ask inherits every troll listing (a lone 100p ask on a 10p item, or
      // a 1p undercut on an undercut day). Falls back for older callers.
      const target =
        (r.clearing_price ?? 0) > 0 ? Math.round(r.clearing_price as number)
        : r.low_sell > 0 ? r.low_sell
        : Math.round(r.avg_price);
      // Cap using sellable (owned minus the "Keep copies" reserve), not raw
      // owned — this is the last line of defense against listing a copy the
      // user asked to hold back. Falls back to owned for callers that
      // predate the reserve field.
      const sellable = r.sellable ?? r.owned;
      return {
        key: r.key ?? r.slug,
        slug: r.slug,
        subtype: r.subtype ?? null,
        name: r.name,
        include: true,
        platinum: Math.max(5, target),
        quantity: 1,
        owned: r.owned,
        sellable,
        leveled: r.leveled ?? 0,
        // Rank 0 = unranked, the tier dupe stacks actually are. Editable so
        // a leveled copy can be listed at its real rank; the app only
        // sends rank for items WFM ranks (mods/arcanes), so a non-zero rank
        // on a rankless item is ignored server-side, not an error.
        rank: 0,
        reference_low_sell: r.low_sell || 0,
        avg: r.avg_price,
      };
    });
  }

  // Prefill sanity flag: a suggested price far off the 48h average deserves
  // a second look before it goes out in a 50-item batch.
  function priceOff(r: PlanRow): boolean {
    return r.avg > 0 && (r.platinum > r.avg * 1.3 || r.platinum < r.avg * 0.7);
  }

  // Re-initialize when the modal OPENS — and only then.
  //
  // `rows` is read through untrack() deliberately. The caller passes
  // `reviewRowsOverride ?? listableRows.slice(0, 50)`, and that .slice() mints
  // a fresh array identity every time the `listableRows` derived recomputes.
  // Tracking it meant any background recompute while the modal was open
  // re-ran this init and threw away the user's in-flight price and quantity
  // edits, mid-review, on a batch of up to 50 listings. The rows to review are
  // whatever they were when the modal opened; nothing here wants live updates.
  $effect(() => {
    if (open) {
      plan = initialPlanFor(untrack(() => rows) ?? []);
      phase = 'review';
      serverResults = [];
      networkError = null;
    }
  });

  let selectedCount = $derived(plan.filter((r) => r.include).length);

  // ---- Live prices (desktop only) ----
  // The prefill comes from the 2-hourly snapshot. One click asks WFM for the
  // ≤5 best ONLINE asks/bids for each selected row's exact tier (rank /
  // relic refinement) — the price you'd actually be competing with right
  // now. Paced at WFM's 3 req/s, so a big batch shows a counter.
  const canLive = isDesktopRuntime();
  type LiveState = 'idle' | 'running' | 'done' | 'error';
  let liveState = $state<LiveState>('idle');
  let liveProgress = $state({ done: 0, total: 0 });
  let liveError = $state<string | null>(null);
  let live = $state<Map<string, LiveTop>>(new Map());
  let liveListenerArmed = false;
  let unlistenLiveProgress = () => {};

  onDestroy(() => unlistenLiveProgress());

  function liveKey(slug: string, rank: number, subtype: string | null): string {
    return `${slug}|${rank}|${subtype ?? ''}`;
  }
  function liveFor(row: PlanRow): LiveTop | undefined {
    return live.get(liveKey(row.slug, row.rank, row.subtype));
  }

  async function checkLivePrices(): Promise<void> {
    const targets = plan.filter((r) => r.include);
    if (targets.length === 0) return;
    if (!liveListenerArmed) {
      liveListenerArmed = true;
      unlistenLiveProgress = listenForTauriEvent<{ done: number; total: number }>(LIVE_TOP_PROGRESS_EVENT, (p) => {
        liveProgress = p;
      });
    }
    liveState = 'running';
    liveError = null;
    liveProgress = { done: 0, total: targets.length };
    try {
      const res = await desktopLiveTopPrices(
        targets.map((r) => ({ slug: r.slug, rank: r.rank, subtype: r.subtype })),
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

  /** Set the row's price to the live lowest online ask (match, don't undercut). */
  function useLive(i: number): void {
    const t = liveFor(plan[i]);
    if (t?.low_sell != null) plan[i].platinum = Math.max(MIN_PLATINUM, t.low_sell);
  }
  function useLiveAll(): void {
    plan.forEach((_, i) => { if (plan[i].include) useLive(i); });
  }
  /** How the row's price sits against the live book: over the lowest ask
   *  (won't sell first), under the top bid (leaving plat on the table), or ok. */
  function liveVerdict(row: PlanRow): 'above' | 'below-bid' | 'ok' | null {
    const t = liveFor(row);
    if (!t || t.error) return null;
    if (t.low_sell != null && row.platinum > t.low_sell) return 'above';
    if (t.top_buy != null && row.platinum < t.top_buy) return 'below-bid';
    return 'ok';
  }
  let totalPlat = $derived(
    plan
      .filter((r) => r.include)
      .reduce((s, r) => s + r.platinum * r.quantity, 0)
  );
  let canSubmit = $derived(
    selectedCount > 0 && selectedCount <= MAX_PLAN_ITEMS && plan.every(
      (r) => !r.include || (r.platinum >= MIN_PLATINUM && r.platinum <= MAX_PLATINUM && r.quantity >= 1 && r.quantity <= r.sellable)
    )
  );

  function close(): void {
    open = false;
    onclose?.();
  }

  /** Desktop lock-state rejection → hand off to the auth dialogs and return to
   *  review so Send is one click away once the session unlocks. */
  function handleAuthCode(e: unknown): boolean {
    if (e instanceof DesktopCmdError && (e.code === 'needs_login' || e.code === 'needs_unlock')) {
      phase = 'review';
      onauthrequired?.(e.code);
      return true;
    }
    return false;
  }

  async function send(): Promise<void> {
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
        rank: r.rank > 0 ? r.rank : undefined,
        subtype: r.subtype || undefined,
        reference_low_sell: r.reference_low_sell || undefined,
      }));
    try {
      const resp = await transport.submitPlan(items);
      serverResults = resp.results || [];
      phase = 'results';
    } catch (e) {
      if (handleAuthCode(e)) return;
      networkError = humanError(e);
      phase = 'error';
    }
  }

  let updatedCount = $derived(
    serverResults.filter((r) => r.status === 'ok' && r.action === 'updated').length,
  );
  let okCount = $derived(
    serverResults.filter((r) => r.status === 'ok').length - updatedCount,
  );
  let errCount = $derived(serverResults.filter((r) => r.status !== 'ok').length);

  // The results table only carries slugs; map back to human names so the
  // "what did I just list" review isn't a wall of snake_case.
  let planNameBySlug = $derived(
    new Map(plan.map((r) => [r.slug, r.name])),
  );

  let visibilityBusy = $state(false);
  let visibilityDone = $state(false);
  let visibilityResults = $state<ItemResult[]>([]);

  function setAll(include: boolean): void {
    for (let i = 0; i < plan.length; i++) plan[i].include = include;
  }

  async function makeAllVisible(): Promise<void> {
    // Only freshly-created orders: updated ones keep the visibility the user
    // chose on WFM (matches the "N created" count on the button).
    const ids = serverResults
      .filter((r) => r.status === 'ok' && r.action !== 'updated' && r.order_id)
      .map((r) => r.order_id as string);
    if (ids.length === 0) return;
    visibilityBusy = true;
    try {
      const resp = await transport.bulkVisibility(ids, true);
      visibilityResults = resp?.results || [];
      visibilityDone = true;
    } catch (e) {
      // A lock-state rejection here (desktop logout between send and toggle)
      // must not dump the user to the error phase and lose the results table.
      if (e instanceof DesktopCmdError && (e.code === 'needs_login' || e.code === 'needs_unlock')) {
        onauthrequired?.(e.code);
      } else {
        networkError = humanError(e);
        phase = 'error';
      }
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
      <DialogHeader titleId="rm-title" title="List on warframe.market" onclose={close} />

      {#if phase === 'review'}
        <p class="lead">
          Review every row. Default price is the estimated clearing price —
          the lowest live ask, sanity-clamped against the recent median so a
          lone troll listing can't set it (floored at 5p). Rank 0 = unranked;
          set a rank only if you're listing a leveled copy. Listings go up
          <strong>hidden</strong> — no buyers can see them until you flip
          them visible on the results screen (or later, in My orders).
        </p>

        <div class="bulkrow">
          <button class="ghost" onclick={() => setAll(true)}>Select all</button>
          <button class="ghost" onclick={() => setAll(false)}>Deselect all</button>
          {#if canLive}
            <span class="spacer"></span>
            <button
              class="ghost live-btn"
              onclick={checkLivePrices}
              disabled={liveState === 'running' || selectedCount === 0}
              title="Ask warframe.market for the ≤5 best online asks and bids for each selected row's exact rank / refinement, right now. Paced to WFM's rate limit (~3 items per second)."
            >
              {#if liveState === 'running'}
                Checking live prices… {liveProgress.done}/{liveProgress.total}
              {:else if liveState === 'done'}
                Re-check live prices
              {:else}
                Check live prices
              {/if}
            </button>
            {#if liveState === 'done' && live.size > 0}
              <button class="ghost" onclick={useLiveAll} title="Set every selected row's price to its live lowest online ask (match it — no undercutting).">Match lowest asks</button>
            {/if}
            {#if liveState === 'error' && liveError}
              <span class="live-err">{liveError}</span>
            {/if}
          {/if}
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
                {#if canLive}<th title="Live lowest online ask / highest online bid for this exact rank or refinement (after “Check live prices”). Click a value to use it.">Live ask / bid</th>{/if}
                <th title="Mod/arcane rank of the copies you're listing. 0 = unranked (dupe stacks). Ignored for items WFM doesn't rank.">Rank</th>
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
                      max={row.sellable}
                      bind:value={plan[i].quantity}
                      disabled={!row.include}
                    />
                  </td>
                  <td class="muted">
                    {#if row.sellable < row.owned}
                      {@const bd = ownedBreakdown(row.owned, row.sellable, row.leveled)}
                      {row.owned} owned{#if bd.leveledPart > 0}{' · '}<span class="leveled-note" title={LEVELED_NOTE_TITLE}>{bd.leveledPart} leveled</span>{/if}{#if bd.keptPart > 0}{' · '}<span class="kept-note" title={keptNoteTitle(bd.keptPart)}>{bd.keptPart} kept</span>{/if}
                    {:else}
                      {row.owned} owned
                    {/if}
                  </td>
                  <td>
                    <input
                      type="number"
                      min="5"
                      max={MAX_PLATINUM}
                      bind:value={plan[i].platinum}
                      disabled={!row.include}
                      class:off={row.include && priceOff(row)}
                      title={row.include && priceOff(row) ? `More than 30% off the 48h average (${row.avg.toFixed(0)}p) — double-check before sending` : undefined}
                    />
                  </td>
                  <td class="muted">{plat(row.avg)}</td>
                  {#if canLive}
                    {@const t = liveFor(row)}
                    {@const v = liveVerdict(row)}
                    <td class="live-cell" class:above={v === 'above'} class:belowbid={v === 'below-bid'}>
                      {#if !t}
                        <span class="muted">·</span>
                      {:else if t.error}
                        <span class="muted" title={t.error}>n/a</span>
                      {:else}
                        {#if t.low_sell != null}
                          <button class="linkish" onclick={() => useLive(i)} disabled={!row.include}
                            title={`Online asks: ${t.sells.join(', ')}p — click to price at ${t.low_sell}p`}>{t.low_sell}p</button>
                        {:else}<span class="muted" title="No online sellers right now">no ask</span>{/if}
                        <span class="muted"> / </span>
                        {#if t.top_buy != null}
                          <span title={`Online bids: ${t.buys.join(', ')}p`}>{t.top_buy}p</span>
                        {:else}<span class="muted" title="No online buyers right now">no bid</span>{/if}
                        {#if v === 'above'}<span class="verdict" title="Your price is above the lowest online ask — it won't be the first to sell.">▲</span>{/if}
                        {#if v === 'below-bid'}<span class="verdict" title="A live buyer is bidding more than your price — you'd be leaving plat on the table.">▼</span>{/if}
                      {/if}
                    </td>
                  {/if}
                  <td>
                    <input
                      type="number"
                      min="0"
                      max="10"
                      class="rank"
                      bind:value={plan[i].rank}
                      disabled={!row.include}
                    />
                  </td>
                  <td class="right">{plat(row.platinum * row.quantity)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>

        <footer>
          <div class="totals">
            <span><strong>{selectedCount}</strong> items</span>
            <span><strong>{plat(totalPlat)}</strong> plat total</span>
            {#if selectedCount > 50}
              <span class="warn">Batch cap is 50 — deselect some.</span>
            {/if}
          </div>
          <div class="actions">
            <button class="ghost" onclick={close}>Cancel</button>
            <button onclick={send} disabled={!canSubmit}>
              Send {selectedCount} listings (hidden)
            </button>
          </div>
        </footer>
      {:else if phase === 'sending'}
        <p class="lead">
          Posting to warframe.market. ~3 listings/second —
          this will take ~{Math.ceil((selectedCount * 0.35) + 1)} s.
        </p>
        <div class="spinner">Sending…</div>
      {:else if phase === 'results'}
        <p class="lead">
          Done. <span class="ok">{okCount} created</span>
          {#if updatedCount > 0}· <span class="ok">{updatedCount} updated</span>{/if}
          {#if errCount > 0}· <span class="bad">{errCount} failed</span>{/if}.
          {#if visibilityDone}
            Listings are <strong>visible</strong> — buyers can see them now.
          {:else}
            New listings start <strong>hidden</strong> — no buyers can see them
            yet. Flip them visible after you've reviewed the prices.
          {/if}
          Updated orders keep their existing visibility and price history.
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
                  <td>
                    <span class="item-name">{planNameBySlug.get(r.slug) ?? r.slug}</span>
                    <span class="item-slug">{r.slug}</span>
                  </td>
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
    background: var(--scrim);
    backdrop-filter: blur(2px);
    display: grid;
    place-items: center;
    z-index: 1000;
    padding: 24px;
  }
  .modal {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    width: min(900px, 100%);
    max-height: 88vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
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
    align-items: center;
    flex-wrap: wrap;
  }
  .bulkrow .spacer { flex: 1; }
  .live-err { color: var(--bad); font-size: 12px; }
  td.live-cell { white-space: nowrap; font-variant-numeric: tabular-nums; }
  td.live-cell.above .verdict { color: var(--warn); margin-left: 4px; }
  td.live-cell.belowbid .verdict { color: var(--warn); margin-left: 4px; }
  button.linkish {
    background: none;
    border: 0;
    padding: 0;
    color: var(--fg);
    text-decoration: underline dotted;
    cursor: pointer;
    font: inherit;
  }
  button.linkish:hover:not(:disabled) { color: var(--accent, var(--fg)); }
  button.linkish:disabled { cursor: default; text-decoration: none; color: var(--muted); }
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
  .item-name { color: var(--fg); }
  .item-slug {
    color: var(--muted);
    font-family: var(--font-mono);
    font-size: 11px;
    margin-left: 6px;
  }
  /* Leveled gear is a harder constraint than the Keep-copies reserve — the
     game itself won't let you trade it — so it gets the same warm tint
     ResultsTable uses for its owned-column note. */
  .leveled-note { color: var(--warn); }
  .kept-note { color: var(--muted); }
  input[type="number"] {
    font: inherit;
    font-family: var(--font-mono);
    font-size: 12.5px;
    width: 64px;
    background: var(--panel-2);
    border: 1px solid var(--border);
    color: var(--fg);
    border-radius: var(--radius-ctl);
    padding: 3px 6px;
  }
  input[type="number"]:disabled { opacity: 0.4; }
  input[type="number"].rank { width: 46px; }
  input[type="number"].off { border-color: var(--warn); }
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
    border-radius: var(--radius-ctl);
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
