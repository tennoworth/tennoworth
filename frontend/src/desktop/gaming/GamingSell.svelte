<script lang="ts">
  import { formatLocale, t } from '../../i18n/i18n.svelte';
  import { selectPicks } from '../../domain/sell-priority';
  import { pickReason } from '../selling/pick-reason';
  import type { SellRow } from '../../contracts/selling';
  import type { TradeAllowance } from '../../contracts/data';

  let { results, ready, hasInventory, canList, blockReason, actionLabel, tradeAllowance, onlist, oncheck }: {
    results: SellRow[];
    ready: boolean;
    hasInventory: boolean;
    canList: boolean;
    /** Why listing waits (no scan, signed out, quantities unchecked), and the
     *  button that clears it; the same ones the desktop layout shows. */
    blockReason: string | null;
    actionLabel: string | null;
    tradeAllowance: TradeAllowance | null;
    onlist: (rows?: SellRow | SellRow[]) => void;
    oncheck: () => void;
  } = $props();

  let picks = $derived(selectPicks(results));
  const price = (p: SellRow) => Math.round(p.clearing_price ?? p.low_sell);
  let value = $derived(picks.reduce((sum, p) => sum + price(p) * p.sellable, 0));
  let tradesLeft = $derived(tradeAllowance?.remaining ?? null);

  export function secondary() {
    if (!canList) oncheck();
    else if (ready && picks.length) onlist(picks);
  }
</script>

<section class="gm-section" aria-labelledby="gm-sell-title">
  <header class="gm-head">
    <h2 id="gm-sell-title">{t('gaming.sell')}</h2>
    {#if ready && picks.length}
      <p class="muted">{t('gaming.sellSummary', { count: picks.length, value: value.toLocaleString(formatLocale()) })}{#if tradesLeft != null}{' · '}{t('sell.tradesLeft', { count: tradesLeft })}{/if}</p>
      {#if canList}<button type="button" class="gm-action" onclick={secondary}><kbd class="gm-key">X</kbd>{t('gaming.listAll')}</button>{/if}
    {/if}
  </header>
  {#if hasInventory && !canList}
    <div class="gm-notice" role="status">
      <p>{blockReason}</p>
      <button type="button" class="gm-action primary" data-pad-default onclick={oncheck}><kbd class="gm-key">X</kbd>{actionLabel ?? t('sell.checkListings')}</button>
    </div>
  {/if}
  {#if !hasInventory}
    <div class="gm-empty"><strong>{t('gaming.needScanTitle')}</strong><p>{t('gaming.needScanBody')}</p></div>
  {:else if !ready}
    <div class="gm-empty"><p>{t('gaming.calculating')}</p></div>
  {:else if picks.length === 0}
    <div class="gm-empty"><strong>{t('gaming.noPicksTitle')}</strong><p>{t('gaming.noPicksBody')}</p></div>
  {:else}
    <ul class="gm-list" data-pad-scroll>
      {#each picks as p, index (p.key ?? p.slug)}
        <li>
          <button type="button" class="gm-row" data-pad-default={index === 0 && canList ? '' : undefined} disabled={!canList} onclick={() => onlist(p)} aria-label={t('sell.listItem', { item: p.name })}>
            <span class="gm-main">
              <span class="gm-name">{p.name}</span>
              <span class="gm-sub">{pickReason(p)}</span>
            </span>
            <span class="gm-figs">
              <span class="gm-big mono">{(price(p) * p.sellable).toLocaleString(formatLocale())}<span class="unit">p</span></span>
              <span class="gm-sub mono">{p.sellable} × {t('gaming.each', { price: price(p) })}</span>
            </span>
            <span class="gm-verb">{t('gaming.list')}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</section>
