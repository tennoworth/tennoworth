<script lang="ts">
  // Baro's manifest, priced.
  //
  // The stock comes from DE's worldState, which publishes it when the visit is
  // ANNOUNCED — days before he lands. That is the whole reason this exists:
  // the previous source only knew his stock during the 48h he was standing in
  // the relay, by which point the decision has already been made for you.
  //
  // Sorted by plat-per-ducat, because "what are my ducats worth this rotation"
  // is the question, and his shelf order answers a different one.
  import { byPlatPerDucat, ducatGap, priceManifest, stockIsCurrent } from '../lib/baro-board';
  import type { BaroVerdict } from '../lib/baro-board';
  import { planDucats, scrapCandidates } from '../lib/ducat-plan';
  import { glyphFor } from '../lib/glyphs';
  import type { Market, OwnedRecord } from '../lib/types';
  import Glyph from './Glyph.svelte';

  let {
    market,
    baro,
    ducatsHeld = 0,
    owned = null,
  }: {
    market: Market | null;
    baro: NonNullable<Market['baro']> | null;
    /** Ducats the user actually holds. 0 on the web app, where there is no
     *  inventory — the gap strip hides itself rather than reading as "you have
     *  nothing". */
    ducatsHeld?: number;
    /** Scanned inventory, for the shortfall plan. Absent on the web app. */
    owned?: Map<string, OwnedRecord> | null;
  } = $props();

  let showAll = $state(false);

  let current = $derived(stockIsCurrent(baro?.inventory_for, baro?.activation));
  let rows = $derived(byPlatPerDucat(priceManifest(baro?.inventory ?? [], market)));
  let gap = $derived(ducatGap(rows, ducatsHeld));
  // What to scrap to close the gap. Only computed when there IS a gap and an
  // inventory to close it from.
  let scrapPlan = $derived(
    gap.short > 0 && owned ? planDucats(scrapCandidates(owned, market), gap.short) : null,
  );

  // Skip and unpriced rows are collapsed by default. They are not hidden —
  // "he is selling it and it isn't worth your ducats" is information — but
  // they should not be the first thing in the list.
  let shown = $derived(
    showAll ? rows : rows.filter((r) => r.verdict === 'flip' || r.verdict === 'hold' || r.verdict === 'thin'),
  );
  let hiddenCount = $derived(rows.length - shown.length);

  const VERDICT_LABEL: Record<BaroVerdict, string> = {
    flip: 'buy · flip',
    hold: 'buy · hold',
    thin: 'thin market',
    skip: 'poor value',
    unpriced: 'not tradeable',
  };

  const VERDICT_HINT: Record<BaroVerdict, string> = {
    flip: 'Priced above its own 90-day baseline, with enough trades to sell into.',
    hold: "At or below baseline — his arrival is why. The profit is in holding for the recovery.",
    thin: 'Too few trades for the price to mean much. Treat it as a guess.',
    skip: 'The plat it returns does not justify the ducats.',
    unpriced: 'Cosmetic or bundle — it has no market listing, so there is no price to show.',
  };

  function plat(n: number | null): string {
    return n == null ? '—' : `${Math.round(n)}p`;
  }

  function ratio(n: number | null): string {
    return n == null ? '—' : n.toFixed(2);
  }
</script>

