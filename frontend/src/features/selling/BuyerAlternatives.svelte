<script lang="ts">
  import { onMount } from 'svelte';
  import { useDesktopServices } from '../../ui/desktop-context';
  import { compareBuyers } from '../../domain/buyer-alternatives';
  import { humanError } from '../../contracts/errors';
  import type { LiveTop } from '../../contracts/desktop';
  import { wfmItemUrl } from '../../ui/format';

  let { slug, name, quantity, ask, onclose }: {
    slug: string; name: string; quantity: number; ask: number; onclose(): void;
  } = $props();
  const { desktopLiveTopPrices } = useDesktopServices();
  let quote = $state<LiveTop | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let now = $state(Date.now());
  let disposed = false;
  let request = 0;
  let comparison = $derived(compareBuyers(quote, quantity, ask, now));

  async function refresh() {
    const current = ++request;
    loading = true;
    error = null;
    try {
      const quotes = await desktopLiveTopPrices([{ slug, rank: 0, subtype: null }]);
      if (disposed || current !== request) return;
      quote = quotes.find(row => row.slug === slug && (row.rank ?? 0) === 0 && row.subtype == null) ?? null;
      now = Date.now();
      error = quote?.error ?? null;
    } catch (e) {
      if (!disposed && current === request) { quote = null; error = humanError(e); }
    } finally { if (!disposed && current === request) loading = false; }
  }
  onMount(() => {
    void refresh();
    const timer = setInterval(() => { now = Date.now(); }, 1000);
    return () => { disposed = true; clearInterval(timer); };
  });
</script>

<section class="ui-panel ui-stack" aria-label="Buyer alternatives">
  <div class="ui-toolbar">
    <h3>Buyers for {name} ×{quantity}</h3>
    <button class="btn" onclick={refresh} disabled={loading}>{loading ? 'Checking buyers…' : 'Refresh buyers'}</button>
    <button class="btn ghost" onclick={onclose}>Close buyer comparison</button>
  </div>
  <p>Compare visible bids with the selected reference of {ask}p per unit. Rank 0 where applicable.</p>
  <p class="muted">Top-five sample, not the full order book. Whole lots only; this comparison fills higher unit bids first and may leave units uncovered. Offers do not guarantee a sale or response.</p>
  {#if error}
    <p class="ui-notice" data-tone="bad" role="alert">{error}</p>
  {:else if !loading && comparison.error}
    <p class="ui-notice" data-tone="warn" role="status">{comparison.error}</p>
  {:else if !loading && comparison.error === null}
    <p class="ui-notice" role="status">{comparison.covered} of {quantity} units covered · {comparison.value}p visible bid value · {comparison.uncovered} {comparison.uncovered === 1 ? 'unit' : 'units'} without observed coverage.</p>
    <p>For those same {comparison.covered} covered units: {comparison.askValue}p at your listing reference;
      {Math.abs(comparison.gap)}p {comparison.gap >= 0 ? 'less at these bids' : 'more at these bids'}.</p>
    <p class="muted">Checked {new Date(comparison.observed ?? '').toLocaleTimeString()}. Your own orders are excluded.</p>
    {#if comparison.rows.length}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex (Keyboard focus lets users scroll every buyer column.) -->
      <div class="wrap tw" role="region" aria-label="Compatible buyer quantities" tabindex="0">
        <table class="tw">
          <thead><tr><th class="l">Buyer</th><th>Units / lot</th><th>Units covered</th><th>Bid value</th><th class="l">Status / platform</th></tr></thead>
          <tbody>{#each comparison.rows as row (row.order.id)}
            <tr><td class="l"><a href={`https://warframe.market/profile/${encodeURIComponent(row.order.user_slug)}`} target="_blank" rel="noopener noreferrer">{row.order.name}</a></td>
              <td>{row.order.per_trade}</td><td>{row.quantity}</td><td>{row.value}p</td><td class="l">{row.order.status} · {row.order.platform}</td></tr>
          {/each}</tbody>
        </table>
      </div>
    {/if}
  {/if}
  <a class="btn ghost" href={wfmItemUrl(slug)} target="_blank" rel="noopener noreferrer">Open item on WFM</a>
</section>

<style>
  h3 { margin: 0; font-size: var(--text-section); }
  p { margin: 0; }
  table { min-width: 38rem; }
</style>
