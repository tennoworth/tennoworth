<script lang="ts">
  import { busy as pending, roving } from '../../ui/keyboard-actions';
  // Settings → warframe.market account → Trade presence: the status, following
  // the game, and how long a status picked by hand is kept.
  import { PRESENCE_KEEP_FOR_CHOICES, type PresenceChoice } from '../../contracts/desktop';
  import type { PresenceController } from './presence.svelte';
  import Select from '../../ui/Select.svelte';
  import { formatLocale, t } from '../../i18n/i18n.svelte';
  import Rich from '../../ui/Rich.svelte';
  import { PRESENCE_CHOICES, presenceLabel, keepForLabel, presenceLine, presenceProblem, presenceUsable } from './presence';

  let { presence }: { presence: PresenceController } = $props();

  const keepChoices: (number | null)[] = [null, ...PRESENCE_KEEP_FOR_CHOICES];
  let closedChoices = $derived((['invisible', 'online'] as PresenceChoice[]).map((choice) => ({ value: choice, label: presenceLabel(choice) })));

  let status = $derived(presence.status);
  let usable = $derived(presenceUsable(status));
  let problem = $derived(status ? presenceProblem(status) : null);
</script>

<div class="ui-section-group presence" aria-labelledby="set-presence">
  <h4 id="set-presence">{t('presence.title')}</h4>
  {#if !status}
    <p class="inset-note">{t('presence.reading')}</p>
  {:else if !status.signedIn}
    <p class="inset-note">{t('presence.locked')}</p>
  {:else}
    {#if status.followPaused}
      <div class="ui-notice" data-tone="warn">
        <p>{t('presence.pausedNoteSettings')}</p>
        <button type="button" class="btn xs" onclick={() => void presence.followNow()}>{t('presence.followNow')}</button>
      </div>
    {/if}
    {#if problem}<div class="ui-notice" data-tone="bad"><p>{problem}</p></div>{/if}
    {#if presence.error}<div class="ui-notice" data-tone="bad" role="alert"><p>{presence.error}</p></div>{/if}
    <div class="ui-setting-row">
      <div class="ui-setting-copy"><strong id="presence-status-label">{t('presence.status')}</strong><p>{t('presence.statusHelp', { line: presenceLine(status) })}</p></div>
      <div class="ui-setting-control">
        <div class="ui-segmented" role="group" aria-labelledby="presence-status-label" use:roving>
          {#each PRESENCE_CHOICES as choice (choice)}
            <button type="button" aria-pressed={status.status === choice} disabled={!usable} use:pending={presence.pending !== null} onclick={() => void presence.set(choice)}>{presenceLabel(choice)}{#if presence.pending === choice}…{/if}</button>
          {/each}
        </div>
      </div>
    </div>
    <div class="ui-setting-row">
      <div class="ui-setting-copy"><label for="presence-follow">{t('presence.follow')}</label><p>{t('presence.followHelp')}</p><p>{t('presence.rules')}</p></div>
      <div class="ui-setting-control">
        <label class="ui-setting-check"><input id="presence-follow" type="checkbox" checked={status.settings.followGame} use:pending={presence.savingSettings} onchange={(e) => void presence.saveSettings({ ...status.settings, followGame: e.currentTarget.checked })}><span>{t('presence.enabled')}</span></label>
        <span id="presence-closed-label" class="status">{t('presence.whenClosed')}</span>
        <Select id="presence-closed" labelledby="presence-closed-label" value={status.settings.whenClosed} options={closedChoices} disabled={presence.savingSettings || !status.settings.followGame} onchange={(choice) => void presence.saveSettings({ ...status.settings, whenClosed: choice })} />
      </div>
    </div>
    <div class="ui-setting-row">
      <div class="ui-setting-copy"><strong id="presence-keep-setting">{t('presence.keepFor')}</strong><p>{t('presence.keepHelp')}{#if status.following}{' '}{t('presence.keepFollowing')}{/if}</p></div>
      <div class="ui-setting-control">
        <div class="ui-segmented" role="group" aria-labelledby="presence-keep-setting" use:roving>
          {#each keepChoices as minutes (minutes)}
            <button type="button" aria-pressed={!status.following && status.settings.keepForMinutes === minutes} disabled={status.following} use:pending={presence.savingSettings} onclick={() => void presence.saveSettings({ ...status.settings, keepForMinutes: minutes })}>{keepForLabel(minutes)}</button>
          {/each}
        </div>
      </div>
    </div>
    <details class="troubleshooting">
      <summary>{t('presence.troubleshooting')}</summary>
      <div class="body">
        <p>{t(status.connected ? 'presence.connected' : 'presence.notConnected')}{#if status.statusSetAt} <Rich text={t('presence.lastChange', { time: new Date(status.statusSetAt).toLocaleTimeString(formatLocale()) })} />{/if}{#if status.detail} <Rich text={t('presence.lastReply', { reply: status.detail })} />{/if}</p>
        <p>{t('presence.privacy')}</p>
      </div>
    </details>
  {/if}
</div>

<style>
  .presence { border-bottom: 0; }
  .inset-note { margin: 0; padding: 0 var(--inset); font-size: var(--text-control); line-height: var(--leading-body); color: var(--muted); }
  .ui-notice { margin: 0 var(--inset) var(--s3); display: flex; flex-wrap: wrap; align-items: center; gap: var(--s2) var(--s3); }
  .ui-notice p { margin: 0; flex: 1 1 18rem; min-width: 0; }
  .status { font-size: var(--text-control); color: var(--muted); }
  .troubleshooting { margin-top: var(--s2); border-top: 1px var(--rule) var(--border); }
  .troubleshooting > summary { min-height: var(--ctl-lg); padding: var(--s1) var(--inset); cursor: pointer; color: var(--muted); font-size: var(--text-control); }
  .body { display: flex; flex-direction: column; gap: var(--s2); padding: 0 var(--inset) var(--s2); font-size: var(--text-control); line-height: var(--leading-body); color: var(--muted); }
  .body p { margin: 0; }
  .body :global(code) { font-family: var(--font-mono); font-size: var(--text-caption); }
</style>