<section class="board">
  <header class="board-head">
    <h3>What he is selling</h3>
    <p class="sub">
      {rows.length} {rows.length === 1 ? 'item' : 'items'} · ranked by plat returned per ducat spent.
      {#if !current}
        <span class="stale">Stock is from a previous visit — treat it as a preview.</span>
      {/if}
    </p>
  </header>

  {#if ducatsHeld > 0 && gap.needed > 0}
    <p class="gap">
      You hold <strong>{ducatsHeld.toLocaleString()}</strong> ducats — enough for
      <strong>{gap.affordable}</strong> of the {rows.filter((r) => r.verdict === 'flip' || r.verdict === 'hold').length}
      worth buying.
      {#if gap.short > 0}
        <span class="short">{gap.short.toLocaleString()} short</span> of the full basket,
      {:else}
        The full basket is covered,
      {/if}
      which resells for about <strong>{gap.resale.toLocaleString()}p</strong> at 90-day medians.
    </p>
  {/if}

  <div class="scroll">
    <table>
      <thead>
        <tr>
          <th scope="col">Item</th>
          <th scope="col" class="num">Ducats</th>
          <th scope="col" class="num">Credits</th>
          <th scope="col" class="num">Now</th>
          <th scope="col" class="num">90d</th>
          <th scope="col" class="num">p/ducat</th>
          <th scope="col">Verdict</th>
        </tr>
      </thead>
      <tbody>
        {#each shown as row (row.unique ?? row.item)}
          <tr class:top={row.verdict === 'flip'}>
            <th scope="row" class="name">
              <Glyph name={glyphFor(market?.path_to_info?.[row.unique ?? '']?.category)} />
              {row.item}
            </th>
            <td class="num ducat">{row.ducats?.toLocaleString() ?? '—'}</td>
            <td class="num muted">{row.credits ? `${Math.round(row.credits / 1000)}k` : '—'}</td>
            <td class="num">{plat(row.price)}</td>
            <td class="num muted">{plat(row.baseline)}</td>
            <td class="num">{ratio(row.platPerDucat)}</td>
            <td>
              <span class="tag {row.verdict}" title={VERDICT_HINT[row.verdict]}>
                {VERDICT_LABEL[row.verdict]}
              </span>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

  {#if scrapPlan && scrapPlan.picks.length}
    <details class="scrap">
      <summary>close the {gap.short.toLocaleString()}-ducat gap</summary>
      <p class="sub">
        Ranked by ducats gained per plat given up — scrapping a 65-ducat part
        worth 18p to save a 45-ducat part worth 3p is the trade to avoid.
      </p>
      <ul class="picks">
        {#each scrapPlan.picks as pick (pick.slug)}
          <li>
            <span class="pick-n">{pick.spare}×</span>
            <span class="pick-name">{pick.name}</span>
            <span class="pick-d">{pick.totalDucats}d</span>
            <span class="pick-p">{pick.totalPlat > 0 ? `${Math.round(pick.totalPlat)}p given up` : 'unsold anyway'}</span>
          </li>
        {/each}
      </ul>
      <p class="sub">
        <strong>{scrapPlan.ducats.toLocaleString()}</strong> ducats for
        <strong>{Math.round(scrapPlan.platGivenUp)}p</strong> of market value.
        {#if scrapPlan.short > 0}
          <span class="short">Still {scrapPlan.short.toLocaleString()} short.</span>
        {/if}
      </p>
      {#if scrapPlan.heldBack.length}
        <p class="sub">
          Held back as worth more sold than scrapped:
          {scrapPlan.heldBack
            .slice(0, 4)
            .map((h) => `${h.name} (${h.ducats}d / ${Math.round(h.plat)}p)`)
            .join(', ')}.
        </p>
      {/if}
    </details>
  {/if}

  {#if hiddenCount > 0 || showAll}
    <button type="button" class="more" onclick={() => (showAll = !showAll)}>
      {showAll ? 'Hide poor-value and untradeable lines' : `Show ${hiddenCount} more (poor value or not tradeable)`}
    </button>
  {/if}
</section>

<style>
  .board {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }
  .board-head h3 {
    margin: 0;
  }
  .sub,
  .gap {
    margin: 0;
    font-size: 0.9rem;
    color: var(--muted);
  }
  .gap strong {
    color: var(--fg);
  }
  .short {
    color: var(--warn);
  }
  .stale {
    color: var(--warn);
  }

  .scroll {
    overflow-x: auto;
    border: 1px solid var(--border);
    background: var(--panel);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    min-width: 42rem;
  }
  th,
  td {
    padding: 0.35rem 0.6rem;
    text-align: left;
    border-bottom: 1px solid var(--hairline);
    font-size: 0.9rem;
  }
  thead th {
    background: var(--panel-2);
    color: var(--muted);
    font-size: 0.78rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    white-space: nowrap;
  }
  tbody tr:last-child th,
  tbody tr:last-child td {
    border-bottom: 0;
  }
  tbody tr.top {
    background: color-mix(in srgb, var(--good) 8%, transparent);
  }
  .name {
    font-weight: 500;
    color: var(--fg);
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }
  /* Digits line up in a column so a scan down the ducat cost is readable. */
  .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .muted {
    color: var(--muted);
  }
  .ducat {
    color: var(--ducat);
  }

  .tag {
    display: inline-block;
    padding: 0 0.35rem;
    font-size: 0.75rem;
    letter-spacing: 0.04em;
    border: 1px solid currentColor;
    white-space: nowrap;
    /* The verdict is also the row's only opinion, so it gets a cursor hint
       that there is a reason behind it. */
    cursor: help;
  }
  .tag.flip {
    color: var(--good);
  }
  .tag.hold {
    color: var(--warn);
  }
  .tag.thin,
  .tag.skip,
  .tag.unpriced {
    color: var(--faint);
  }

  .more {
    align-self: flex-start;
  }
  .scrap > summary {
    cursor: pointer;
    color: var(--muted);
    font-size: 0.9rem;
  }
  .scrap[open] > summary {
    margin-bottom: 0.5rem;
  }
  .picks {
    margin: 0.4rem 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }
  .picks li {
    display: flex;
    gap: 0.5rem;
    align-items: baseline;
    font-size: 0.88rem;
  }
  .pick-n {
    font-variant-numeric: tabular-nums;
    color: var(--muted);
    min-width: 2rem;
    text-align: right;
  }
  .pick-name {
    flex: 1 1 auto;
  }
  .pick-d {
    color: var(--ducat);
    font-variant-numeric: tabular-nums;
  }
  .pick-p {
    color: var(--faint);
    font-variant-numeric: tabular-nums;
  }
</style>
