<script lang="ts">
  // The latest price-watch hit, ready to act on when the player switches to the
  // app: who to whisper, the whisper, and Copy. Rust owns the offer and its
  // 15-minute life; this banner only shows it and asks for the one recheck.
  import { onMount } from 'svelte';
  import { focusView } from '../../ui/keyboard-actions';
  import { useDesktopServices } from '../shell/desktop-context';
  import { WATCH_OFFER_FRESH_SECS, type WatchOffer } from '../../contracts/desktop';
  import { WATCH_OFFER_EVENT } from '../../contracts/events';
  import { humanError } from '../../contracts/errors';
  import { t } from '../../i18n/i18n.svelte';

  let { onwatches }: { onwatches?: () => void } = $props();
  const { desktopLatestWatchOffer, desktopDismissWatchOffer, desktopCopyWatchWhisper, desktopRecheckWatchOffer, listenForTauriEvent } = useDesktopServices();

  let offer = $state<WatchOffer | null>(null);
  let now = $state(Date.now());
  let checking = $state(false);
  let recheckError = $state('');
  let copied = $state(false);
  let copyError = $state('');
  // Offers this banner already asked about; the recheck is once per offer.
  const asked = new Set<string>();
  const key = (o: WatchOffer) => `${o.watch_id}:${o.seen_at}`;

  const fresh = $derived(offer && now / 1000 - offer.seen_at < WATCH_OFFER_FRESH_SECS ? offer : null);
  const age = $derived.by(() => {
    if (!fresh) return '';
    const minutes = Math.floor((now / 1000 - fresh.seen_at) / 60);
    return minutes < 1 ? t('format.justNow') : t('format.minutesAgo', { n: minutes });
  });

  async function load() {
    try {
      const next = await desktopLatestWatchOffer();
      if (!next || !offer || key(next) !== key(offer)) { copied = false; copyError = ''; recheckError = ''; }
      offer = next;
      maybeRecheck();
    } catch { /* The banner is optional; a failed read leaves it as it was. */ }
  }

  // Only when the player is looking at the app: a hit that arrives while they
  // are in game costs warframe.market nothing until they switch over.
  function maybeRecheck() {
    if (!fresh || fresh.rechecked_at != null || asked.has(key(fresh))) return;
    if (document.visibilityState !== 'visible' || !document.hasFocus()) return;
    asked.add(key(fresh));
    checking = true; recheckError = '';
    desktopRecheckWatchOffer()
      .then((next) => { if (next && offer && key(next) === key(offer)) offer = next; })
      .catch((e) => { recheckError = humanError(e); })
      .finally(() => { checking = false; });
  }

  async function copy() {
    copyError = '';
    try { await desktopCopyWatchWhisper(); copied = true; }
    catch (e) { copied = false; copyError = humanError(e); }
  }

  async function dismiss() {
    offer = null;
    try { await desktopDismissWatchOffer(); } catch { /* Rust drops it at expiry regardless. */ }
  }

  onMount(() => {
    void load();
    const unlisten = listenForTauriEvent(WATCH_OFFER_EVENT, () => void load());
    const tick = setInterval(() => { now = Date.now(); }, 30_000);
    const focus = () => { now = Date.now(); maybeRecheck(); };
    window.addEventListener('focus', focus);
    document.addEventListener('visibilitychange', focus);
    return () => {
      unlisten();
      clearInterval(tick);
      window.removeEventListener('focus', focus);
      document.removeEventListener('visibilitychange', focus);
    };
  });
</script>

{#if fresh}
  <div class="card ui-panel warn-banner general-banner watch-offer" role="status" data-testid="watch-offer" data-region="notices">
    <div class="gb-body">
      <strong>{t('offer.hit', { age })}</strong>
      {t(fresh.side === 'sell' ? 'offer.sells' : 'offer.buys', { player: fresh.player, item: fresh.name, price: fresh.price, threshold: fresh.threshold })}
      <code class="whisper">{fresh.whisper}</code>
      {#if checking}<p class="state">{t('offer.checking')}</p>
      {:else if fresh.still_listed === true}<p class="state">{t('offer.stillListed')}</p>
      {:else if fresh.still_listed === false}<p class="state" data-tone="warn">{t('offer.gone')}</p>
      {:else if recheckError}<p class="state">{t('offer.recheckFailed', { error: recheckError })}</p>{/if}
      {#if copyError}<p class="state" role="alert">{copyError}</p>{/if}
    </div>
    <div class="gb-actions">
      <button class="btn primary" onclick={copy}>{t(copied ? 'offer.copied' : 'offer.copy')}</button>
      {#if onwatches}<button class="btn" onclick={onwatches}>{t('offer.watches')}</button>{/if}
      <button class="gb-dismiss" aria-label={t('offer.dismiss')} onclick={() => { dismiss(); requestAnimationFrame(focusView); }}>×</button>
    </div>
  </div>
{/if}

<style>
  .whisper { display: block; margin-top: var(--s1); font-family: var(--font-mono); font-size: var(--text-control); overflow-wrap: anywhere; user-select: all; }
  .state { margin: var(--s1) 0 0; color: var(--muted); font-size: var(--text-control); }
  .state[data-tone="warn"] { color: var(--warn); }
</style>
