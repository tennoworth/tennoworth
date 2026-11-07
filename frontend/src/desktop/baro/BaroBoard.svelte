<script lang="ts">
  import { fitTable } from '../../ui/table-fit';
  // Baro's manifest, priced.
  //
  // The stock comes from DE's worldState, which publishes it when the visit is
  // ANNOUNCED - days before he lands. That is the whole reason this exists:
  // the previous source only knew his stock during the 48h he was standing in
  // the relay, by which point the decision has already been made for you.
  //
  // Sorted by plat-per-ducat, because "what are my ducats worth this rotation"
  // is the question, and his shelf order answers a different one.
  import { ducatBasket, stockIsCurrent } from '../../domain/baro-board';
  import type { BaroValue, BaroVerdict } from '../../contracts/generated/domain';
  import { DomainResult } from '../selling/domain-result.svelte';
  import type { DucatPlan, ScrapCandidate } from '../../contracts/generated/domain';
  import { useDesktopServices } from '../shell/desktop-context';
  const { ducatPlan, baroValue } = useDesktopServices();
  import { glyphFor } from '../../ui/glyphs';
  import { plat } from '../../ui/format';
  import { formatLocale, t } from '../../i18n/i18n.svelte';
  import Rich from '../../ui/Rich.svelte';
  import type { Market, OwnedRecord } from '../../contracts/data';
  import Glyph from '../../ui/Glyph.svelte';
  import Meter from '../../ui/Meter.svelte';

  let {
    market,
    baro,
    owned = null,
    availability, unavailableItems = 0, quantitiesUnavailable = false,
  }: {
    market: Market | null;
    baro: NonNullable<Market['baro']> | null;
    /** Scanned inventory. Absent on the web app, where the board still prices
     *  his stock but can say nothing about what the reader can afford.
     *
     *  There is deliberately no `ducatsHeld`: ducats are account state and an
     *  inventory scan cannot see them. The only honest figure is what the
     *  user's spare parts would YIELD, derived below - and it must never be
     *  presented as a balance, or the scrap plan double-counts it. */
    owned?: Map<string, OwnedRecord> | null;
    availability?: ReadonlyMap<string, number>;
    unavailableItems?: number;
    quantitiesUnavailable?: boolean;
  } = $props();

  let showAll = $state(false);

  let current = $derived(stockIsCurrent(baro?.inventory_for, baro?.activation));
  const valueResult = new DomainResult<BaroValue | null>(() => null);
  $effect(() => {
    const stock = baro?.inventory ?? [];
    const snapshot = market;
    return valueResult.start(() => baroValue(stock, snapshot, null));
  });
  let rows = $derived(valueResult.value?.rows ?? []);
  // Everything the user could feed the kiosk, and what that would yield. This
  // is a POTENTIAL, not a balance.
  let candidates = $state<ScrapCandidate[]>([]);
  let scrapPlan = $state<DucatPlan | null>(null);
  let scrapError = $state('');
  let scrapLoading = $state(false);
  const basketTarget = $derived(ducatBasket(rows, 0).needed);
  $effect(() => {
    const inventory = owned;
    const available = availability;
    const snapshot = market;
    const target = basketTarget;
    let active = true;
    candidates = [];
    scrapPlan = null;
    scrapError = '';
    scrapLoading = !!inventory && !quantitiesUnavailable;
    if (quantitiesUnavailable) return () => { active = false; };
    // The target comes from the stock valuation; a plan made before it arrives is thrown away.
    if (valueResult.phase === 'loading') return () => { active = false; };
    if (inventory) ducatPlan(inventory, snapshot, target, 15, available).then((result) => {
      if (active) { candidates = result.candidates; scrapPlan = target > 0 ? result.plan : null; scrapLoading = false; }
    }).catch(() => { if (active) { scrapError = t('baro.scrapUnavailable'); scrapLoading = false; } });
    return () => { active = false; };
  });
  let scrapPotential = $derived(candidates.reduce((sum, c) => sum + c.totalDucats, 0));
  let basket = $derived(ducatBasket(rows, scrapPotential));
  // What to scrap to pay for the basket. Sized against the basket itself, not
  // against a shortfall computed from a balance we do not have.


  // Skip and unpriced rows are collapsed by default. They are not hidden -
  // "he is selling it and it isn't worth your ducats" is information - but
  // they should not be the first thing in the list.
  let shown = $derived(
    showAll ? rows : rows.filter((r) => r.verdict === 'flip' || r.verdict === 'hold' || r.verdict === 'thin'),
  );
  let hiddenCount = $derived(rows.length - shown.length);

  const verdictLabel = (verdict: BaroVerdict) => t(`baro.verdict.${verdict}`);
  const verdictHint = (verdict: BaroVerdict) => t(`baro.hint.${verdict}`);
  const count = (n: number) => n.toLocaleString(formatLocale());

  function ratio(n: number | null): string {
    return n == null ? '-' : n.toLocaleString(formatLocale(), { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  }
  function credits(n: number | null | undefined): string {
    if (!n) return '-';
    return n >= 999_500
      ? t('baro.millions', { n: Number((n / 1_000_000).toFixed(1)).toLocaleString(formatLocale()) })
      : t('baro.thousands', { n: count(Math.round(n / 1000)) });
  }
</script>

<section class="wrap tw board">
  <div class="rail"><h3>{t('baro.selling')}</h3></div>
  <div class="board-body">
  {#if valueResult.phase === 'loading'}<p role="status">{t('baro.pricing')}</p>{/if}
  {#if valueResult.phase === 'error'}<p class="ui-notice" data-tone="bad" role="alert">{t('baro.pricingUnavailable')}</p>{/if}
  {#if unavailableItems}<p class="ui-notice" data-tone="warn">{t('baro.quantitiesUnknown', { count: unavailableItems })} {t(quantitiesUnavailable ? 'baro.recheck' : 'baro.knownOnly')}</p>{/if}
  {#if scrapLoading}<p role="status">{t('baro.calculating')}</p>{/if}
  {#if scrapError}<p class="ui-notice" data-tone="bad" role="alert">{scrapError}</p>{/if}
  {#if valueResult.phase === 'done'}
  <header class="board-head">
    <p class="sub">
      {t('baro.ranked', { count: rows.length })}
      {#if !current}
        <span class="stale">{t('baro.stale')}</span>
      {/if}
    </p>
  </header>
  {/if}

  {#if basket.needed > 0}
    {@const scrapKnown = owned && !quantitiesUnavailable && !scrapLoading && !scrapError}
    {#if scrapKnown}
      <div class="budget">
        <Meter
          label={t(unavailableItems ? 'baro.knownSpares' : 'baro.yourSpares')}
          value={scrapPotential}
          max={basket.needed}
          text={t('baro.of', { have: count(scrapPotential), need: count(basket.needed) })}
        />
      </div>
    {/if}
    <p class="gap">
      <Rich text={t('baro.basket', { count: basket.count, needed: count(basket.needed), resale: plat(basket.resale) })} />
      {#if scrapKnown}<Rich text={t('baro.covered', { count: basket.count, covered: basket.coveredByScrapping })} />{/if}
    </p>
    {#if owned}
      <p class="sub">
        {t('baro.noBalance')}
      </p>
    {/if}
  {/if}

  {#if valueResult.phase === 'done'}
  <div class="scroll" use:fitTable>
    <table>
      <thead>
        <tr>
          <th scope="col">{t('baro.col.item')}</th>
          <th scope="col" class="num">{t('baro.col.ducats')}</th>
          <th scope="col" class="num">{t('baro.col.credits')}</th>
          <th scope="col" class="num">{t('baro.col.now')}</th>
          <th scope="col" class="num">{t('baro.col.baseline')}</th>
          <th scope="col" class="num">{t('baro.col.perDucat')}</th>
          <th scope="col">{t('baro.col.verdict')}</th>
        </tr>
      </thead>
      <tbody>
        {#each shown as row (row.unique ?? row.item)}
          <tr class:top={row.verdict === 'flip'}>
            <th scope="row" class="name">
              <span class="name-in">
                <Glyph name={glyphFor(market?.path_to_info?.[row.unique ?? '']?.category)} />
                {row.item}
              </span>
            </th>
            <td class="num ducat">{row.ducats == null ? '-' : count(row.ducats)}</td>
            <td class="num muted">{credits(row.credits)}</td>
            <td class="num">{row.price == null ? '-' : `${plat(row.price, { useGrouping: false })}p`}</td>
            <td class="num muted">{row.baseline == null ? '-' : `${plat(row.baseline, { useGrouping: false })}p`}</td>
            <td class="num">{ratio(row.platPerDucat)}</td>
            <td>
              <span class="tag {row.verdict}" title={verdictHint(row.verdict)}>
                {verdictLabel(row.verdict)}<span class="visually-hidden">: {verdictHint(row.verdict)}</span>
              </span>
            </td>
          </tr>
        {/each}
        {#if shown.length === 0 && valueResult.phase === 'done'}<tr><td colspan="7" class="empty">{t('baro.emptyFilter')}</td></tr>{/if}
      </tbody>
    </table>
  </div>
  {/if}

  {#if scrapPlan && scrapPlan.picks.length}
    <details class="scrap">
      <summary>{t('baro.scrapFor', { ducats: count(basket.needed) })}</summary>
      <p class="sub">
        {t('baro.scrapHelp')}
      </p>
      <ul class="picks">
        {#each scrapPlan.picks as pick (pick.slug)}
          <li>
            <span class="pick-n">{pick.spare}×</span>
            <span class="pick-name">{pick.name}</span>
            <span class="pick-d">{pick.totalDucats}d</span>
            <span class="pick-p">{pick.totalPlat > 0 ? t('baro.givenUp', { price: plat(pick.totalPlat) }) : t('baro.unsold')}</span>
          </li>
        {/each}
      </ul>
      <p class="sub">
        <Rich text={t('baro.planTotal', { ducats: count(scrapPlan.ducats), price: plat(scrapPlan.platGivenUp) })} />
        {#if scrapPlan.short > 0}
          <span class="short">{t('baro.short', { ducats: count(scrapPlan.short) })}</span>
        {/if}
      </p>
      {#if scrapPlan.heldBack.length}
        <p class="sub">
          {t('baro.heldBack', { items: scrapPlan.heldBack
            .slice(0, 4)
            .map((h) => `${h.name} (${h.ducats}d / ${plat(h.plat)}p)`)
            .join(', ') })}
        </p>
      {/if}
    </details>
  {/if}

  {#if hiddenCount > 0 || showAll}
    <button type="button" class="more" aria-expanded={showAll} onclick={() => (showAll = !showAll)}>
      {showAll ? t('baro.hide') : t('baro.showMore', { count: hiddenCount })}
    </button>
  {/if}
  </div>
</section>

<style>
  .board {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }
  .sub,
  .gap {
    margin: 0;
    font-size: 0.9rem;
    color: var(--muted);
  }
  .gap :global(b),
  .sub :global(b) {
    color: var(--fg);
  }
  .budget {
    max-width: 32rem;
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
  /* The row header stays a table cell: as a flex box it left the row's
     divider and drew its own, offset from the other cells. Item names are
     reading text, not labels, so they drop the header face and tracking. */
  .name {
    font-family: var(--font-body);
    font-weight: 500;
    letter-spacing: normal;
    color: var(--fg);
  }
  .name-in {
    display: inline-flex;
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
    margin-inline-start: 0;
    /* The verdict is also the row's only opinion, so it gets a cursor hint
       that there is a reason behind it. */
    cursor: help;
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
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .board-body { padding: var(--s4) var(--inset); display: flex; flex-direction: column; gap: var(--s3); }
  .board { gap: 0; }
  .empty { padding: var(--s4); color: var(--muted); }
</style>
