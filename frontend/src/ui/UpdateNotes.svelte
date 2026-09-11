<script lang="ts">
  import { onMount, tick } from 'svelte';
  import type { UpdateNotesServices, UpdateNotesStatus } from '../contracts/update';
  let { services, ready = false, blocked = false }: { services: UpdateNotesServices; ready?: boolean; blocked?: boolean } = $props();
  let data = $state<UpdateNotesStatus | null>(null);
  let dialog: HTMLDialogElement;
  let title = $state<HTMLHeadingElement>();
  let error = $state('');
  let saveError = $state(false);
  let loadEpoch = 0;
  let saveEpoch = 0;
  let requested = false;
  let pending = false;
  let disposed = false;
  let manual = $state(false);
  let expanded = $state(false);
  let returnFocus: HTMLElement | null = null;
  const ordinary = $derived(data?.changes.filter(change => change.kind !== 'action') ?? []);
  const visible = $derived(expanded ? ordinary : ordinary.slice(0, 6));
  const groups = $derived([
    { name: 'Action needed', kind: 'action', changes: data?.changes.filter(change => change.kind === 'action') ?? [] },
    { name: 'New & improved', kind: 'improved', changes: visible.filter(change => change.kind === 'improved') },
    { name: 'Fixes', kind: 'fixed', changes: visible.filter(change => change.kind === 'fixed') },
  ]);
  function unobstructed(manualRequest = false) {
    if (document.visibilityState !== 'visible' || (!manualRequest && !document.hasFocus())) return false;
    return ![...document.querySelectorAll('dialog[open], [role="dialog"], [aria-modal="true"]')]
      .some(element => element !== dialog && element.getClientRects().length > 0);
  }
  async function present() {
    const manualRequest = requested;
    if (disposed || pending || dialog?.open || !data || !unobstructed(manualRequest)) return;
    if (!manualRequest && (!data.auto_show || !ready || blocked)) return;
    pending = true;
    try {
      if (!manualRequest && !await services.updateNotesCanPresent()) return;
      if (disposed || !unobstructed(manualRequest) || (!manualRequest && (!ready || blocked))) return;
      manual = manualRequest;
      requested = false;
      expanded = false;
      returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      await tick();
      if (disposed || !unobstructed(manualRequest)) return;
      dialog.showModal();
      title?.focus();
    } catch { /* Optional notes must not interrupt startup when the native window is unavailable. */ }
    finally { pending = false; }
  }
  export async function open() {
    error = '';
    const epoch = ++loadEpoch;
    try {
      const value = await services.updateNotes();
      if (epoch !== loadEpoch || disposed) return;
      data = value; requested = true; await present();
    } catch { if (epoch === loadEpoch && !disposed) error = 'Release notes could not be loaded. You can try again from Settings.'; }
  }
  async function saveAcknowledgement() {
    if (!data) return;
    const epoch = ++saveEpoch;
    try { await services.acknowledgeUpdateNotes(data.current_version); if (epoch === saveEpoch) saveError = false; }
    catch { if (epoch === saveEpoch) saveError = true; }
  }
  function containFocus(event: KeyboardEvent) {
    if (event.key !== 'Tab') return;
    const controls = [...dialog.querySelectorAll<HTMLElement>('button:not(:disabled), a[href], summary, [tabindex="0"]')].filter(el => el.getClientRects().length > 0);
    const first = controls[0], last = controls.at(-1);
    const outside = !controls.includes(document.activeElement as HTMLElement);
    if (outside || (event.shiftKey ? document.activeElement === first : document.activeElement === last)) {
      event.preventDefault();
      (event.shiftKey ? last : first)?.focus();
    }
  }
  function dismiss() {
    if (!data) return;
    loadEpoch++;
    data = { ...data, auto_show: false };
    requested = false;
    dialog.close();
    if (returnFocus?.isConnected && returnFocus !== document.body) returnFocus.focus();
    void saveAcknowledgement();
  }
  onMount(() => {
    const epoch = ++loadEpoch;
    void services.updateNotes().then(value => { if (epoch === loadEpoch && !disposed) { data = value; void present(); } }).catch(() => {});
    const timer = window.setInterval(() => { void present(); }, 750);
    window.addEventListener('focus', present);
    document.addEventListener('visibilitychange', present);
    return () => { disposed = true; clearInterval(timer); window.removeEventListener('focus', present); document.removeEventListener('visibilitychange', present); };
  });
