<script lang="ts">
  import { roving } from '../../ui/keyboard-actions';
  import { t } from '../../i18n/i18n.svelte';
  import Rich from '../../ui/Rich.svelte';
  // Steps for copying the warframe.market `JWT` cookie out of the user's own
  // browser - the fallback sign-in when the in-app window cannot load WFM.
  // Text steps rather than screenshots or drawings: browser devtools are
  // redesigned often, and a picture of the wrong layout misleads more than a
  // step that names the tab.
  type Browser = 'firefox' | 'chromium';
  const STEPS = {
    firefox: ['guide.ff1', 'guide.ff2', 'guide.ff3'],
    chromium: ['guide.cr1', 'guide.cr2', 'guide.cr3'],
  } as const;
  let browser = $state<Browser>(
    typeof navigator !== 'undefined' && /Windows/.test(navigator.userAgent) ? 'chromium' : 'firefox',
  );
</script>

<div class="token-guide" data-testid="wfm-token-guide">
  <div class="ui-segmented xs" role="group" aria-label={t('guide.browser')} use:roving>
    <button type="button" aria-pressed={browser === 'firefox'} onclick={() => (browser = 'firefox')}>Firefox</button>
    <button type="button" aria-pressed={browser === 'chromium'} onclick={() => (browser = 'chromium')}>Chrome / Edge</button>
  </div>
  <ol>
    <li>
      <a href="https://warframe.market/auth/signin" target="_blank" rel="noopener noreferrer">{t('guide.open')}</a>{t('guide.openAfter')}
    </li>
    {#each STEPS[browser] as step (step)}
      <li><Rich text={t(step)} /></li>
    {/each}
    <li><Rich text={t('guide.paste', { field: t('auth.sessionToken') })} /></li>
  </ol>
  <p class="muted">{t('guide.note')}</p>
</div>

<style>
  .token-guide { display: flex; flex-direction: column; gap: var(--s2); }
  .token-guide .ui-segmented { align-self: flex-start; }
  .token-guide ol {
    margin: 0;
    padding-left: var(--s5);
    font-family: var(--font-body);
    font-size: var(--text-control);
    line-height: var(--leading-body);
  }
  .token-guide li + li { margin-top: var(--s1); }
  .token-guide p { margin: 0; }
  .token-guide :global(kbd) {
    font-family: var(--font-mono);
    font-size: var(--text-caption);
    padding: 0 var(--s1);
    border: 1px solid var(--border);
    border-radius: var(--radius-input);
    background: var(--panel-2);
    color: var(--fg);
  }
</style>
