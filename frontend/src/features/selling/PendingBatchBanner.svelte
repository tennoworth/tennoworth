<script lang="ts">
import type { ListingController, WfmAccessController } from './controller.svelte';
import { interruptedBatch } from '../../domain/listing-plan';
  let { listing, marketAccess, onorders }: {
    listing: ListingController;
    marketAccess: WfmAccessController;
    onorders: () => void;
  } = $props();
let batch = $derived(interruptedBatch(listing.pendingPlan));
let uncertainRemaining = $derived(batch?.uncertain ?? 0);
let resumeOk = $derived(listing.resumeResults.filter(r => r.status === 'ok').length);
let resumeErr = $derived(listing.resumeResults.filter(r => r.status !== 'ok').length);
</script>

  {#if listing.pendingPlan || listing.resumePhase !== 'idle'}
    <section data-shell class="card ui-panel pending-banner">
      {#if listing.resumePhase === 'running'}
        <div data-shell class="row">
          <div data-shell class="src">
            <span data-shell class="dot aging" aria-hidden="true"></span>
            <strong data-shell>Resuming interrupted batch…</strong>
            <span data-shell class="muted">Checking current orders before continuing.</span>
          </div>
        </div>
      {:else if listing.resumePhase === 'done'}
        <div data-shell class="row">
          <div data-shell class="src">
            <span data-shell class="dot fresh" aria-hidden="true"></span>
            <strong data-shell>Resumed.</strong>
            <span data-shell class="muted">
              <span data-shell class="ok-text">{resumeOk} created</span>
              {#if resumeErr > 0}· <span data-shell class="bad">{resumeErr} failed</span>{/if}.
              New listings are still hidden - toggle from the orders panel.
            </span>
          </div>
          <div data-shell class="row gap-sm">
            <button data-shell class="ghost" onclick={() => { listing.resumePhase = 'idle'; listing.resumeResults = []; }}>Dismiss</button>
          </div>
        </div>
      {:else if listing.resumePhase === 'error'}
        <div data-shell class="row">
          <div data-shell class="src">
            <span data-shell class="dot stale" aria-hidden="true"></span>
            <strong data-shell>Resume failed.</strong>
            <span data-shell class="muted bad">{listing.resumeError}</span>
          </div>
          <div data-shell class="row gap-sm">
            <button data-shell onclick={() => listing.doResume()} disabled={marketAccess.mutationsBlocked}>Retry</button>
            <button data-shell class="ghost" onclick={() => listing.doDiscard()}>Discard pending</button>
          </div>
        </div>
      {:else if listing.pendingPlan && batch}
        <div data-shell class="row">
          <div data-shell class="src">
            <span data-shell class="dot aging" aria-hidden="true"></span>
            <strong data-shell>Interrupted batch from {new Date(listing.pendingPlan.started_at).toLocaleString()}</strong>
            {#if listing.durabilityError}<span data-shell class="bad" role="alert">{listing.durabilityError}</span>{/if}
            <span data-shell class="muted">{batch.detail}</span>
            {#if uncertainRemaining > 0}
              <span data-shell class="muted">
                A listing whose outcome the market never confirmed may already be live. Check My Orders before listing it again.
              </span>
            {/if}
          </div>
          <div data-shell class="row gap-sm">
            {#if batch.resumable}
              <button data-shell onclick={() => listing.doResume()} disabled={marketAccess.mutationsBlocked}>Resume</button>
            {:else}
              <button data-shell onclick={onorders}>Review in My Orders</button>
            {/if}
            <button data-shell class="ghost" onclick={() => listing.doDiscard()}>Discard</button>
          </div>
        </div>
      {:else if listing.pendingPlan}
        <div data-shell class="row">
          <div data-shell class="src">
            <span data-shell class="dot stale" aria-hidden="true"></span>
            <strong data-shell>Completed batch still saved.</strong>
            <span data-shell class="muted">{listing.durabilityError ?? 'The saved batch could not be cleared. Check My Orders, then discard this record.'}</span>
          </div>
          <div data-shell class="row gap-sm">
            <button data-shell onclick={() => listing.doResume()}>Retry saving</button>
            <button data-shell onclick={onorders}>Review in My Orders</button>
            <button data-shell class="ghost" onclick={() => listing.doDiscard()}>Discard record</button>
          </div>
        </div>
      {/if}
    </section>
  {/if}
