<script lang="ts">
  import { useDesktopServices } from '../../ui/desktop-context';
  const { listenForTauriEvent, currentOverlayResult } = useDesktopServices();
  import { onMount } from 'svelte';
  
  
  import type { RelicOverlayResult } from '../../contracts/data';

  let result = $state<RelicOverlayResult | null>(null);

  onMount(() => {
    window.__TENNOWORTH_RELIC_OVERLAY_UPDATE__ = (next) => { result = next; };
    window.__TENNOWORTH_RELIC_OVERLAY_HIDE__ = () => { result = null; };
    currentOverlayResult()
      .then((current) => { if (current) result = current; })
      .catch(() => {});
    const unlistenUpdate = listenForTauriEvent<RelicOverlayResult>('relic-overlay:update', (payload) => {
      result = payload;
    });
    const unlistenHide = listenForTauriEvent('relic-overlay:hide', () => {
      result = null;
    });
    return () => {
      unlistenUpdate();
      unlistenHide();
      delete window.__TENNOWORTH_RELIC_OVERLAY_UPDATE__;
      delete window.__TENNOWORTH_RELIC_OVERLAY_HIDE__;
    };
  });

  const price = (slot: RelicOverlayResult['slots'][number]) =>
    slot.livePlatinum ?? slot.cachedPlatinum;
</script>

<svelte:head><title>TennoWorth relic overlay</title></svelte:head>

<div class="overlay" aria-live="polite">
  {#if result}
    {#each result.slots as slot (slot.index)}
      <article
        class="reward"
        class:best={slot.bestPlatinum}
        class:uncertain={slot.confidence < 0.9}
        style:left={`${slot.box.x * 100}%`}
        style:top={`${slot.box.y * 100}%`}
        style:width={`${slot.box.width * 100}%`}
        style:--reward-scale={result.scale}
      >
        <div class="inner" style:transform={`scale(${result.scale})`}>
          {#if slot.bestPlatinum}<span class="best-flag">BEST PLAT</span>{/if}
          {#if slot.bestDucats && !slot.bestPlatinum}<span class="ducat-flag">BEST DUCATS</span>{/if}
          <strong class="name">{slot.name ?? slot.rawText}</strong>
          <div class="facts">
            <b class="plat">{price(slot) == null ? '-' : `${price(slot)}p`}</b>
            <span>{slot.ducats == null ? '-d' : `${slot.ducats}d`}</span>
            <span>{slot.owned == null ? 'own -' : `own ${slot.owned}`}</span>
            <span class="source">{slot.livePlatinum == null ? 'cached' : 'live'}</span>
          </div>
          {#if slot.confidence < 0.9}
            <span class="confidence">check name · {Math.round(slot.confidence * 100)}%</span>
          {/if}
        </div>
      </article>
    {/each}
  {/if}
</div>

<style>
  :global(html.relic-overlay-surface), :global(html.relic-overlay-surface body), :global(html.relic-overlay-surface #app) { width: 100%; height: 100%; margin: 0; background: transparent !important; overflow: hidden; }
  .overlay { position: fixed; inset: 0; pointer-events: none; font-family: var(--font-body); color: var(--reward-fg); }
  .reward { position: absolute; box-sizing: border-box; padding: 0 var(--s2); transform: translateY(var(--s2)); }
  .inner { position: relative; width: calc(100% / var(--reward-scale, 1)); max-width: 15.625rem; min-height: 4rem; margin: auto; padding: var(--s2) var(--s3); border: 1px solid var(--reward-border); border-radius: var(--radius-panel); background: var(--reward-surface); transform-origin: top center; }
  .reward.best .inner { border-color: var(--reward-value); }
  .reward.uncertain .inner { border-color: var(--reward-warning); }
  .best-flag, .ducat-flag { display: inline-block; margin-bottom: var(--s1); padding: 0 var(--s1); background: var(--reward-value); color: var(--reward-on-value); font: 600 var(--text-caption)/var(--leading-body) var(--font-ui); letter-spacing: .08em; }
  .ducat-flag { background: var(--reward-good); }
  .name { display: block; white-space: normal; overflow-wrap: anywhere; font-size: var(--text-caption); }
  .facts { display: flex; flex-wrap: wrap; align-items: baseline; gap: var(--s1) var(--s2); color: var(--reward-muted); font: var(--text-caption)/var(--leading-body) var(--font-mono); }
  .plat { color: var(--reward-value); font-size: var(--text-metric-lg); }
  .source { color: var(--reward-good); margin-left: auto; }
  .confidence { display: block; color: var(--reward-warning); font: var(--text-caption)/var(--leading-control) var(--font-mono); }
</style>
