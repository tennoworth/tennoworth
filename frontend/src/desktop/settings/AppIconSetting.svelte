<script lang="ts">
  import { t } from '../../i18n/i18n.svelte';
  // Settings → This app → Appearance → App icon. One radio card per choice,
  // each previewing its compact mark on the light and the dark ground, since
  // the point of the choice is how it reads on the user's taskbar.
  import type { AppIconChoice, AppIconController } from './app-icon.svelte';

  let { appIcon }: { appIcon: AppIconController } = $props();

  const OPTIONS: AppIconChoice[] = ['blue', 'match', 'ink', 'rag'];
</script>

<div class="ui-setting-row app-icon">
  <div class="ui-setting-copy">
    <strong id="app-icon-label">{t('icon.title')}</strong>
    <p id="app-icon-help">{t('icon.help')}</p>
    {#if !appIcon.windowIcon}
      <p>{t('icon.wayland')}</p>
    {/if}
  </div>
  <div class="options" role="radiogroup" aria-labelledby="app-icon-label" aria-describedby="app-icon-help">
    {#each OPTIONS as option (option)}
      <label class="option" class:selected={appIcon.choice === option}>
        <input
          type="radio"
          name="app-icon"
          value={option}
          checked={appIcon.choice === option}
          onchange={() => void appIcon.select(option)}
        />
        <img src={`/app-icon/${option}.svg`} alt="" />
        <span class="text">
          <span class="name">{t(`icon.${option}`)}</span>
          <span class="hint">{t(`icon.${option}.hint`)}</span>
        </span>
      </label>
    {/each}
  </div>
  {#if appIcon.error}<p class="error" role="alert">{t('icon.failed', { error: appIcon.error })}</p>{/if}
</div>

<style>
  /* The cards need the row's full width; the copy sits above them. */
  .app-icon { grid-template-columns: minmax(0, 1fr); }
  .options { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: var(--s2); min-width: 0; }
  @media (max-width: 760px) { .options { grid-template-columns: minmax(0, 1fr); } }
  .option {
    display: flex; flex-wrap: wrap; align-items: center; gap: var(--s3); min-width: 0;
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
  /* WebKitGTK draws no ring on these restyled radios, so the card shows it. */
  .option:has(input:focus-visible) { outline: 2px solid var(--accent); outline-offset: 2px; }
  img { flex: 0 0 auto; width: 4.5rem; height: 2.25rem; border: 1px solid var(--hairline); }
  .text { display: flex; flex: 1 1 8rem; flex-direction: column; min-width: 0; max-width: 100%; overflow-wrap: anywhere; }
  .name { font: 600 var(--text-control)/var(--leading-control) var(--font-ui); }
  .hint { font-size: var(--text-caption); line-height: var(--leading-body); color: var(--muted); }
  .error { margin: 0; color: var(--bad); font-size: var(--text-control); }
</style>
