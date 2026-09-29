<script lang="ts">
  import { useDesktopServices } from '../../ui/desktop-context';
  const { desktopNotificationPreferences, desktopSaveNotificationPreferences, desktopTestNotification } = useDesktopServices();
  import { onMount } from 'svelte';
  
import { NOTIFICATION_CATEGORIES } from '../../contracts/desktop';
import { type NotificationPreferences, type NotificationCategory } from '../../contracts/desktop';
  import { humanError } from '../../contracts/errors';
  let preferences = $state<NotificationPreferences | null>(null);
  let error = $state('');
  let message = $state('');
  let busy = $state(false);
  const labels = { trades: 'Completed trades and listing follow-up', watches: 'Price watches', baro: 'Baro arrival and departure', calendar: 'Events affecting your holdings', digest: 'Daily sell opportunities' };
  const timing: Partial<Record<NotificationCategory, string>> = { baro: 'One hour before arrival, on arrival, and one hour before departure.', calendar: 'When active and one hour before ending.', digest: 'Once daily after 18:00 local time, with inventory and prices no older than 24 hours.' };
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
<section class="wrap tw notification-settings" id="settings-notifications" tabindex="-1" aria-labelledby="notification-settings-title">
  <div class="rail"><h3 id="notification-settings-title">Notifications</h3><button class="btn xs" disabled={busy} onclick={test}>Send test notification</button></div>
  {#if error}<div class="ui-notice notice" data-tone="bad" role="alert">{error} <button class="btn xs" onclick={load} disabled={busy}>Retry loading preferences</button></div>{/if}
  {#if preferences}
    <div class="ui-setting-row">
      <div class="ui-setting-copy"><label for="notification-popups">Desktop popups</label><p>Pause popups to keep alerts in your inbox without interruptions. Operating-system settings may also suppress popups.</p></div>
      <label class="ui-setting-check"><input id="notification-popups" type="checkbox" checked={preferences.popups} disabled={busy} onchange={(e) => change(e)} /><span>Enabled</span></label>
    </div>
    <div class="scroll">
      <table class="tw categories">
        <thead><tr><th>Alert</th><th>Inbox</th><th>Popup</th></tr></thead>
        <tbody>
          {#each NOTIFICATION_CATEGORIES as category}
            <tr>
              <td><strong>{labels[category]}</strong>{#if timing[category]}<small>{timing[category]}</small>{/if}</td>
              <td><input type="checkbox" aria-label={`${labels[category]} in inbox`} checked={preferences.categories[category].enabled} disabled={busy} onchange={(e) => change(e, category, 'enabled')} /></td>
              <td><input type="checkbox" aria-label={`${labels[category]} popup`} checked={preferences.categories[category].native} disabled={busy || !preferences.popups || !preferences.categories[category].enabled} onchange={(e) => change(e, category, 'native')} /></td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else if !error}<p class="inset" role="status">Loading notification preferences…</p>{/if}
  <p class="inset footnote">Alerts run while TennoWorth is open, including in the tray. Your inbox keeps the latest 1,000 entries for up to 30 days.</p>
  {#if message}<p class="ui-notice notice" data-tone="good" role="status">{message}</p>{/if}
</section>
<style>
  p { margin: 0; }
  .inset { padding: var(--s3) var(--inset); line-height: var(--leading-body); }
  .footnote { color: var(--muted); font-size: var(--text-control); border-top: 1px var(--rule) var(--border); }
  .notice { margin: 0 var(--inset) var(--s4); }
  .rail + .notice { margin-top: var(--s4); }
  .scroll { border-top: 1px var(--rule) var(--border); }
  /* Category names are reading text that wraps, not truncated data. */
  .categories td:first-child { height: auto; padding-block: var(--s2); text-align: left; white-space: normal; font-family: var(--font-body); color: var(--fg); }
  .categories th:first-child { text-align: left; }
  .categories :is(th, td):not(:first-child) { width: 6rem; text-align: center; }
  .categories strong { display: block; font-weight: 500; }
  .categories small { display: block; color: var(--muted); font-size: var(--text-caption); line-height: var(--leading-body); }
  @media (max-width: 480px) { .categories :is(th, td):not(:first-child) { width: 3.5rem; } }
</style>
