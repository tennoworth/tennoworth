<script lang="ts">
  // Light / Dark / System — the app's only theme control. (The four-look
  // picker went away in 2026-08: YoRHa is THE theme, so the choice is a mode.)
  // Reads and writes through the ThemeController from src/lib/theme.ts, which
  // owns the <html> attributes and the persisted setting.
  //
  // Two mounts exist and they never coexist: Settings → Appearance in the
  // shell, and a compact one in the hosted site's footer (a visitor who never
  // searches never reaches the shell). So local $state mirrors of the
  // controller's values are enough — no shared store needed.
  import { onMount } from 'svelte';
  import { systemMode, type ModePref, type ThemeController } from '../lib/theme';

  interface Props {
    theme: ThemeController;
    /** Footer variant: smaller, quieter, no resolved-mode hint. */
    compact?: boolean;
    /** Accessible name for the group; distinguishes the two mounts. */
    label?: string;
  }
  let { theme, compact = false, label = 'Colour mode' }: Props = $props();

  // Snapshot on init is intended: `theme` is a plain controller (not
  // reactive), and this component is the only writer while it is mounted.
  // svelte-ignore state_referenced_locally
  let modePref = $state<ModePref>(theme.modePref);
  // Tracked so the "System → currently …" hint flips when the OS scheme does.
  let sysDark = $state(systemMode() === 'dark');

  onMount(() => {
    const mq = matchMedia('(prefers-color-scheme: dark)');
    const onChange = () => (sysDark = mq.matches);
    mq.addEventListener('change', onChange);
    return () => mq.removeEventListener('change', onChange);
  });

  const MODES: { id: ModePref; label: string }[] = [
    { id: 'light', label: 'Light' },
    { id: 'dark', label: 'Dark' },
    { id: 'system', label: 'System' },
  ];

  function pickMode(id: ModePref) {
    modePref = id;
    theme.setModePref(id);
  }
</script>

<div class="theme-switcher" class:compact>
  <div class="segmented" role="radiogroup" aria-label={label}>
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
  {#if !compact && modePref === 'system'}
    <span class="hint">currently {sysDark ? 'dark' : 'light'}</span>
  {/if}
</div>

<style>
  /* .segmented / .seg-btn are shared class names: app.css carries the yorha
     structural rules for them (dotted divider, inverted active). */
  .theme-switcher {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--s1) var(--s3);
    font-size: 11px;
  }
  .segmented {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: var(--radius-ctl);
    overflow: hidden;
  }
  .seg-btn {
    font: inherit;
    font-size: 12px;
    background: var(--panel-2);
    border: none;
    border-left: 1px var(--rule) var(--border);
    color: var(--muted);
    padding: 0 var(--s3);
    height: var(--ctl);
    cursor: pointer;
    border-radius: 0;
  }
  .seg-btn:first-child { border-left: none; }
  .seg-btn:hover { color: var(--fg); background: var(--hover); }
  .seg-btn.active { color: var(--accent); background: var(--panel); font-weight: 600; }
  .hint { color: var(--muted); font-size: 11px; line-height: 1rem; }

  /* Footer variant: matches the site footer's 11px type scale and stays quiet
     — it is a convenience for a visitor who never reaches Settings, not a
     piece of chrome that should compete with the footer's links. */
  .compact .segmented { border-color: var(--hairline); }
  .compact .seg-btn {
    font-size: 11px;
    line-height: 1rem;
    height: var(--ctl-xs);
    padding: 0 var(--s2);
    background: transparent;
  }
  .compact .seg-btn.active { color: var(--fg); background: var(--panel-2); font-weight: 600; }
</style>
