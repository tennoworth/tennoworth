<script lang="ts">
  // Look picker (live swatch per look) + Light/Dark/System mode control. Reads
  // and writes through the ThemeController from src/lib/theme.ts, which owns
  // the <html> attributes and the persisted settings. Only one instance is
  // ever mounted at a time (landing OR shell), so local $state mirrors of the
  // controller's values are enough — no shared store needed.
  import { onMount } from 'svelte';
  import { LOOKS, systemMode, type Look, type ModePref, type ThemeController } from '../lib/theme';

  let { theme }: { theme: ThemeController } = $props();

  // Snapshot on init is intended: `theme` is a plain controller (not
  // reactive), and this component is the only writer while it is mounted.
  // svelte-ignore state_referenced_locally
  let look = $state<Look>(theme.look);
  // svelte-ignore state_referenced_locally
  let modePref = $state<ModePref>(theme.modePref);
  // Tracked so the "no dark variant" note flips when the OS scheme does while
  // the pref is 'system'.
  let sysDark = $state(systemMode() === 'dark');

  onMount(() => {
    const mq = matchMedia('(prefers-color-scheme: dark)');
    const onChange = () => (sysDark = mq.matches);
    mq.addEventListener('change', onChange);
    return () => mq.removeEventListener('change', onChange);
  });

  const resolved = $derived(modePref === 'system' ? (sysDark ? 'dark' : 'light') : modePref);
  const info = $derived(LOOKS.find((l) => l.id === look) ?? LOOKS[0]);
  // A look without a token block for the resolved mode keeps its other block;
  // say so instead of letting a "Dark" pin look broken.
  const fallbackNote = $derived(
    info.modes.includes(resolved)
      ? null
      : `${info.label} has no ${resolved} variant yet — showing its ${info.modes[0]} tokens.`,
  );

  const MODES: { id: ModePref; label: string }[] = [
    { id: 'light', label: 'Light' },
    { id: 'dark', label: 'Dark' },
    { id: 'system', label: 'System' },
  ];

  function pickLook(id: Look) {
    look = id;
    theme.setLook(id);
  }
  function pickMode(id: ModePref) {
    modePref = id;
    theme.setModePref(id);
  }
</script>

<div class="theme-switcher">
  <div class="looks" role="radiogroup" aria-label="Look">
    {#each LOOKS as l (l.id)}
      <button
        type="button"
        class="theme-swatch"
        class:on={look === l.id}
        data-swatch={l.id}
        role="radio"
        aria-checked={look === l.id}
        title="{l.label} — {l.blurb}"
        onclick={() => pickLook(l.id)}
      >
        <span class="tile" aria-hidden="true">
          <span class="bar"></span>
          <span class="sample">Aa <b>123</b></span>
          <span class="acc"></span>
        </span>
        <span class="name">{l.label}</span>
      </button>
    {/each}
  </div>
  <div class="segmented" role="radiogroup" aria-label="Colour mode">
    {#each MODES as m (m.id)}
      <button
        type="button"
        class="seg-btn"
        class:active={modePref === m.id}
        role="radio"
        aria-checked={modePref === m.id}
        onclick={() => pickMode(m.id)}
      >{m.label}</button>
    {/each}
  </div>
  {#if fallbackNote}
    <p class="note">{fallbackNote}</p>
  {/if}
</div>

<style>
  .theme-switcher {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: 8px 12px;
    font-size: 11px;
  }
  .looks { display: flex; gap: 4px; }
  /* Swatch tiles are painted from per-look literals (--sw-* set in app.css by
     data-swatch) so every tile previews ITS look, not the current one. */
  .theme-swatch {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 3px;
    padding: 2px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-ctl);
    color: var(--muted);
    cursor: pointer;
    font: inherit;
    font-size: 10px;
    line-height: 1.2;
  }
  .theme-swatch:hover { color: var(--fg); background: transparent; }
  .theme-swatch.on { color: var(--fg); border-color: var(--fg); }
  .tile {
    display: flex;
    flex-direction: column;
    width: 40px;
    height: 28px;
    background: var(--sw-panel);
    border: 1px solid var(--sw-border);
    border-radius: var(--radius-tag);
    overflow: hidden;
    position: relative;
  }
  .bar { height: 6px; background: var(--sw-bar); flex: none; }
  .sample {
    flex: 1;
    display: flex;
    align-items: center;
    padding-left: 4px;
    color: var(--sw-fg);
    font-size: 8.5px;
    letter-spacing: 0.02em;
    white-space: nowrap;
  }
  .sample b { font-family: var(--font-mono); font-weight: 600; margin-left: 2px; }
  .acc { position: absolute; right: 3px; bottom: 3px; width: 6px; height: 6px; background: var(--sw-accent); }
  .segmented {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: var(--radius-ctl);
    overflow: hidden;
    align-self: center;
  }
  .seg-btn {
    font: inherit;
    font-size: 11px;
    background: var(--panel-2);
    border: none;
    border-left: 1px var(--rule) var(--border);
    color: var(--muted);
    padding: 3px 9px;
    min-height: 22px;
    cursor: pointer;
    border-radius: 0;
  }
  .seg-btn:first-child { border-left: none; }
  .seg-btn:hover { color: var(--fg); background: var(--hover); }
  .seg-btn.active { color: var(--accent); background: var(--panel); font-weight: 600; }
  .note { flex-basis: 100%; margin: 0; color: var(--muted); font-size: 11px; line-height: 1.4; }
</style>
