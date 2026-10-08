<script lang="ts">
  import { untrack, type Snippet } from 'svelte';
  import type { SettingsStore } from '../contracts/state-store';
  import { dismissPrompt, dismissedPrompts } from './prompts';
  // An optional invitation that is safe to ignore. Turning it down is
  // remembered per id, so the owner only decides whether it currently applies.
  let { id, store, title, dismissLabel = 'Not now', children, actions }: {
    id: string;
    store: SettingsStore;
    title: string;
    dismissLabel?: string;
    children: Snippet;
    /** Receives `dismiss` for actions that should also retire the prompt. */
    actions?: Snippet<[() => void]>;
  } = $props();
  // Read at mount; owners remount the banner rather than swapping its id.
  let dismissed = $state(untrack(() => dismissedPrompts(store).has(id)));
  function dismiss() {
    dismissed = true;
    // A failed write only means the prompt may return next launch.
    void dismissPrompt(store, id).catch(() => {});
  }
</script>

{#if !dismissed}
  <section class="card ui-panel general-banner" aria-label={title} data-prompt={id}>
    <div class="gb-body ui-stack">
      <strong>{title}</strong>
      {@render children()}
    </div>
    <div class="gb-actions">
      {@render actions?.(dismiss)}
      <button type="button" class="btn ghost" onclick={dismiss}>{dismissLabel}</button>
    </div>
  </section>
{/if}

<style>
  .gb-body :global(p) { margin: 0; line-height: var(--leading-body); }
  @media (max-width: 35rem) {
    .general-banner .gb-actions { width: 100%; justify-content: flex-end; }
  }
</style>
