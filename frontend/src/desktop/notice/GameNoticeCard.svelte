<script lang="ts">
  // The webview in-game notice (Windows, X11, and Wayland without layer-shell).
  // The native Wayland renderer draws the same card in `wayland.rs`.
  import { onMount } from 'svelte';
  import { useDesktopServices } from '../shell/desktop-context';
  import type { GameNotice } from '../../contracts/desktop';
  const { currentGameNotice } = useDesktopServices();

  let notice = $state<GameNotice | null>(null);

  onMount(() => {
    let pushed = false;
    let disposed = false;
    window.__TENNOWORTH_GAME_NOTICE__ = (next) => { pushed = true; notice = next; };
    // A cold window misses the first push; ask once, unless a push already won.
    currentGameNotice()
      .then((current) => { if (!disposed && !pushed) notice = current; })
      .catch(() => {});
    return () => {
      disposed = true;
      delete window.__TENNOWORTH_GAME_NOTICE__;
    };
  });
</script>

<svelte:head><title>TennoWorth in-game notice</title></svelte:head>

<div class="surface" aria-live="polite">
  {#if notice}
    <article class="card">
      <span class="title">{notice.title}</span>
      <strong class="detail">{notice.detail}</strong>
      {#if notice.hint}<span class="hint">{notice.hint}</span>{/if}
    </article>
  {/if}
</div>

<style>
  :global(html.game-notice-surface), :global(html.game-notice-surface body), :global(html.game-notice-surface #app) { width: 100%; height: 100%; margin: 0; background: transparent !important; overflow: hidden; }
  .surface { position: fixed; inset: 0; pointer-events: none; color: var(--reward-fg); font-family: var(--font-body); }
  /* Text stays on one line, as the native card does: the window is sized before the text is known. */
  .card { box-sizing: border-box; display: flex; flex-direction: column; justify-content: center; gap: var(--s1); width: 100%; height: 100%; padding: var(--s2) var(--s4); border: 1px solid var(--reward-border); border-left: 3px solid var(--reward-value); border-radius: var(--radius-panel); background: var(--reward-surface); }
  .card > * { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .title { color: var(--reward-muted); font: 600 var(--text-caption)/1.2 var(--font-ui); letter-spacing: .08em; text-transform: uppercase; }
  .detail { font-size: var(--text-body); font-weight: 600; line-height: 1.25; }
  .hint { color: var(--reward-value); font-size: var(--text-control); line-height: 1.25; }
</style>
