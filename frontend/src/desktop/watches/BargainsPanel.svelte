<script lang="ts">
  import { onDestroy } from 'svelte';
  import { fitTable } from '../../ui/table-fit';
  import { busy } from '../../ui/keyboard-actions';
  import { plat, wfmItemUrl } from '../../ui/format';
  import { t } from '../../i18n/i18n.svelte';
  import type { Market } from '../../contracts/data';
  import { findBargains, liveBargain, type Bargain } from '../../domain/market-bargains';
  import { LiveTopController } from '../selling/live-top.svelte';
  import { useDesktopServices } from '../shell/desktop-context';

  let { market = null }: { market?: Market | null } = $props();

  const live = new LiveTopController(useDesktopServices());
  onDestroy(() => live.dispose());

  let rows = $derived(findBargains(market));
  // A new snapshot is a new list; quotes for the old one would mislabel it.
  $effect(() => {
    void market;
    live.reset();
  });

  function check(): void {
    void live.check(rows.map((b) => ({ slug: b.slug })));
  }

  function liveCell(b: Bargain): { text: string; cls: string } {
    const state = liveBargain(b, live.get({ slug: b.slug }));
    if (!state) return { text: t('bargains.notChecked'), cls: 'muted' };
    if (state.state === 'failed') return { text: t('bargains.checkFailed'), cls: 'muted' };
    if (state.state === 'gone') {
      return { text: state.ask == null ? t('bargains.goneNoAsk') : t('bargains.gone', { price: plat(state.ask) }), cls: 'muted' };
    }
    return { text: state.seller ? t('bargains.holdsBy', { price: plat(state.ask), seller: state.seller }) : t('bargains.holds', { price: plat(state.ask) }), cls: 'good' };
  }
</script>

<section class="wrap tw bargains" aria-labelledby="bargains-title">
  <div class="rail">
    <h3 id="bargains-title">{t('bargains.title')}</h3>
    <span class="exp">{t('bargains.lede')}</span>
    {#if rows.length}
      <button type="button" class="btn xs" onclick={check} use:busy={live.phase === 'running'} title={t('bargains.checkTitle')}>
        {#if live.phase === 'running'}{t('bargains.checking', { done: live.progress.done, total: live.progress.total })}
        {:else if live.phase === 'done'}{t('bargains.recheck')}
        {:else}{t('bargains.check')}{/if}
      </button>
    {/if}
  </div>
  {#if live.phase === 'error'}
    <div class="ui-notice" data-tone="bad" role="alert">{t('bargains.failed', { error: live.error ?? '' })}</div>
  {/if}
  {#if rows.length === 0}
    <div class="ui-notice">{t(market ? 'bargains.none' : 'bargains.noMarket')}</div>
  {:else}
    <div class="scroll" use:fitTable>
      <table class="tw">
        <thead>
          <tr>
            <th class="l">{t('bargains.col.item')}</th>
            <th>{t('bargains.col.ask')}</th>
            <th>{t('bargains.col.resale')}</th>
            <th>{t('bargains.col.margin')}</th>
            <th data-drop="1">{t('bargains.col.vol')}</th>
            <th class="l">{t('bargains.col.live')}</th>
          </tr>
        </thead>
        <tbody>
          {#each rows as b (b.slug)}
            {@const cell = liveCell(b)}
            <tr>
              <td class="l">
                <a href={wfmItemUrl(b.slug)} target="_blank" rel="noopener noreferrer">{b.name}</a>
                <span class="tag {b.kind}" title={t(b.kind === 'flip' ? 'bargains.flipWhat' : 'bargains.cheapWhat')}>{t(b.kind === 'flip' ? 'bargains.flip' : 'bargains.cheap')}</span>
              </td>
              <td class="price">{plat(b.ask)}<span class="unit">p</span></td>
              <td class="num" title={t(b.kind === 'flip' ? 'bargains.resaleBid' : 'bargains.resaleUsual')}>{plat(b.resale)}<span class="unit">p</span> <span class="muted">{t(b.kind === 'flip' ? 'bargains.bid' : 'bargains.usual')}</span></td>
              <td class="num">+{plat(b.margin)}<span class="unit">p</span></td>
              <td class="num muted">{b.vol}</td>
              <td class="l {cell.cls}" aria-live="polite">{cell.text}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
  <div class="foot">{t('bargains.foot')}</div>
</section>

<style>
  .scroll { overflow: auto; }
  .num { font-family: var(--font-mono); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .num .muted { font-family: var(--font-ui); }
  .muted { color: var(--muted); }
  .good { color: var(--good); font-weight: 600; }
  .ui-notice { margin: var(--s4) var(--inset); }
</style>
