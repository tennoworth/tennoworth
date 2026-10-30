<script lang="ts">
import { untrack } from 'svelte';
import { feedbackSnapshot, feedbackLink, feedbackProblems, improvementUrl } from './feedback';
import type { DesktopServices } from '../../contracts/services';
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
      <h3 data-shell id="feedback-title">Send feedback</h3>
      <p data-shell id="feedback-description">Help make TennoWorth more useful.</p>
    </header>
    <p data-shell class="feedback-note">What would you like to share?</p>
    <div data-shell class="feedback-options">
      <a data-shell href={bugReport?.url} aria-disabled={feedbackLoading} onclick={(event) => openFeedbackLink(event, bugReport?.url, feedbackLoading)} target="_blank" rel="noopener noreferrer">
        <strong data-shell>Report a bug <span data-shell aria-hidden="true">↗</span></strong>
        <span data-shell>Something broke or didn’t work as expected.</span>
      </a>
      <a data-shell href={improvementUrl} onclick={(event) => openFeedbackLink(event, improvementUrl, false, 'improvement')} target="_blank" rel="noopener noreferrer">
        <strong data-shell>Suggest an improvement <span data-shell aria-hidden="true">↗</span></strong>
        <span data-shell>Tell us what would make your next trade easier.</span>
      </a>
    </div>
    <label data-shell class="feedback-check"><input type="checkbox" bind:checked={includeFeedbackState} /> Include app-state snapshot with bug report</label>
    <p data-shell class="feedback-note">Version, operating system, current screen, scan, sign-in and update status, error categories, theme, and window size. No account identifiers, credentials, inventory contents, file paths, or game memory.</p>
    {#if reportedProblems.length}
      <div data-shell class="ui-notice feedback-problems" data-tone="warn" data-testid="feedback-problems">
        <strong data-shell>Included in this report</strong>
        <ul data-shell>
          {#each reportedProblems as problem (problem.area)}<li data-shell>{problem.area}: {problem.summary}.</li>{/each}
        </ul>
        <span data-shell>Only these categories are sent, not the error text.</span>
      </div>
    {/if}
    {#if includeFeedbackState && feedbackState}
      <details data-shell class="feedback-state">
        <summary data-shell>Review app-state snapshot</summary>
        <pre data-shell>{JSON.stringify(feedbackState, null, 2)}</pre>
      </details>
      <div data-shell class="ui-toolbar">
        <button data-shell type="button" class="btn" onclick={() => captureFeedback()} disabled={feedbackLoading}>Refresh snapshot</button>
        <button data-shell type="button" class="btn" onclick={downloadFeedback} disabled={feedbackLoading}>Download diagnostics</button>
      </div>
    {/if}
    {#if feedbackLoading}<p data-shell class="feedback-note" role="status">Reading app version…</p>{/if}
    {#if bugReport?.needsAttachment}<p data-shell class="feedback-note" role="status">This snapshot is too large to prefill. Download diagnostics, then attach the file to your GitHub report.</p>{/if}
    {#if feedbackDownloadError}<p data-shell role="alert">The download could not start. Copy the reviewed snapshot into the diagnostics field on GitHub.</p>{/if}
    <p data-shell class="feedback-note">Opens GitHub · Account required · Reports are public.</p>
    {#if feedbackFallbackUrl}
      <p data-shell role="alert">Couldn’t open your browser. Copy this link into your browser to continue.</p>
      <label data-shell>GitHub issue link<textarea data-shell class="ui-input" readonly value={feedbackFallbackUrl}></textarea></label>
    {/if}
    <footer data-shell><button data-shell type="submit" class="btn">Close</button></footer>
  </form>
</dialog>

