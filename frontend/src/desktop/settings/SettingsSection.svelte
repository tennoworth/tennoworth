<script lang="ts">
  import type { Snippet } from 'svelte';
  import { t } from '../../i18n/i18n.svelte';
  import type { SettingsSectionId, SettingsSections } from './sections.svelte';

  interface Props {
    id: SettingsSectionId;
    title: string;
    /** Absent where a section renders on its own (component tests). */
    sections?: SettingsSections;
    /** Rail actions; they belong to the content, so they fold with it. */
    actions?: Snippet;
    children: Snippet;
  }
  let { id, title, sections, actions, children }: Props = $props();

  let localOpen = $state(true);
  let open = $derived(sections ? sections.isOpen(id) : localOpen);
  function toggle() {
    if (sections) sections.toggle(id);
    else localOpen = !localOpen;
  }
</script>

<section class="wrap tw" id="settings-{id}" tabindex="-1" aria-labelledby="set-{id}">
  <div class="rail">
    <h3 id="set-{id}">{title}</h3>
    {#if open}{@render actions?.()}{/if}
    <span class="grow"></span>
    <button type="button" class="rail-toggle" aria-expanded={open} aria-controls="settings-{id}-body" onclick={toggle}>
      <span class="glyph" aria-hidden="true">{open ? '▾' : '▸'}</span><span>{t(open ? 'settings.hide' : 'settings.show')} <span class="sr-only">{title}</span></span>
    </button>
  </div>
  <!-- Hidden rather than unmounted, so a draft or a pending save survives a fold. -->
  <div class="body" id="settings-{id}-body" hidden={!open}>{@render children()}</div>
</section>

<style>
  section[tabindex="-1"]:focus { outline: none; }
  .grow { flex: 1 1 0; min-width: 0; }
  .rail-toggle {
    display: inline-flex; align-items: center; gap: var(--s1);
    min-height: var(--ctl-xs); min-width: 24px; padding: 0 var(--s1);
    font: 500 var(--text-caption)/1 var(--font-ui);
    color: var(--muted); background: transparent; border: 0; cursor: pointer;
  }
  .rail-toggle:hover { color: var(--fg); background: transparent; }
  .glyph { width: 1em; }
  .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
  /* The rail already rules the section off; a first group needs no second line. */
  .body > :global(.ui-section-group:first-child) { border-top: 0; }
</style>
