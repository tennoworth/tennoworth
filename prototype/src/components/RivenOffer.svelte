<script lang="ts">
  // "Someone offered me 200p - is that good, and should I reroll instead?"
  //
  // The panel around this one already refuses to print "this riven is worth N",
  // and that stays true here: the price comes from the USER. What we add is the
  // arithmetic against DE's weekly distribution for the weapon - where the
  // offer sits, what a reroll would cost, and the odds it improves on what
  // they hold. Every number carries the population it came from.
  import { appraise, rerolledDiscount, type PriceVerdict } from '../lib/riven-appraise';
  import { bandForRiven } from '../lib/rivens';
  import type { OwnedRiven } from '../lib/rivens';
  import type { Market } from '../lib/types';

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

  let band = $derived(bandForRiven(riven, market?.riven_stats));
  let result = $derived(appraise(offer, band, riven.rerolls));

  let entry = $derived(riven.slug ? market?.riven_stats?.[riven.slug] : undefined);
  let discount = $derived(rerolledDiscount(entry?.unrolled, entry?.rolled));

  const PLACEMENT_TEXT: Record<
    NonNullable<ReturnType<typeof appraise>['placement']>,
    string
  > = {
    'below-observed': 'Under the cheapest sale DE recorded',
    bottom: 'Below the median',
    middle: 'Between median and average',
    upper: 'Above the average',
    'above-observed': 'Over the dearest sale DE recorded',
  };

  const VERDICT_TEXT: Record<PriceVerdict, string> = {
    below: 'below this weapon’s market',
    fair: 'about right for this weapon',
    above: 'above this weapon’s market',
    outlier: 'above anything DE recorded this week',
  };

  function pct(n: number): string {
    return `${Math.round(n * 100)}%`;
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
    <span>Offer</span>
    <input
      type="number"
      inputmode="numeric"
      min="1"
      step="1"
      placeholder="plat"
      bind:value={offerText}
    />
  </label>

  {#if result.unavailable}
    <p class="note">{result.caveats[0]}</p>
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
      <span>{result.dist.min.toFixed(0)}p</span>
      <span>median {result.dist.median.toFixed(0)}p</span>
      <span>{result.dist.max.toFixed(0)}p</span>
    </div>

    {#if offer != null && result.placement && result.verdict}
      <p class="read">
        <strong>{PLACEMENT_TEXT[result.placement]}</strong> - {VERDICT_TEXT[result.verdict]}.
      </p>
    {/if}

    {#if result.reroll && offer != null}
      <!-- Two facts, no forecast. A reroll draws from the set of POSSIBLE
           rolls while DE publishes the SOLD ones, so nothing here - not a
           percentage, not even a "likely" - can say what a new roll will do.
           The reader draws the inference. -->
      <p class="read">
        Rerolling costs <strong>{result.reroll.kuva.toLocaleString()}</strong> kuva. This weapon's
        median trade is <strong>{result.reroll.median.toFixed(0)}p</strong>; your offer is
        {result.reroll.aboveMedian ? 'above' : 'at or below'} it.
      </p>
    {/if}

    {#if result.skewed}
      <p class="note warnish">
        A few large sales pull this weapon's average well above its median. Read the median.
      </p>
    {/if}

    {#if discount != null && discount > 0.02}
      <p class="note">
        Rerolled rivens on this weapon trade about <strong>{pct(discount)}</strong> below unrolled
        ones - buyers pay for reroll headroom.
      </p>
    {/if}

    <ul class="caveats">
      {#each result.caveats as c (c)}
        <li>{c}</li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .offer {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    font-size: 0.85rem;
  }
  label {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    color: var(--muted);
  }
  input {
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
    font-size: 0.72rem;
    color: var(--faint);
    font-variant-numeric: tabular-nums;
  }
  .read,
  .note {
    margin: 0;
    color: var(--muted);
  }
  .read strong {
    color: var(--fg);
  }
  .warnish {
    color: var(--warn);
  }
  .caveats {
    margin: 0;
    padding-left: 1rem;
    color: var(--faint);
    font-size: 0.75rem;
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }
</style>
