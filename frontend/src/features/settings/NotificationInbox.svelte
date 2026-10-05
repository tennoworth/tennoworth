<script lang="ts">
  import { useDesktopServices } from '../../ui/desktop-context';
  const { desktopNotifications, desktopReadNotifications, desktopClearNotifications, listenForTauriEvent } = useDesktopServices();
  import { onMount } from 'svelte';
  import NotificationRow from './NotificationRow.svelte';
  import { notificationDate } from '../../ui/format';
  import { NOTIFICATIONS_EVENT } from '../../contracts/events';
  import { type NotificationEntry, type NotificationTarget } from '../../contracts/desktop';
  import { humanError } from '../../contracts/errors';
  let { onopen, onsettings }: { onopen: (target: NotificationTarget) => void; onsettings: () => void } = $props();
  let entries = $state<NotificationEntry[]>([]);
  let loading = $state(true);
  let error = $state('');
  let busy = $state(false);
  let confirmClear = $state(false);
  let unreadOnly = $state(false);
  let visible = $derived(entries.filter(n => !unreadOnly || !n.read));
  let now = $state(Date.now());
  let unread = $derived(entries.filter(entry => !entry.read).length);
  let groups = $derived.by(() => {
    const days = new Map<string, { date: ReturnType<typeof notificationDate>; entries: NotificationEntry[] }>();
    for (const entry of visible) {
      const date = notificationDate(entry.created_at, now);
      let group = days.get(date.key);
      if (!group) { group = { date, entries: [] }; days.set(date.key, group); }
      group.entries.push(entry);
    }
    return [...days.values()];
  });
  let request = 0;
  async function load() {
    const current = ++request;
    try { const rows = await desktopNotifications(); if (current === request) { entries = rows; error = ''; } }
    catch (e) { if (current === request) error = humanError(e); }
    finally { if (current === request) loading = false; }
  }
  onMount(() => {
    const stop = listenForTauriEvent(NOTIFICATIONS_EVENT, () => { void load(); });
    void load();
    const timer = setInterval(() => now = Date.now(), 60_000);
    return () => { request++; stop(); clearInterval(timer); };
  });
  async function action(fn: () => Promise<void>) {
    busy = true;
    try { await fn(); await load(); } catch (e) { error = humanError(e); }
    finally { busy = false; }
  }
  async function open(entry: NotificationEntry) {
    await action(() => desktopReadNotifications(entry.id));
    onopen(entry.target);
  }
</script>
<section class="view-header">
  <h2>Notifications</h2>
  <p class="lede">Trade results and timely opportunities, with a next step when you need one.</p>
</section>
<section class="ui-panel ui-stack inbox" aria-label="Notification history" aria-busy={loading || busy}>
  <div class="ui-toolbar inbox-controls">
    <div class="ui-toolbar"><button class="btn" onclick={onsettings}>Notification settings</button>
    <button class="btn" disabled={busy || !entries.some(n => !n.read)} onclick={() => action(() => desktopReadNotifications())}>Mark all read</button>
    <button class="btn" disabled={busy || !entries.length} onclick={() => confirmClear = true}>Clear history</button></div>
    <label><input type="checkbox" bind:checked={unreadOnly} /> Unread only</label>
    <span class="history-count">{unread} unread · {entries.length} {entries.length === 1 ? 'notification' : 'notifications'}</span>
  </div>
  {#if confirmClear}
    <div class="ui-notice ui-toolbar" data-tone="warn">
      <span>Clear notification history? Your trade ledger and reminders are kept.</span>
      <button class="btn bad" disabled={busy} onclick={() => action(async () => { await desktopClearNotifications(); confirmClear = false; })}>Clear notifications</button>
      <button class="btn" disabled={busy} onclick={() => confirmClear = false}>Cancel</button>
    </div>
  {/if}
  {#if error}<div class="ui-notice" data-tone="bad" role="alert">{error} <button class="btn xs" disabled={busy} onclick={load}>Retry</button></div>{/if}
  {#if loading}<p role="status">Loading notifications…</p>
  {:else if !visible.length && !error}<p>{unreadOnly ? 'You’re all caught up.' : 'No notifications yet. Trades, price watches, and timely reminders will appear here.'}</p>
  {:else}
    <ol class="history">
      {#each groups as group (group.date.key)}
        <li class="notification-day">
          <div class="day-heading"><h3>{group.date.label}</h3><span>{group.date.date}</span></div>
          <ol>
            {#each group.entries as entry (entry.id)}
              <li><NotificationRow {entry} {now} {busy} onopen={() => open(entry)} onread={() => action(() => desktopReadNotifications(entry.id))} /></li>
            {/each}
          </ol>
        </li>
      {/each}
    </ol>
  {/if}
</section>
<style>
  .inbox { padding: 0; gap: 0; }
  .inbox-controls { padding: var(--s4) var(--inset); border-bottom: 1px solid var(--border); }
  .inbox-controls label { margin-left: auto; }
  .history-count { color: var(--muted); font-size: var(--text-control); }
  ol { list-style: none; padding: 0; margin: 0; }
  .day-heading { display: flex; flex-wrap: wrap; align-items: baseline; gap: var(--s2) var(--s3); padding: var(--s3) var(--inset); border-bottom: 1px var(--rule) var(--hairline); }
  .day-heading h3 { margin: 0; font: 500 var(--text-caption)/var(--leading-body) var(--font-ui); text-transform: uppercase; letter-spacing: .12em; }
  .day-heading span { color: var(--muted); font-size: var(--text-caption); }
  .inbox > :is(p, .ui-notice) { margin: var(--s4) var(--inset); }
  label { display: flex; align-items: center; gap: var(--s2); min-height: var(--ctl); }
  @media (max-width: 560px) { .inbox-controls label { margin-left: 0; } }
</style>
