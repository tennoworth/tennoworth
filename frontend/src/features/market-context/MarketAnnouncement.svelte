<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import AnnouncementBanner from './AnnouncementBanner.svelte';
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
  let failed = $state<string | null>(null);
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
    saving = true; failed = null;
    const next = [...dismissed.filter(value => value !== id), id].slice(-100);
    try {
      await store?.setSetting('dismissed-announcements', JSON.stringify(next));
      if (store && store.getSetting('dismissed-announcements') !== JSON.stringify(next)) throw new Error('Dismissal was not saved');
      dismissed = next;
    } catch { failed = id; }
    finally { saving = false; }
  }
</script>

{#each active as message (message.id)}
  <AnnouncementBanner {message} {saving} error={failed === message.id ? 'Could not save dismissal. Try again.' : ''} ondismiss={() => dismiss(message.id)} />
{/each}
