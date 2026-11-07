<script lang="ts">
  import { busy as pending } from '../../ui/keyboard-actions';
  import { fitTable } from '../../ui/table-fit';
  import { useDesktopServices } from '../shell/desktop-context';
  const { desktopNotificationPreferences, desktopSaveNotificationPreferences, desktopTestNotification } = useDesktopServices();
  import { onMount } from 'svelte';
  
import { NOTIFICATION_CATEGORIES, IN_GAME_CATEGORIES } from '../../contracts/desktop';
import { type NotificationPreferences, type NotificationCategory } from '../../contracts/desktop';
  import { humanError } from '../../contracts/errors';
  import { t } from '../../i18n/i18n.svelte';
  let preferences = $state<NotificationPreferences | null>(null);
  let error = $state('');
  let message = $state('');
  let busy = $state(false);
  const label = (category: NotificationCategory) => t(`notify.label.${category}`);
  const TIMED = ['baro', 'calendar', 'digest', 'announcements'] as const;
  const timing = (category: NotificationCategory) => ((TIMED as readonly string[]).includes(category) ? t(`notify.timing.${category as (typeof TIMED)[number]}`) : '');
  async function load() {
    error = '';
    try {
      const value = await desktopNotificationPreferences();
      if (!value || typeof value.popups !== 'boolean' || NOTIFICATION_CATEGORIES.some(k => typeof value.categories?.[k]?.enabled !== 'boolean' || typeof value.categories?.[k]?.native !== 'boolean')) throw new Error(t('notify.readFailed'));
      preferences = value;
    } catch (e) { error = humanError(e); }
  }
  onMount(() => { void load(); });
  async function save(next: NotificationPreferences) {
    busy = true; error = ''; message = '';
    try { preferences = await desktopSaveNotificationPreferences(next); message = t('notify.saved'); }
    catch (e) { error = humanError(e); }
    finally { busy = false; }
  }
  const offersInGame = (category: NotificationCategory) => (IN_GAME_CATEGORIES as readonly string[]).includes(category);
  const anyInGame = $derived(!!preferences && NOTIFICATION_CATEGORIES.some(k => offersInGame(k) && preferences!.categories[k].in_game));
  function change(event: Event, category?: NotificationCategory, field: 'enabled' | 'native' | 'in_game' = 'enabled') {
    if (!preferences) return;
    const control = event.currentTarget as HTMLInputElement;
    const next = control.checked;
    control.checked = category ? preferences.categories[category][field] : preferences.popups;
    void save(category
      ? { ...preferences, categories: { ...preferences.categories, [category]: { ...preferences.categories[category], [field]: next } } }
      : { ...preferences, popups: next });
  }
  function changeCopy(event: Event) {
    if (!preferences) return;
    const control = event.currentTarget as HTMLInputElement;
    const next = control.checked;
    control.checked = preferences.copy_whisper;
    void save({ ...preferences, copy_whisper: next });
  }
  async function test() {
    busy = true; error = ''; message = '';
    try { message = await desktopTestNotification(); }
    catch (e) { error = humanError(e); }
    finally { busy = false; }
  }
</script>
<section class="wrap tw notification-settings" id="settings-notifications" tabindex="-1" aria-labelledby="notification-settings-title">
  <div class="rail"><h3 id="notification-settings-title">{t('inbox.title')}</h3><button class="btn xs" use:pending={busy} onclick={test}>{t('notify.test')}</button></div>
  {#if error}<div class="ui-notice notice" data-tone="bad" role="alert">{error} <button class="btn xs" onclick={load} use:pending={busy}>{t('notify.retryLoad')}</button></div>{/if}
  {#if preferences}
    <div class="ui-setting-row">
      <div class="ui-setting-copy"><label for="notification-popups">{t('notify.popups')}</label><p>{t('notify.popupsHelp')}</p></div>
      <label class="ui-setting-check"><input id="notification-popups" type="checkbox" checked={preferences.popups} use:pending={busy} onchange={(e) => change(e)} /><span>{t('notify.enabled')}</span></label>
    </div>
    <div class="scroll" use:fitTable>
      <table class="tw categories">
        <thead><tr><th>{t('notify.col.alert')}</th><th>{t('notify.col.inbox')}</th><th>{t('notify.col.popup')}</th><th>{t('notify.col.inGame')}</th></tr></thead>
        <tbody>
          {#each NOTIFICATION_CATEGORIES as category}
            <tr>
              <td><strong>{label(category)}</strong>{#if timing(category)}<small>{timing(category)}</small>{/if}</td>
              <td><input type="checkbox" aria-label={t('notify.inInbox', { alert: label(category) })} checked={preferences.categories[category].enabled} use:pending={busy} onchange={(e) => change(e, category, 'enabled')} /></td>
              <td><input type="checkbox" aria-label={t('notify.popup', { alert: label(category) })} checked={preferences.categories[category].native} disabled={!preferences.popups || !preferences.categories[category].enabled} use:pending={busy} onchange={(e) => change(e, category, 'native')} /></td>
              <td>{#if offersInGame(category)}<input type="checkbox" aria-label={t('notify.inGame', { alert: label(category) })} checked={preferences.categories[category].in_game} disabled={!preferences.popups || !preferences.categories[category].enabled} use:pending={busy} onchange={(e) => change(e, category, 'in_game')} />{:else}<span class="visually-hidden">{t('notify.notOffered')}</span>{/if}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <p class="inset in-game-note">{t('notify.inGameNote')}</p>
    <div class="ui-setting-row">
      <div class="ui-setting-copy"><label for="notification-copy-whisper">{t('notify.copyWhisper')}</label><p>{t('notify.copyWhisperHelp')}</p></div>
      <label class="ui-setting-check"><input id="notification-copy-whisper" type="checkbox" checked={preferences.copy_whisper} disabled={!anyInGame} use:pending={busy} onchange={changeCopy} /><span>{t('notify.enabled')}</span></label>
    </div>
  {:else if !error}<p class="inset" role="status">{t('notify.loading')}</p>{/if}
  <p class="inset footnote">{t('notify.footnote')}</p>
  {#if message}<p class="ui-notice notice" data-tone="good" role="status">{message}</p>{/if}
</section>
<style>
  p { margin: 0; }
  .inset { padding: var(--s3) var(--inset); line-height: var(--leading-body); }
  .footnote { color: var(--muted); font-size: var(--text-control); border-top: 1px var(--rule) var(--border); }
  .notice { margin: 0 var(--inset) var(--s4); }
  .rail + .notice { margin-top: var(--s4); }
  .scroll { border-top: 1px var(--rule) var(--border); }
  /* Anchors the hidden text to its cell; otherwise it escapes the table's scroll box and widens the page. */
  .categories td:last-child { position: relative; }
  .visually-hidden { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .in-game-note { color: var(--muted); font-size: var(--text-control); border-top: 1px var(--rule) var(--border); }
  /* Category names are reading text that wraps, not truncated data. */
  .categories td:first-child { height: auto; padding-block: var(--s2); text-align: left; white-space: normal; font-family: var(--font-body); color: var(--fg); }
  .categories th:first-child { text-align: left; }
  .categories :is(th, td):not(:first-child) { width: 6rem; text-align: center; }
  .categories strong { display: block; font-weight: 500; }
  .categories small { display: block; color: var(--muted); font-size: var(--text-caption); line-height: var(--leading-body); }
  @media (max-width: 480px) { .categories :is(th, td):not(:first-child) { width: 3.5rem; } }
</style>
