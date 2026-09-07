<script lang="ts">
  import { onMount } from 'svelte';
  import { desktopNotificationPreferences, desktopSaveNotificationPreferences, desktopTestNotification, NOTIFICATION_CATEGORIES, type NotificationPreferences, type NotificationCategory } from '../lib/transport';
  import { humanError } from '../lib/errors';
  let preferences = $state<NotificationPreferences | null>(null);
  let error = $state('');
  let message = $state('');
  let busy = $state(false);
  const labels = { trades: 'Completed trades and listing follow-up', watches: 'Price watches', scans: 'Inventory scan summary', baro: 'Baro arrival and departure', calendar: 'Events affecting your holdings', digest: 'Daily sell opportunities' };
  async function load() {
    error = '';
    try {
      const value = await desktopNotificationPreferences();
      if (!value || typeof value.popups !== 'boolean' || NOTIFICATION_CATEGORIES.some(k => typeof value.categories?.[k]?.enabled !== 'boolean' || typeof value.categories?.[k]?.native !== 'boolean')) throw new Error('Notification preferences could not be read. Please retry.');
      preferences = value;
    } catch (e) { error = humanError(e); }
  }
  onMount(() => { void load(); });
  async function save(next: NotificationPreferences) {
    busy = true; error = ''; message = '';
    try { preferences = await desktopSaveNotificationPreferences(next); message = 'Preferences saved.'; }
    catch (e) { error = humanError(e); }
    finally { busy = false; }
  }
  function change(event: Event, category?: NotificationCategory, field: 'enabled' | 'native' = 'enabled') {
    if (!preferences) return;
    const control = event.currentTarget as HTMLInputElement;
    const next = control.checked;
    control.checked = category ? preferences.categories[category][field] : preferences.popups;
    void save(category
      ? { ...preferences, categories: { ...preferences.categories, [category]: { ...preferences.categories[category], [field]: next } } }
      : { ...preferences, popups: next });
  }
  async function test() {
    busy = true; error = ''; message = '';
    try { message = await desktopTestNotification(); }
    catch (e) { error = humanError(e); }
    finally { busy = false; }
  }
</script>
<section class="ui-panel ui-stack notification-settings" aria-labelledby="notification-settings-title">
  <h3 id="notification-settings-title">Notifications</h3>
  <p>Alerts run while TennoWorth is open, including in the tray. Your inbox keeps the latest 1,000 entries for up to 30 days.</p>
  {#if error}<div class="ui-notice" data-tone="bad" role="alert">{error} <button class="btn xs" onclick={load} disabled={busy}>Retry loading preferences</button></div>{/if}
  {#if preferences}
    <label class="toggle"><input type="checkbox" checked={preferences.popups} disabled={busy} onchange={(e) => change(e)} /> Desktop popups</label>
    <p class="muted">Pause popups to keep alerts in your inbox without interruptions. Operating-system settings may also suppress popups.</p>
    <div class="categories">
      {#each NOTIFICATION_CATEGORIES as category}
        <div class="category">
          <strong>{labels[category]}</strong>
          <label class="toggle"><input type="checkbox" aria-label={`${labels[category]} enabled`} checked={preferences.categories[category].enabled} disabled={busy} onchange={(e) => change(e, category, 'enabled')} /> Enabled</label>
          <label class="toggle"><input type="checkbox" aria-label={`${labels[category]} popups`} checked={preferences.categories[category].native} disabled={busy || !preferences.popups || !preferences.categories[category].enabled} onchange={(e) => change(e, category, 'native')} /> Popup</label>
        </div>
      {/each}
    </div>
    <p class="muted">Baro: one hour before arrival, on arrival, and one hour before departure. Relevant events: when active and one hour before ending. Sell digest: once daily after 18:00 local time, with inventory and prices no older than 24 hours.</p>
  {:else if !error}<p role="status">Loading notification preferences…</p>{/if}
  <div><button class="btn" disabled={busy} onclick={test}>Send test notification</button></div>
  {#if message}<p class="ui-notice" data-tone="good" role="status">{message}</p>{/if}
</section>
<style>
  .notification-settings { padding: var(--inset); }
  h3, p { margin: 0; }
  p { line-height: var(--leading-body); }
  .muted { color: var(--muted); }
  .category { display: flex; flex-wrap: wrap; align-items: center; gap: var(--s3); padding: var(--s3) 0; border-bottom: 1px var(--rule) var(--hairline); }
  .category strong { flex: 1 1 16rem; font-family: var(--font-body); }
  .toggle { display: flex; align-items: center; gap: var(--s2); min-height: var(--ctl); cursor: pointer; }
  input { width: var(--s4); height: var(--s4); accent-color: var(--accent); }
</style>
