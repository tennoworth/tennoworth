<script lang="ts">
  // The status strip's warframe.market status: the word and its mark, opening
  // the same choices the website offers plus Follow the game.
  import { untrack } from 'svelte';
  import { PRESENCE_KEEP_FOR_CHOICES, type PresenceChoice } from '../../contracts/desktop';
  import type { PresenceController } from './presence.svelte';
  import PresenceMark from './PresenceMark.svelte';
  import { PRESENCE_CHOICES, PRESENCE_HINT, PRESENCE_LABEL, keepForLabel, presenceLine, presenceMark, presenceProblem, presenceUsable, presenceWord } from './presence';

  let { presence, onsettings }: { presence: PresenceController; onsettings?: () => void } = $props();

  let open = $state(false);
  let trigger = $state<HTMLButtonElement>();
  let choices = $state<HTMLFieldSetElement>();
  let pop = $state<HTMLDivElement>();
  /** Horizontal nudge that keeps the menu inside the window wherever the strip wrapped the trigger. */
  let shift = $state(0);
  const keepChoices: (number | null)[] = [null, ...PRESENCE_KEEP_FOR_CHOICES];

  let status = $derived(presence.status);
  let usable = $derived(presenceUsable(status));
  let problem = $derived(status ? presenceProblem(status) : null);

  $effect(() => {
    if (!open) return;
    const click = (e: MouseEvent): void => {
      if (!(e.target as HTMLElement | null)?.closest('.presence')) open = false;
    };
    const key = (e: KeyboardEvent): void => {
      if (e.key !== 'Escape') return;
      open = false;
      trigger?.focus();
    };
    document.addEventListener('click', click, true);
    document.addEventListener('keydown', key);
    return () => { document.removeEventListener('click', click, true); document.removeEventListener('keydown', key); };
  });

  $effect(() => {
    if (!open || !pop) return;
    const menu = pop;
    // Measured without the current nudge, and written once: this effect must
    // not read the state it writes.
    const fit = (): void => {
      const gutter = 8;
      const applied = untrack(() => shift);
      const box = menu.getBoundingClientRect();
      const left = box.left - applied;
      const right = box.right - applied;
      let next = 0;
      if (right > innerWidth - gutter) next = innerWidth - gutter - right;
      if (left + next < gutter) next = gutter - left;
      shift = next;
    };
    fit();
    addEventListener('resize', fit);
    return () => removeEventListener('resize', fit);
  });

  // A refused pick leaves the clicked radio checked in the DOM while the
  // committed status did not move; put the group back to what is true.
  async function pick(choice: PresenceChoice): Promise<void> {
    await presence.set(choice);
    for (const input of choices?.querySelectorAll('input') ?? []) input.checked = input.value === presence.status?.status;
  }
</script>

