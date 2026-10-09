<script lang="ts">
  // Settings → This app → Appearance → App icon. One radio card per choice,
  // each previewing its compact mark on the light and the dark ground, since
  // the point of the choice is how it reads on the user's taskbar.
  import type { AppIconChoice, AppIconController } from './app-icon.svelte';

  let { appIcon }: { appIcon: AppIconController } = $props();

  const OPTIONS: { id: AppIconChoice; name: string; hint: string }[] = [
    { id: 'blue', name: 'Classic blue', hint: 'Default. Visible on any taskbar.' },
    { id: 'match', name: 'Match colour mode', hint: 'Ink in Light, rag in Dark.' },
    { id: 'ink', name: 'Ink', hint: 'Hard to see on dark taskbars.' },
    { id: 'rag', name: 'Rag', hint: 'Hard to see on light taskbars.' },
  ];
</script>

<div class="ui-setting-row app-icon">
  <div class="ui-setting-copy">
    <strong id="app-icon-label">App icon</strong>
    <p id="app-icon-help">The icon on this window, the taskbar and the tray. Your desktop shortcut and the installer keep the classic icon.</p>
    {#if !appIcon.windowIcon}
      <p>On this Wayland session the window and taskbar show the installed icon; only the tray follows this setting.</p>
    {/if}
  </div>
  <div class="options" role="radiogroup" aria-labelledby="app-icon-label" aria-describedby="app-icon-help">
    {#each OPTIONS as option (option.id)}
      <label class="option" class:selected={appIcon.choice === option.id}>
        <input
          type="radio"
          name="app-icon"
          value={option.id}
          checked={appIcon.choice === option.id}
          onchange={() => void appIcon.select(option.id)}
        />
        <img src={`/app-icon/${option.id}.svg`} alt="" />
        <span class="text">
          <span class="name">{option.name}</span>
          <span class="hint">{option.hint}</span>
        </span>
      </label>
    {/each}
  </div>
  {#if appIcon.error}<p class="error" role="alert">Couldn’t change the app icon: {appIcon.error}</p>{/if}
</div>

<style>
  /* The cards need the row's full width; the copy sits above them. */
  .app-icon { grid-template-columns: minmax(0, 1fr); }
  .options { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: var(--s2); min-width: 0; }
  @media (max-width: 760px) { .options { grid-template-columns: minmax(0, 1fr); } }
  .option {
    display: flex; align-items: center; gap: var(--s3); min-width: 0;
    padding: var(--s3); border: 1px solid var(--border); border-radius: var(--radius-panel);
    cursor: pointer;
  }
  .option:hover { background: var(--hover); }
  /* Selected is the ink inversion, as on every other mode card. The radio,
     hint and focus ring invert with it, or the check would vanish into the
     fill; the previews keep their own light and dark grounds. */
  .option.selected, .option.selected:hover { background: var(--ink-bar); border-color: var(--ink-bar); color: var(--on-ink); }
  .option.selected .hint { color: var(--on-ink-muted); }
  .option.selected input[type="radio"] { border-color: var(--on-ink); }
  .option.selected input[type="radio"]:checked::before { background: var(--on-ink); }
  .option.selected input[type="radio"]:focus-visible { outline-color: var(--on-ink); }
  img { flex: 0 0 auto; width: 4.5rem; height: 2.25rem; border: 1px solid var(--hairline); }
  .text { display: flex; flex-direction: column; min-width: 0; }
  .name { font: 600 var(--text-control)/var(--leading-control) var(--font-ui); }
  .hint { font-size: var(--text-caption); line-height: var(--leading-body); color: var(--muted); }
  .error { margin: 0; color: var(--bad); font-size: var(--text-control); }
</style>
