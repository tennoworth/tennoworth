<script lang="ts">
  // Settings → warframe.market account → Trade presence: the status, following
  // the game, and how long a status picked by hand is kept.
  import { PRESENCE_KEEP_FOR_CHOICES, type PresenceChoice } from '../../contracts/desktop';
  import type { PresenceController } from './presence.svelte';
  import { PRESENCE_CHOICES, PRESENCE_LABEL, keepForLabel, presenceLine, presenceProblem, presenceUsable } from './presence';

  let { presence }: { presence: PresenceController } = $props();

  const keepChoices: (number | null)[] = [null, ...PRESENCE_KEEP_FOR_CHOICES];
  const closedChoices: PresenceChoice[] = ['invisible', 'online'];

  let status = $derived(presence.status);
  let usable = $derived(presenceUsable(status));
  let problem = $derived(status ? presenceProblem(status) : null);
</script>

<div class="ui-section-group presence" aria-labelledby="set-presence">
  <h4 id="set-presence">Trade presence</h4>
  {#if !status}
    <p class="inset-note">Reading your warframe.market status…</p>
  {:else if !status.signedIn}
    <p class="inset-note">Unlock your warframe.market session to show or change your status. Until then warframe.market keeps whatever it last had.</p>
  {:else}
    {#if status.followPaused}
      <div class="ui-notice" data-tone="warn">
        <p>You picked a status, here or on warframe.market, so following the game waits for your next game session.</p>
        <button type="button" class="btn xs" onclick={() => void presence.followNow()}>Follow the game now</button>
      </div>
    {/if}
    {#if problem}<div class="ui-notice" data-tone="bad"><p>{problem}</p></div>{/if}
    {#if presence.error}<div class="ui-notice" data-tone="bad" role="alert"><p>{presence.error}</p></div>{/if}
    <div class="ui-setting-row">
      <div class="ui-setting-copy"><strong id="presence-status-label">Status</strong><p>What other traders see next to your orders. {presenceLine(status)}.</p></div>
      <div class="ui-setting-control">
        <div class="ui-segmented" role="group" aria-labelledby="presence-status-label">
          {#each PRESENCE_CHOICES as choice (choice)}
            <button type="button" aria-pressed={status.status === choice} disabled={!usable || presence.pending !== null} onclick={() => void presence.set(choice)}>{PRESENCE_LABEL[choice]}{#if presence.pending === choice}…{/if}</button>
          {/each}
        </div>
      </div>
    </div>
    <div class="ui-setting-row">
      <div class="ui-setting-copy"><label for="presence-follow">Follow the game</label><p>Online in game from the moment you log in to Warframe until you quit. If you change your status yourself, here or on warframe.market, following pauses until your next game session.</p></div>
      <div class="ui-setting-control">
        <label class="ui-setting-check"><input id="presence-follow" type="checkbox" checked={status.settings.followGame} disabled={presence.savingSettings} onchange={(e) => void presence.saveSettings({ ...status.settings, followGame: e.currentTarget.checked })}><span>Enabled</span></label>
        <label for="presence-closed" class="status">When Warframe closes</label>
        <select id="presence-closed" class="ui-input" value={status.settings.whenClosed} disabled={presence.savingSettings || !status.settings.followGame} onchange={(e) => void presence.saveSettings({ ...status.settings, whenClosed: e.currentTarget.value as PresenceChoice })}>
          {#each closedChoices as choice (choice)}<option value={choice}>{PRESENCE_LABEL[choice]}</option>{/each}
        </select>
      </div>
    </div>
    <div class="ui-setting-row">
      <div class="ui-setting-copy"><strong id="presence-keep-setting">Keep status for</strong><p>When you pick a status yourself. After that warframe.market sets you to Invisible, as it does on the website.{#if status.following}{' '}Following the game manages this for you right now.{/if}</p></div>
      <div class="ui-setting-control">
        <div class="ui-segmented" role="group" aria-labelledby="presence-keep-setting">
          {#each keepChoices as minutes (minutes)}
            <button type="button" aria-pressed={!status.following && status.settings.keepForMinutes === minutes} disabled={status.following || presence.savingSettings} onclick={() => void presence.saveSettings({ ...status.settings, keepForMinutes: minutes })}>{keepForLabel(minutes)}</button>
          {/each}
        </div>
      </div>
    </div>
    <details class="troubleshooting">
      <summary>Troubleshooting: status connection</summary>
      <div class="body">
        <p>{status.connected ? 'Connected to warframe.market live updates.' : 'Not connected.'}{#if status.statusSetAt} Last change <span class="mono">{new Date(status.statusSetAt).toLocaleTimeString()}</span>.{/if}{#if status.detail} Last reply <span class="mono">{status.detail}</span>.{/if}</p>
        <p>Reads Warframe's EE.log only to tell when you log in and quit. Nothing is sent to the game and no input is injected. When TennoWorth quits or you sign out, a status it was keeping up is set to Invisible.</p>
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
  .mono { font-family: var(--font-mono); font-size: var(--text-caption); }
</style>