{#if status}
  <div class="presence">
    <button type="button" class="trigger" bind:this={trigger} aria-expanded={open} aria-controls="presence-menu" onclick={() => (open = !open)}>
      <PresenceMark kind={presenceMark(status)} />{presenceWord(status)}{#if presence.pending}…{/if} ▾
    </button>
    {#if open}
      <div id="presence-menu" class="pop" role="dialog" aria-label="Your warframe.market status" bind:this={pop} style:translate="{shift}px 0">
        <p class="head">What buyers see</p>
        <fieldset bind:this={choices} disabled={!usable || presence.pending !== null}>
          <legend class="visually-hidden">Status</legend>
          {#each PRESENCE_CHOICES as choice (choice)}
            <label class="opt">
              <input type="radio" name="presence-choice" value={choice} checked={status.status === choice} onchange={() => void pick(choice)}>
              <span class="txt"><b><PresenceMark kind={choice} />{PRESENCE_LABEL[choice]}</b><small>{PRESENCE_HINT[choice]}</small></span>
            </label>
          {/each}
        </fieldset>
        <div class="keep">
          <span class="k" id="presence-keep-label">Keep status for</span>
          <div class="ui-segmented xs" role="group" aria-labelledby="presence-keep-label">
            {#each keepChoices as minutes (minutes)}
              <button type="button" aria-pressed={!status.following && status.settings.keepForMinutes === minutes} disabled={status.following || presence.savingSettings} onclick={() => void presence.saveSettings({ ...status.settings, keepForMinutes: minutes })}>{keepForLabel(minutes)}</button>
            {/each}
          </div>
          {#if status.following}<small>Following the game keeps your status up for you.</small>{/if}
        </div>
        <hr>
        <label class="opt">
          <input type="checkbox" checked={status.settings.followGame} disabled={presence.savingSettings} onchange={(e) => void presence.saveSettings({ ...status.settings, followGame: e.currentTarget.checked })}>
          <span class="txt"><b>Follow the game</b><small>Online in game from login to quit. {PRESENCE_LABEL[status.settings.whenClosed]} when Warframe closes.</small></span>
        </label>
        <p class="now">{presenceLine(status)}</p>
        {#if status.followPaused}
          <div class="note">You picked a status, here or on warframe.market, so following waits for your next game session. <button type="button" class="btn xs" onclick={() => { trigger?.focus(); void presence.followNow(); }}>Follow the game now</button></div>
        {/if}
        {#if problem}<p class="note bad">{problem}</p>{/if}
        {#if presence.error}<p class="note bad" role="alert">{presence.error}</p>{/if}
        {#if onsettings}
          <hr>
          <button type="button" class="more" onclick={() => { open = false; onsettings?.(); }}>Presence settings →</button>
        {/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .presence { position: relative; display: inline-flex; }
  .trigger {
    display: inline-flex; align-items: center; gap: var(--s2); min-height: var(--ctl-xs); padding: 0 var(--s1);
    font: inherit; font-size: var(--text-caption); text-transform: uppercase; letter-spacing: 0.12em;
    color: var(--fg); background: transparent; border: 0; cursor: pointer; white-space: nowrap;
  }
  .trigger:hover { text-decoration: underline dotted; }
  .pop {
    position: absolute; top: calc(100% + var(--s2) + 1px); right: 0; z-index: var(--layer-menu);
    display: flex; flex-direction: column; width: min(19rem, calc(100vw - 2rem)); padding: var(--s2) 0;
    font: var(--text-control)/1.35 var(--font-body); text-transform: none; letter-spacing: normal; white-space: normal;
    color: var(--fg); background: var(--panel-2); border: 1px solid var(--fg); box-shadow: var(--shadow-pop);
  }
  .head, .k { margin: 0; padding: var(--s1) var(--s3) var(--s2); font: 600 var(--text-caption)/1.3 var(--font-ui); letter-spacing: 0.12em; text-transform: uppercase; color: var(--muted); }
  .k { padding: 0; }
  fieldset { margin: 0; padding: 0; border: 0; min-width: 0; }
  .opt { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--s2); align-items: start; padding: var(--s2) var(--s3); cursor: pointer; }
  .opt:hover { background: var(--panel); }
  fieldset:disabled .opt { cursor: not-allowed; }
  .opt input { margin-top: 2px; }
  .txt { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .txt b { display: flex; align-items: center; gap: var(--s2); font-weight: 500; }
  .txt small, .keep small { color: var(--muted); font-size: var(--text-caption); line-height: var(--leading-body); }
  .keep { display: flex; flex-direction: column; gap: var(--s1); padding: var(--s1) var(--s3) var(--s2); }
  hr { width: 100%; margin: var(--s1) 0; border: 0; border-top: 1px var(--rule) var(--hairline); }
  .now { margin: 0; padding: var(--s1) var(--s3); font-size: var(--text-caption); color: var(--muted); }
  .note { margin: var(--s1) var(--s3); padding-left: var(--s3); border-left: 3px solid var(--warn); font-size: var(--text-caption); line-height: var(--leading-body); color: var(--fg); }
  .note.bad { border-left-color: var(--bad); }
  .note .btn { margin-top: var(--s2); }
  .more { padding: var(--s2) var(--s3) var(--s1); text-align: left; font: inherit; font-size: var(--text-caption); color: var(--fg); background: transparent; border: 0; cursor: pointer; }
  .more:hover { text-decoration: underline dotted; }
  .visually-hidden { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
</style>
