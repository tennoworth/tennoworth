<script lang="ts">
  import { onMount } from 'svelte';
  import { useDesktopServices } from '../shell/desktop-context';
  import { compareBuyers } from '../../domain/buyer-alternatives';
  import { humanError } from '../../contracts/errors';
  import type { LiveTop } from '../../contracts/desktop';
  import { wfmItemUrl } from '../../ui/format';
  import { busy } from '../../ui/keyboard-actions';
  import { formatLocale, t } from '../../i18n/i18n.svelte';

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
    return () => { disposed = true; };
  });
  // `now` only matters where the comparison's freshness flips: when the
  // observation time is reached (clock skew) and 60s after it. One timer per
  // boundary replaces a once-a-second tick that ran even in hidden views; the
  // write lands in the timer callback, outside this effect's tracking.
  $effect(() => {
    const observed = Date.parse(quote?.buyer_book?.observed_at ?? '');
    const at = now;
    if (!Number.isFinite(observed)) return;
    const next = at < observed ? observed : observed + 60_000;
    if (next <= at) return;
    const timer = setTimeout(() => { now = Date.now(); }, next - at);
    return () => clearTimeout(timer);
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions (Escape closes the comparison from any control inside it.) -->
<section class="ui-panel ui-stack buyer-alternatives" aria-label={t('buyers.panel')}
  onkeydown={(event) => { if (event.key === 'Escape' && !event.defaultPrevented) { event.preventDefault(); onclose(); } }}>
  <div class="ui-toolbar">
    <h3 tabindex="-1">{t('buyers.title', { item: name, quantity })}</h3>
    <button class="btn" onclick={refresh} use:busy={loading}>{t(loading ? 'buyers.checking' : 'buyers.refresh')}</button>
    <button class="btn ghost" onclick={onclose}>{t('buyers.close')}</button>
  </div>
  <p>{t('buyers.intro', { ask })}</p>
  <p class="muted">{t('buyers.sample')}</p>
  {#if error}
    <p class="ui-notice" data-tone="bad" role="alert">{error}</p>
  {:else if !loading && comparison.error}
    <p class="ui-notice" data-tone="warn" role="status">{t(`buyers.error.${comparison.error}`)}</p>
  {:else if !loading && comparison.error === null}
    <p class="ui-notice" role="status">{t('buyers.coverage', { covered: comparison.covered, quantity, value: comparison.value, uncovered: comparison.uncovered })}</p>
    <p>{t(comparison.gap >= 0 ? 'buyers.gapLess' : 'buyers.gapMore', { covered: comparison.covered, ask: comparison.askValue, gap: Math.abs(comparison.gap) })}</p>
    <p class="muted">{t('buyers.checked', { time: new Date(comparison.observed ?? '').toLocaleTimeString(formatLocale()) })}</p>
    {#if comparison.rows.length}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex (Keyboard focus lets users scroll every buyer column.) -->
      <div class="wrap tw" role="region" aria-label={t('buyers.tableLabel')} tabindex="0">
        <table class="tw">
          <thead><tr><th class="l">{t('buyers.col.buyer')}</th><th>{t('buyers.col.lot')}</th><th>{t('buyers.col.covered')}</th><th>{t('buyers.col.value')}</th><th class="l">{t('buyers.col.status')}</th></tr></thead>
          <tbody>{#each comparison.rows as row (row.order.id)}
            <tr><td class="l"><a href={`https://warframe.market/profile/${encodeURIComponent(row.order.user_slug)}`} target="_blank" rel="noopener noreferrer">{row.order.name}</a></td>
              <td>{row.order.per_trade}</td><td>{row.quantity}</td><td>{row.value}p</td><td class="l">{row.order.status} · {row.order.platform}</td></tr>
          {/each}</tbody>
        </table>
      </div>
    {/if}
  {/if}
  <a class="btn ghost" href={wfmItemUrl(slug)} target="_blank" rel="noopener noreferrer">{t('buyers.openOnWfm')}</a>
</section>

<style>
  h3 { margin: 0; font-size: var(--text-section); scroll-margin: var(--s4); }
  h3:focus { outline: none; }
  p { margin: 0; }
  table { min-width: 38rem; }
</style>
