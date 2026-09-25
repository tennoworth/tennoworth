<script lang="ts">
  // Light / Dark / System - the app's only theme control. (The four-look
  // picker went away in 2026-08: YoRHa is THE theme, so the choice is a mode.)
  // Reads and writes through the ThemeController from src/lib/theme.ts, which
  // owns the <html> attributes and the persisted setting.
  //
  // Two mounts exist and they never coexist: Settings → Appearance in the
  // shell, and a compact one in the hosted site's footer (a visitor who never
  // searches never reaches the shell). So local $state mirrors of the
  // controller's values are enough - no shared store needed.
  import { onMount } from 'svelte';
  import { systemMode, type ModePref, type ThemeController } from './theme';

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
  <div class="ui-segmented" class:xs={compact} role="radiogroup" aria-label={label}>
    {#each MODES as m (m.id)}
      <button
        type="button"
        role="radio"
        aria-checked={modePref === m.id}
        tabindex={modePref === m.id ? 0 : -1}
        onkeydown={(event) => {
          const direction = ['ArrowRight', 'ArrowDown'].includes(event.key) ? 1 : ['ArrowLeft', 'ArrowUp'].includes(event.key) ? -1 : 0;
          if (!direction && event.key !== 'Home' && event.key !== 'End') return;
          event.preventDefault();
          const index = event.key === 'Home' ? 0 : event.key === 'End' ? MODES.length - 1 : (MODES.findIndex(mode => mode.id === m.id) + direction + MODES.length) % MODES.length;
          pickMode(MODES[index].id);
          (event.currentTarget.parentElement?.querySelectorAll('button')[index] as HTMLButtonElement | undefined)?.focus();
        }}
        onclick={() => pickMode(m.id)}
      >{m.label}</button>
    {/each}
  </div>
  {#if !compact && modePref === 'system'}
    <span class="hint">currently {sysDark ? 'dark' : 'light'}</span>
  {/if}
</div>

<style>
  .theme-switcher {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--s1) var(--s3);
    font-size: var(--text-caption);
  }
  .hint { color: var(--muted); font-size: var(--text-caption); line-height: 1rem; }
</style>
