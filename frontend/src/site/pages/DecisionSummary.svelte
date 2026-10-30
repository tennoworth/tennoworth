<script lang="ts">
  import { decisionText, formatStampUtc, priceText, type DecisionAnswer } from './model';

  let { answer, buildStamp, enhanced }: {
    answer: DecisionAnswer;
    /** The snapshot the page and its social metadata were built from. */
    buildStamp: string;
    enhanced: boolean;
  } = $props();

  const text = $derived(decisionText(answer));
  let copyState = $state<'idle' | 'copied' | 'error'>('idle');
  let copiedStamp = $state<string | null>(null);
  let copyAttempt = 0;
  let textDetails: HTMLDetailsElement;
  const status = $derived(
    copyState === 'error' ? 'Couldn’t copy. Select the decision text below and copy it manually.'
      : copyState === 'copied' ? (copiedStamp === answer.stamp ? 'Decision copied.' : 'Snapshot changed; copy again.')
      : ''
  );
  const stale = $derived(answer.stamp !== buildStamp);

  function legacyCopy(value: string, restore: HTMLElement | null): boolean {
    try {
      const field = document.createElement('textarea');
      field.value = value;
      field.style.position = 'fixed';
      field.style.opacity = '0';
      document.body.appendChild(field);
      field.select();
      try { return document.execCommand('copy'); } finally { field.remove(); restore?.focus(); }
    } catch {
      return false;
    }
  }

  async function copyDecision(): Promise<void> {
    if (!enhanced) return;
    const attempt = ++copyAttempt;
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const clicked = { stamp: answer.stamp, text: text.clipboard };
    let copied = false;
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(clicked.text);
        copied = true;
      } else {
        copied = legacyCopy(clicked.text, previous);
      }
    } catch {
      copied = legacyCopy(clicked.text, previous);
    }
    if (attempt !== copyAttempt) return;
    if (!copied) {
      copyState = 'error';
      if (textDetails) textDetails.open = true;
      return;
    }
    copiedStamp = clicked.stamp;
    copyState = 'copied';
  }
</script>

<section class="decision" aria-labelledby="decision-title">
  <div class="decision-head">
    <span class="decision-kicker">Set or parts?</span>
    <span class="decision-stamp">Snapshot <time datetime={answer.stamp}>{formatStampUtc(answer.stamp)}</time> · {answer.platform}</span>
  </div>
  <h2 id="decision-title" class="decision-title">{text.headline}</h2>
  <p class="decision-quantities">{text.quantities}</p>
  <div class="decision-numbers">
    <div class="decision-number">
      <span>Complete set</span>
      <strong>{priceText(answer.setPrice)}</strong>
      <small>1 set</small>
    </div>
    <div class="decision-number">
      <span>Summed parts</span>
      <strong>{priceText(answer.partsTotal)}</strong>
      <small>{answer.componentCopies ?? 'Unknown'} component {answer.componentCopies === 1 ? 'copy' : 'copies'} · {answer.distinctComponents} {answer.distinctComponents === 1 ? 'item' : 'items'}</small>
    </div>
  </div>
  <div class="decision-actions">
    <button type="button" class="btn primary" disabled={!enhanced} onclick={copyDecision}>Copy decision</button>
    <a class="btn" href={answer.setOrdersUrl} target="_blank" rel="noopener noreferrer">Check current set orders</a>
    <a class="btn ghost" href="/desktop/">Personalize in the app</a>
  </div>
  <p class="decision-handoff">Windows or Linux. Install the desktop app to compare against your locally scanned inventory.</p>
  <p class="decision-evidence">{text.evidence}</p>
  <p class="decision-status" aria-live="polite">{status}</p>
  {#if stale}<p class="decision-stale">Updated since this page was built ({formatStampUtc(buildStamp)}). Shared previews may show the older snapshot.</p>{/if}
  <details class="decision-text" bind:this={textDetails}>
    <summary>Decision text</summary>
    <pre>{text.clipboard}</pre>
  </details>
  <a class="decision-table-link" href="#set-vs-parts">Component orders in the table below</a>
</section>

<style>
  .decision { border: 1px solid var(--border); background: var(--panel-2); padding: var(--s3) var(--inset); display: flex; flex-direction: column; gap: var(--s2); margin-block: 0 var(--s4); }
  .decision-head { display: flex; justify-content: space-between; gap: var(--s2); align-items: baseline; border-bottom: 1px solid var(--border); padding-bottom: var(--s2); flex-wrap: wrap; }
  .decision-kicker { font: 500 var(--text-caption) var(--font-ui); letter-spacing: .12em; text-transform: uppercase; color: var(--muted); }
  .decision-stamp { color: var(--muted); font: var(--text-caption) var(--font-mono); }
  .decision-title { margin: 0 !important; }
  .decision-quantities { margin: 0; color: var(--muted); font-size: var(--text-control); }
  .decision-numbers { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: var(--s2); }
  .decision-number { border: 1px solid var(--border); background: var(--panel); padding: var(--s2) var(--s3); display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .decision-number span { color: var(--muted); font: 500 var(--text-caption) var(--font-ui); letter-spacing: .08em; text-transform: uppercase; }
  .decision-number strong { font: 600 var(--text-metric-lg) var(--font-mono); }
  .decision-number small { color: var(--muted); font-size: var(--text-caption); overflow-wrap: anywhere; }
  .decision-evidence { margin: 0; color: var(--muted); font-size: var(--text-caption); line-height: var(--leading-body); max-width: 80ch; }
  .decision-actions { display: flex; flex-wrap: wrap; gap: var(--s2); align-items: center; }
  .decision-handoff { margin: 0; color: var(--muted); font-size: var(--text-caption); }
  .decision-status { margin: 0; min-height: 1.25em; color: var(--muted); font-size: var(--text-caption); }
  .decision-stale { margin: 0; color: var(--warn); font-size: var(--text-caption); }
  .decision-text { color: var(--muted); font-size: var(--text-caption); }
  .decision-text summary { cursor: pointer; font-family: var(--font-ui); letter-spacing: .04em; }
  .decision-text pre { margin: var(--s2) 0 0; padding: var(--s2); border: 1px solid var(--border); background: var(--panel); color: var(--fg); font: var(--text-caption)/var(--leading-body) var(--font-mono); white-space: pre-wrap; overflow-wrap: anywhere; }
  .decision-table-link { align-self: flex-start; color: var(--muted); font-size: var(--text-caption); }
  @media (max-width: 520px) {
    .decision-actions .btn { flex: 1 1 auto; }
    .decision-numbers { grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); }
  }
</style>
