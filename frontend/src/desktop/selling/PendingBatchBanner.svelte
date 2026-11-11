<script lang="ts">
import type { ListingController, WfmAccessController } from './controller.svelte';
import { interruptedBatch } from '../../domain/listing-plan';
import { shortStamp } from '../../ui/format';
import { t } from '../../i18n/i18n.svelte';
  let { listing, marketAccess, onorders }: {
    listing: ListingController;
    marketAccess: WfmAccessController;
    onorders: () => void;
  } = $props();
let batch = $derived(interruptedBatch(listing.pendingPlan));
let uncertainRemaining = $derived(batch?.uncertain ?? 0);
let resumeOk = $derived(listing.resumeResults.filter(r => r.status === 'ok').length);
// Every kind the batch holds is named: a batch can hold pending and
// unconfirmed work at once, and the banner must not hide either.
let detail = $derived(batch ? `· ${[
  batch.pending > 0 ? t('listing.pending', { count: batch.pending }) : null,
  batch.uncertain > 0 ? t('listing.uncertain', { count: batch.uncertain }) : null,
  batch.done > 0 ? t('listing.alreadyDone', { count: batch.done }) : null,
].filter(Boolean).join(', ')}` : '');
let resumeErr = $derived(listing.resumeResults.filter(r => r.status !== 'ok').length);
</script>

  {#if listing.pendingPlan || listing.resumePhase !== 'idle'}
    <section data-shell class="card ui-panel pending-banner">
      {#if listing.resumePhase === 'running'}
        <div data-shell class="row">
          <div data-shell class="src">
            <span data-shell class="dot aging" aria-hidden="true"></span>
            <strong data-shell>{t('listing.resuming')}</strong>
            <span data-shell class="muted">{t('listing.resumingNote')}</span>
          </div>
        </div>
      {:else if listing.resumePhase === 'done'}
        <div data-shell class="row">
          <div data-shell class="src">
            <span data-shell class="dot fresh" aria-hidden="true"></span>
            <strong data-shell>{t('listing.resumed')}</strong>
            <span data-shell class="muted">
              <span data-shell class="ok-text">{t('listing.created', { count: resumeOk })}</span>{#if resumeErr > 0}{' · '}<span data-shell class="bad">{t('listing.failed', { count: resumeErr })}</span>{/if}.
              {t('listing.stillHidden')}
            </span>
          </div>
          <div data-shell class="row gap-sm">
            <button data-shell class="ghost" onclick={() => { listing.resumePhase = 'idle'; listing.resumeResults = []; }}>{t('listing.dismiss')}</button>
          </div>
        </div>
      {:else if listing.resumePhase === 'error'}
        <div data-shell class="row">
          <div data-shell class="src">
            <span data-shell class="dot stale" aria-hidden="true"></span>
            <strong data-shell>{t('listing.resumeFailed')}</strong>
            <span data-shell class="muted bad">{listing.resumeError}</span>
          </div>
          <div data-shell class="row gap-sm">
            <button data-shell onclick={() => listing.doResume()} disabled={marketAccess.mutationsBlocked}>{t('listing.retry')}</button>
            <button data-shell class="ghost" onclick={() => listing.doDiscard()}>{t('listing.discardPending')}</button>
          </div>
        </div>
      {:else if listing.pendingPlan && batch}
        <div data-shell class="row">
          <div data-shell class="src">
            <span data-shell class="dot aging" aria-hidden="true"></span>
            <strong data-shell>{t('listing.interruptedFrom', { time: shortStamp(listing.pendingPlan.started_at) })}</strong>
            {#if listing.durabilityError}<span data-shell class="bad" role="alert">{listing.durabilityError}</span>{/if}
            <span data-shell class="muted">{detail}</span>
            {#if uncertainRemaining > 0}
              <span data-shell class="muted">
                {t('listing.mayBeLive')}
              </span>
            {/if}
          </div>
          <div data-shell class="row gap-sm">
            {#if batch.resumable}
              <button data-shell onclick={() => listing.doResume()} disabled={marketAccess.mutationsBlocked}>{t('listing.resume')}</button>
            {/if}
            {#if !batch.resumable || uncertainRemaining > 0}
              <button data-shell onclick={onorders}>{t('listing.reviewOrders')}</button>
            {/if}
            <button data-shell class="ghost" onclick={() => listing.doDiscard()}>{t('listing.discard')}</button>
          </div>
        </div>
      {:else if listing.pendingPlan}
        <div data-shell class="row">
          <div data-shell class="src">
            <span data-shell class="dot stale" aria-hidden="true"></span>
            <strong data-shell>{t('listing.completedSaved')}</strong>
            <span data-shell class="muted">{listing.durabilityError ?? t('listing.notCleared')}</span>
          </div>
          <div data-shell class="row gap-sm">
            <button data-shell onclick={() => listing.doResume()}>{t('listing.retrySaving')}</button>
            <button data-shell onclick={onorders}>{t('listing.reviewOrders')}</button>
            <button data-shell class="ghost" onclick={() => listing.doDiscard()}>{t('listing.discardRecord')}</button>
          </div>
        </div>
      {/if}
    </section>
  {/if}
