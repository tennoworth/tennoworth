<script lang="ts">
import { untrack } from 'svelte';
import { feedbackSnapshot, feedbackLink, feedbackProblems, improvementUrl } from './feedback';
import type { DesktopServices } from '../../contracts/services';
import { t } from '../../i18n/i18n.svelte';
let { captureState, services, onclosed }: { captureState: () => Parameters<typeof feedbackSnapshot>[0]; services: Pick<DesktopServices, 'updateDiagnostics' | 'updateStatus' | 'desktopOpenExternalUrl'>; onclosed: () => void } = $props();
const { updateDiagnostics, updateStatus, desktopOpenExternalUrl } = untrack(() => services);
  let feedbackDialog: HTMLDialogElement;
  let feedbackState = $state<ReturnType<typeof feedbackSnapshot> | null>(null);
  let includeFeedbackState = $state(true);
  let feedbackLoading = $state(false);
  let feedbackDownloadError = $state(false);
  let feedbackGeneration = 0;
  let feedbackFallbackKind = $state<'bug' | 'improvement' | null>(null);
  let bugReport = $derived(feedbackState ? feedbackLink(feedbackState, includeFeedbackState) : null);
  let reportedProblems = $derived(feedbackState && includeFeedbackState ? feedbackProblems(feedbackState) : []);

  let feedbackFallbackUrl = $derived(feedbackFallbackKind === 'bug' ? bugReport?.url : feedbackFallbackKind === 'improvement' ? improvementUrl : null);

  async function captureFeedback() {
    const generation = ++feedbackGeneration;
    feedbackLoading = true;
    feedbackDownloadError = false;
    const state = captureState();
    const operation = updateDiagnostics();
    feedbackState = feedbackSnapshot(state, null, operation);
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      const status = await Promise.race([
        updateStatus(),
        new Promise<null>(resolve => { timer = setTimeout(() => resolve(null), 1500); }),
      ]);
      if (generation === feedbackGeneration) feedbackState = feedbackSnapshot(state, status, operation);
    } catch {
      // Feedback still works if the IPC bridge is unavailable.
    } finally {
      clearTimeout(timer);
      if (generation === feedbackGeneration) feedbackLoading = false;
    }
  }

  export function openFeedback() {
    feedbackFallbackKind = null;
    feedbackDialog.showModal();
    void captureFeedback();
  }

  async function openFeedbackLink(event: MouseEvent, url: string | undefined, loading = false, kind: 'bug' | 'improvement' = 'bug') {
    event.preventDefault();
    if (loading || !url) return;
    feedbackFallbackKind = null;
    try {
      if (!(await desktopOpenExternalUrl(url))) feedbackFallbackKind = kind;
    } catch {
      feedbackFallbackKind = kind;
    }
  }

  function downloadFeedback() {
    if (!feedbackState || !includeFeedbackState) return;
    feedbackDownloadError = false;
    try {
      const url = URL.createObjectURL(new Blob([JSON.stringify(feedbackState, null, 2)], { type: 'application/json' }));
      const link = document.createElement('a');
      link.href = url;
      link.download = 'tennoworth-diagnostics.json';
      document.body.appendChild(link);
      link.click();
      link.remove();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
    } catch {
      feedbackDownloadError = true;
    }
  }

</script>
<!-- Opened from the first-run More menu, the trigger is gone by the time the
     dialog closes; focus returns to the menu button instead of the page. -->
<dialog data-shell bind:this={feedbackDialog} class="cryptobox feedback-dialog" aria-labelledby="feedback-title" aria-describedby="feedback-description" onclose={onclosed}>
  <form data-shell method="dialog">
    <header data-shell>
      <h3 data-shell id="feedback-title">{t('feedback.title')}</h3>
      <p data-shell id="feedback-description">{t('feedback.lede')}</p>
    </header>
    <p data-shell class="feedback-note">{t('feedback.ask')}</p>
    <div data-shell class="feedback-options">
      <a data-shell href={bugReport?.url} aria-disabled={feedbackLoading} onclick={(event) => openFeedbackLink(event, bugReport?.url, feedbackLoading)} target="_blank" rel="noopener noreferrer">
        <strong data-shell>{t('feedback.bug')} <span data-shell aria-hidden="true">↗</span></strong>
        <span data-shell>{t('feedback.bugHelp')}</span>
      </a>
      <a data-shell href={improvementUrl} onclick={(event) => openFeedbackLink(event, improvementUrl, false, 'improvement')} target="_blank" rel="noopener noreferrer">
        <strong data-shell>{t('feedback.idea')} <span data-shell aria-hidden="true">↗</span></strong>
        <span data-shell>{t('feedback.ideaHelp')}</span>
      </a>
    </div>
    <label data-shell class="feedback-check"><input type="checkbox" bind:checked={includeFeedbackState} /> {t('feedback.include')}</label>
    <p data-shell class="feedback-note">{t('feedback.contents')}</p>
    {#if reportedProblems.length}
      <div data-shell class="ui-notice feedback-problems" data-tone="warn" data-testid="feedback-problems">
        <strong data-shell>{t('feedback.included')}</strong>
        <ul data-shell>
          {#each reportedProblems as problem (problem.area)}<li data-shell>{problem.area}: {problem.summary}.</li>{/each}
        </ul>
        <span data-shell>{t('feedback.categoriesOnly')}</span>
      </div>
    {/if}
    {#if includeFeedbackState && feedbackState}
      <details data-shell class="feedback-state">
        <summary data-shell>{t('feedback.review')}</summary>
        <pre data-shell>{JSON.stringify(feedbackState, null, 2)}</pre>
      </details>
      <div data-shell class="ui-toolbar">
        <button data-shell type="button" class="btn" onclick={() => captureFeedback()} disabled={feedbackLoading}>{t('feedback.refresh')}</button>
        <button data-shell type="button" class="btn" onclick={downloadFeedback} disabled={feedbackLoading}>{t('feedback.download')}</button>
      </div>
    {/if}
    {#if feedbackLoading}<p data-shell class="feedback-note" role="status">{t('feedback.reading')}</p>{/if}
    {#if bugReport?.needsAttachment}<p data-shell class="feedback-note" role="status">{t('feedback.tooLarge')}</p>{/if}
    {#if feedbackDownloadError}<p data-shell role="alert">{t('feedback.downloadFailed')}</p>{/if}
    <p data-shell class="feedback-note">{t('feedback.github')}</p>
    {#if feedbackFallbackUrl}
      <p data-shell role="alert">{t('feedback.noBrowser')}</p>
      <label data-shell>{t('feedback.link')}<textarea data-shell class="ui-input" readonly value={feedbackFallbackUrl}></textarea></label>
    {/if}
    <footer data-shell><button data-shell type="submit" class="btn">{t('feedback.close')}</button></footer>
  </form>
</dialog>

