<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { activeAnnouncements, parseAnnouncements } from '../../domain/announcements';
  import type { AnnouncementFeed } from '../../contracts/generated/desktop';
  import type { SettingsStore } from '../../contracts/state-store';
  let { now, load, store, subscribe }: {
    now: number;
    load: () => Promise<AnnouncementFeed | null>;
    store?: SettingsStore;
    subscribe?: (refresh: () => void) => () => void;
  } = $props();
  let feed = $state<AnnouncementFeed | null>(null);
  let saving = $state(false);
  let saveError = $state('');
  let dismissed = $state<string[]>(untrack(() => {
    try {
      const value: unknown = JSON.parse(store?.getSetting('dismissed-announcements') ?? '[]');
      return Array.isArray(value) ? value.filter((id): id is string => typeof id === 'string').slice(-100) : [];
    } catch { return []; }
  }));
  let active = $derived(activeAnnouncements(feed, now).filter(message => !dismissed.includes(message.id)));
  onMount(() => {
    let mounted = true;
    let generation = 0;
    const refresh = async () => {
      const current = ++generation;
      try {
        const next = parseAnnouncements(await load());
        if (mounted && current === generation && next && (!feed || next.updated_at >= feed.updated_at)) feed = next;
      } catch { /* Keep the last valid feed until each message's expiry. */ }
    };
    void refresh();
    const timer = window.setInterval(() => { if (!document.hidden) void refresh(); }, 15 * 60_000);
    const focused = () => { if (!document.hidden) void refresh(); };
    window.addEventListener('focus', focused);
    document.addEventListener('visibilitychange', focused);
    const stop = subscribe?.(() => { void refresh(); });
    return () => { mounted = false; window.clearInterval(timer); window.removeEventListener('focus', focused); document.removeEventListener('visibilitychange', focused); stop?.(); };
  });
  async function dismiss(id: string) {
    saving = true; saveError = '';
    const next = [...dismissed.filter(value => value !== id), id].slice(-100);
    try {
      await store?.setSetting('dismissed-announcements', JSON.stringify(next));
      if (store && store.getSetting('dismissed-announcements') !== JSON.stringify(next)) throw new Error('Dismissal was not saved');
      dismissed = next;
    } catch { saveError = 'Could not save dismissal. Try again.'; }
    finally { saving = false; }
  }
</script>

{#each active as message (message.id)}
  <section class="ui-notice general-banner market-announcement" data-tone="warn" aria-label={message.title}>
    <div class="gb-body ui-stack">
      <strong>{message.title}</strong>
      <p>{message.body}</p>
      {#if saveError}<p class="bad" role="alert">{saveError}</p>{/if}
    </div>
    <div class="gb-actions">
      <a class="btn xs" href={message.source_url} target="_blank" rel="noopener noreferrer">Source announcement ↗</a>
      <button type="button" class="btn ghost xs" aria-label="Dismiss {message.title}" disabled={saving} onclick={() => dismiss(message.id)}>Dismiss</button>
    </div>
  </section>
{/each}

<style>
  .market-announcement { display: flex; margin-bottom: var(--s4); }
  .gb-body { gap: var(--s1); }
  .gb-body p { margin: 0; white-space: pre-line; }
</style>
