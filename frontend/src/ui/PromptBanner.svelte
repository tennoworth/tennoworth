<script lang="ts">
  import { onMount, type Snippet } from 'svelte';
  import type { PromptSession } from './prompt-session.svelte';
  // An optional invitation that is safe to ignore. The owner mounts it only
  // while it applies; the session decides whether this launch asks it.
  let { session, id, title, dismissLabel = 'Not now', children, actions }: {
    session: PromptSession;
    id: string;
    title: string;
    dismissLabel?: string;
    children: Snippet;
    actions?: Snippet;
  } = $props();
  onMount(() => { session.claim(id); });
  let shown = $derived(session.current === id && !session.hidden);
</script>

{#if shown}
  <section class="card ui-panel general-banner" aria-label={title} data-prompt={id}>
    <div class="gb-body ui-stack">
      <strong>{title}</strong>
      {@render children()}
    </div>
    <div class="gb-actions">
      {@render actions?.()}
      <button type="button" class="btn ghost" onclick={() => session.decline()}>{dismissLabel}</button>
    </div>
  </section>
{/if}

<style>
  .gb-body :global(p) { margin: 0; line-height: var(--leading-body); }
  @media (max-width: 35rem) {
    .general-banner .gb-actions { width: 100%; justify-content: flex-end; }
  }
</style>
