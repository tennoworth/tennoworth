<script lang="ts">
  // "Someone offered me 200p - is that good, and should I reroll instead?"
  //
  // The panel around this one already refuses to print "this riven is worth N",
  // and that stays true here: the price comes from the USER. What we add is the
  // arithmetic against DE's weekly distribution for the weapon - where the
  // offer sits and what a reroll would cost. Every number carries the
  // limits of the band it came from.
  import { appraise, rerolledDiscount } from '../../domain/riven-appraise';
  import { bandForRiven } from '../../domain/rivens';
  import type { OwnedRiven } from '../../domain/rivens';
  import type { Market } from '../../contracts/data';
  import { formatLocale, t } from '../../i18n/i18n.svelte';
  import { plat } from '../../ui/format';
  import Rich from '../../ui/Rich.svelte';
  import type { AppraisalCaveat } from '../../domain/riven-appraise';

  let {
    riven,
    market,
  }: {
    riven: Pick<OwnedRiven, 'slug' | 'rerolls'>;
    market: Market | null;
  } = $props();

  // Kept as a string so an empty box is empty rather than 0 - an offer of
  // "0p" and "no offer entered" are different questions.
  let offerText = $state('');
  let offer = $derived.by(() => {
    const n = Number(offerText.trim());
    return offerText.trim() !== '' && Number.isFinite(n) && n > 0 ? n : null;
  });

  // DE's inventory data has not yet been seen carrying a lock, so the owner
  // says whether one is set. A spliced trait does not count: it stays in place
  // without doubling the cost.
  let locked = $state(false);

  let band = $derived(bandForRiven(riven, market?.riven_stats));
  let result = $derived(appraise(offer, band, riven.rerolls, locked));

  let entry = $derived(riven.slug ? market?.riven_stats?.[riven.slug] : undefined);
  let discount = $derived(rerolledDiscount(entry?.unrolled, entry?.rolled));

  function caveatText(code: AppraisalCaveat): string {
    const d = result.dist;
    if (code === 'band') return t('offer.caveat.band', { pop: d?.pop ?? 0 });
    if (code === 'skewed') return t('offer.caveat.skewed', { avg: plat(d?.avg), median: plat(d?.median) });
    return t(`offer.caveat.${code}`);
  }

  function rerollKey(locked: boolean, above: boolean) {
    if (locked) return above ? 'offer.rerollLocked' : 'offer.rerollLockedAtOrBelow';
    return above ? 'offer.reroll' : 'offer.rerollAtOrBelow';
  }

  function pct(n: number): string {
    return (n).toLocaleString(formatLocale(), { style: 'percent', maximumFractionDigits: 0 });
  }

  /** Position along the min–max track, clamped. `span` has a floor of 1 so a
   *  weapon whose every trade landed on one price cannot divide by zero. */
  function at(v: number): number {
    const d = result.dist;
    if (!d) return 0;
    const span = Math.max(1, d.max - d.min);
    return Math.min(100, Math.max(0, ((v - d.min) / span) * 100));
  }
</script>

<div class="offer">
  <label>
    <span>{t('offer.offer')}</span>
    <input
      type="number"
      inputmode="numeric"
      min="1"
      step="1"
      placeholder={t('offer.plat')}
      bind:value={offerText}
    />
  </label>
  <label class="lock">
    <input type="checkbox" bind:checked={locked} />
    <span>{t('offer.locked')}</span>
  </label>

  {#if result.unavailable}
    <p class="note">{caveatText(result.caveats[0])}</p>
  {:else if result.dist}
    <!-- min / median / max, with the offer marked. Positions are clamped so an
         outlier offer sits at the edge instead of overflowing the bar. -->
    <div class="dist" aria-hidden="true">
      <span class="track"></span>
      <span class="median" style="left:{at(result.dist.median)}%"></span>
      {#if offer != null}
        <span class="you" style="left:{at(offer)}%"></span>
      {/if}
    </div>
    <div class="axis">
      <span>{plat(result.dist.min)}p</span>
      <span>{t('offer.median', { price: plat(result.dist.median) })}</span>
      <span>{plat(result.dist.max)}p</span>
    </div>

    {#if offer != null && result.placement && result.verdict}
      <p class="read">
        <strong>{t(`offer.place.${result.placement}`)}</strong> - {t(`offer.verdict.${result.verdict}`)}.
      </p>
    {/if}

    {#if result.reroll && offer != null}
      <!-- Two facts, no forecast. A reroll draws from the set of POSSIBLE
           rolls while DE publishes the SOLD ones, so nothing here - not a
           percentage, not even a "likely" - can say what a new roll will do.
           The reader draws the inference. -->
      <p class="read">
        <Rich text={t(rerollKey(locked, result.reroll.aboveMedian), { kuva: plat(result.reroll.kuva), median: plat(result.reroll.median) })} />
      </p>
    {/if}

    {#if result.skewed}
      <p class="note warnish">
        {t('offer.skewed')}
      </p>
    {/if}

    {#if discount != null && discount > 0.02}
      <p class="note">
        <Rich text={t('offer.discount', { pct: pct(discount) })} />
      </p>
    {/if}

    <ul class="caveats">
      {#each result.caveats as c (c)}
        <li>{caveatText(c)}</li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  /* The cell around this is a numeric column; sentences here read in the
     body face, and numbers keep the mono face where they are marked. */
  .offer {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    font-size: 0.85rem;
    font-family: var(--font-body);
  }
  .axis {
    font-family: var(--font-mono);
  }
  label {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    color: var(--muted);
  }
  input[type='number'] {
    width: 6rem;
  }
  .dist {
    position: relative;
    height: 1.1rem;
  }
  .track {
    position: absolute;
    left: 0;
    right: 0;
    top: 50%;
    height: 1px;
    background: var(--hairline);
  }
  .median,
  .you {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 2px;
  }
  .median {
    background: var(--faint);
  }
  .you {
    background: var(--warn);
  }
  .axis {
    display: flex;
    justify-content: space-between;
    font-size: var(--text-caption);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .read,
  .note {
    margin: 0;
    color: var(--muted);
  }
  .read strong,
  .read :global(b) {
    color: var(--fg);
  }
  .warnish {
    color: var(--warn);
  }
  .caveats {
    margin: 0;
    padding-left: 1rem;
    color: var(--muted);
    font-size: var(--text-caption);
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }
</style>