</script>

{#if error || saveError}
  <div class="notes-feedback">
    {#if error}<div class="ui-notice" role="status">{error} <button class="btn xs" onclick={open}>Retry</button></div>{/if}
    {#if saveError}<div class="ui-notice" data-tone="warn" role="status">Could not remember that you read these notes. They may appear after restarting. <button class="btn xs" onclick={saveAcknowledgement}>Retry saving</button></div>{/if}
    <button class="btn xs" onclick={() => { error = ''; saveError = false; }}>Dismiss notice</button>
  </div>
{/if}
<dialog bind:this={dialog} class="cryptobox update-notes" onkeydown={containFocus} aria-labelledby="update-notes-title" aria-describedby="update-notes-intro" oncancel={event => { event.preventDefault(); dismiss(); }}>
  {#if data}
    <header class="notes-header">
      <div class="status-row"><span class="status-label" class:installed={!manual && !data.earlier_version_unknown}>{!manual && !data.earlier_version_unknown ? '✓ Update installed' : 'Installed version'}</span><button class="btn ghost close-notes" aria-label="Close what’s new" onclick={dismiss}>×</button></div>
      <h2 id="update-notes-title" bind:this={title} tabindex="-1">What’s new</h2>
      <p id="update-notes-intro">{data.previous_version ? 'Here’s what changed since you last updated.' : 'Here’s what changed in this version.'} {#if data.releases.length > 1}Changes from all {data.releases.length} releases are brought together below.{/if}</p>
      <div class="version-strip"><span class="versions">{data.previous_version ? `v${data.previous_version} → v${data.current_version}` : `v${data.current_version}`}</span><span>{data.releases.length} {data.releases.length === 1 ? 'release' : 'releases'} included</span></div>
    </header>
    <div class="notes-content">
      {#if data.earlier_version_unknown}<p class="ui-notice">Your earlier app version wasn’t recorded. These notes cover the installed release.</p>{/if}
      {#if data.partial_history}<p class="ui-notice" data-tone="warn">Some earlier release notes aren’t bundled with this version. <a href="https://github.com/tennoworth/tennoworth/releases" target="_blank" rel="noopener noreferrer">Read the full release history</a>.</p>{/if}
      {#each groups as group}
        {#if group.changes.length}
          <section class="change-group" class:action-needed={group.kind === 'action'}>
            <h3>{group.name}</h3>
            {#each group.changes as change (change.id)}
              <article class="change"><h4>{change.title}{#if change.platforms.length === 1}<span class="platform">{change.platforms[0] === 'linux' ? 'Linux' : 'Windows'}</span>{/if}</h4><p>{change.body}</p></article>
            {/each}
          </section>
        {/if}
      {/each}
      {#if !data.changes.length}<p>No changes specific to your platform are listed for this release.</p>{/if}
      {#if ordinary.length > 6}<button class="btn ghost show-all" onclick={() => expanded = !expanded}>{expanded ? 'Show highlights' : `Show all changes (${data.changes.length})`}</button>{/if}
      <details class="history"><summary>See changes by version ({data.releases.length})</summary>
        {#each data.releases as release (release.version)}<section class="release"><h3>v{release.version} <time datetime={release.date}>{release.date}</time></h3>{#if release.changes.length}<ul>{#each release.changes as change}<li><strong>{change.title}.</strong> {change.body}</li>{/each}</ul>{:else}<p>No changes specific to your platform.</p>{/if}</section>{/each}
      </details>
    </div>
    <footer class="notes-footer"><p>You can read this again in<br><strong>Settings → Updates → What’s new.</strong></p><button class="btn primary" onclick={dismiss}>Got it</button></footer>
  {/if}
</dialog>

<style>
  dialog.update-notes { max-width:47rem; }
  :global(html[data-look='yorha']) dialog.update-notes .notes-header { display:block; margin:0; background:var(--panel); color:var(--fg); padding:var(--s5) var(--s6) 0; }
  :global(html[data-look='yorha']) dialog.update-notes .notes-header :focus-visible { outline-color:var(--accent); }
  .status-row { display:flex; align-items:center; justify-content:space-between; gap:var(--s3); }
  .status-label { font-size:var(--text-control); color:var(--muted); }
  .status-label.installed { color:var(--good); }
  .close-notes { min-width:var(--ctl-lg); min-height:var(--ctl-lg); font-size:var(--text-heading); }
  #update-notes-title { font:600 var(--text-dialog-title)/var(--leading-control) var(--font-ui); margin:var(--s3) 0; outline:none; }
  :global(html[data-look='yorha']) dialog.update-notes #update-notes-intro { margin:0 0 var(--s4); color:var(--muted); }
  .version-strip { display:flex; flex-wrap:wrap; gap:var(--s2) var(--s3); padding:var(--s3) 0; border-block:1px var(--rule) var(--hairline); font-size:var(--text-control); color:var(--muted); }
  .versions { font-family:var(--font-mono); font-size:var(--text-caption); color:var(--fg); }
  .notes-content { padding:var(--s5) var(--s6); }
  .change-group + .change-group { margin-top:var(--s5); }
  dialog.update-notes .change-group > h3 { margin:0 0 var(--s4); font:600 var(--text-caption) var(--font-ui); letter-spacing:.14em; text-transform:uppercase; color:var(--muted); }
  .change { padding-left:var(--s4); border-left:2px solid var(--border); margin-bottom:var(--s4); }
  .change:last-child { margin-bottom:0; }
  .action-needed .change { border-color:var(--warn); }
  .change h4 { margin:0 0 var(--s1); font:600 var(--text-body)/var(--leading-body) var(--font-body); }
  .change p, .release p { margin:0; color:var(--muted); font-size:var(--text-control); }
  .platform { margin-left:var(--s2); font:var(--text-caption) var(--font-mono); color:var(--muted); }
  .history { margin-top:var(--s5); border-top:1px var(--rule) var(--hairline); }
  .history summary { min-height:var(--ctl-lg); padding:var(--s3) 0; cursor:pointer; font-size:var(--text-control); }
  .release { padding:var(--s4) 0; border-top:1px var(--rule) var(--hairline); }
  .release h3 { display:flex; flex-wrap:wrap; gap:var(--s3); margin:0 0 var(--s2); font:500 var(--text-control) var(--font-mono); }
  .release time { color:var(--muted); font-family:var(--font-body); }
  .release ul { padding-left:var(--s5); margin:0; }
  .release li { margin:var(--s2) 0; color:var(--muted); font-size:var(--text-control); }
  .notes-footer { position:sticky; bottom:0; display:flex; flex-wrap:wrap; justify-content:space-between; align-items:center; gap:var(--s3); padding:var(--s4) var(--s6); background:var(--panel-2); border-top:1px solid var(--border); }
  .notes-footer p { margin:0; max-width:35ch; color:var(--muted); font-size:var(--text-caption); }
  .notes-footer .btn { min-height:var(--ctl-lg); min-width:7rem; }
  .ui-notice { margin-bottom:var(--s4); }
  .notes-feedback { position:fixed; bottom:var(--s4); right:var(--s4); z-index:var(--layer-toast); max-width:32rem; width:calc(100% - 2 * var(--s4)); background:var(--panel); border:1px solid var(--border); padding:var(--s3); }
  .show-all { margin-top:var(--s4); }
  @media(max-width:600px) { :global(html[data-look='yorha']) dialog.update-notes .notes-header { padding:var(--s4) var(--s4) 0; } .notes-content { padding:var(--s4); } .notes-footer { padding:var(--s3) var(--s4); } }
  @media(max-height:550px) { .notes-footer { position:static; } }
</style>
