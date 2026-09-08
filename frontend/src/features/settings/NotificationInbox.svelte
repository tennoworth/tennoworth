<script lang="ts">
  import { useDesktopServices } from '../../ui/desktop-context';
  const { desktopNotifications, desktopReadNotifications, desktopClearNotifications, listenForTauriEvent } = useDesktopServices();
  import { onMount } from 'svelte';
  
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
  const targets = { sell: 'Open Sell', orders: 'Review My Orders', ledger: 'Open Ledger', watches: 'Open price watches', baro: 'Open Baro and ducat planner', routines: 'Open calendar' };
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
    return () => { request++; stop(); };
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
  <div class="ui-toolbar">
    <button class="btn" onclick={onsettings}>Notification settings</button>
    <button class="btn" disabled={busy || !entries.some(n => !n.read)} onclick={() => action(() => desktopReadNotifications())}>Mark all read</button>
    <button class="btn" disabled={busy || !entries.length} onclick={() => confirmClear = true}>Clear history</button>
    <label><input type="checkbox" bind:checked={unreadOnly} /> Unread only</label>
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
    <ol>
      {#each visible as entry (entry.id)}
        <li class="notification-entry" class:unread={!entry.read}>
          <div class="entry-meta"><span>{entry.read ? 'Read' : 'Unread'} · {entry.category}</span><time datetime={new Date(entry.created_at * 1000).toISOString()}>{new Date(entry.created_at * 1000).toLocaleString()}</time></div>
          <h3>{entry.title}</h3>
          <p class="body">{entry.body}</p>
          {#if entry.delivery === 'failed'}<p class="ui-notice" data-tone="warn">Desktop popup could not be delivered. This notification is saved here.</p>{/if}
          {#if entry.delivery === 'pending'}<p class="muted">Desktop delivery was not confirmed. This notification is saved here.</p>{/if}
          <div class="ui-toolbar">
            <button class="btn" disabled={busy} onclick={() => open(entry)}>{targets[entry.target]}</button>
            {#if !entry.read}<button class="btn ghost" disabled={busy} onclick={() => action(() => desktopReadNotifications(entry.id))}>Mark read</button>{/if}
          </div>
        </li>
      {/each}
    </ol>
  {/if}
</section>
<style>
  .inbox { padding: var(--inset); }
  ol { list-style: none; padding: 0; margin: 0; }
  h3, p { margin: 0; }
  .body { white-space: pre-wrap; overflow-wrap: anywhere; line-height: var(--leading-body); }
  h3 { overflow-wrap: anywhere; }
  .entry-meta { display: flex; flex-wrap: wrap; gap: var(--s2) var(--s4); color: var(--muted); font-size: var(--text-caption); }
  .muted { color: var(--muted); }
  label { display: flex; align-items: center; gap: var(--s2); min-height: var(--ctl); }
  input { accent-color: var(--accent); }
</style>
