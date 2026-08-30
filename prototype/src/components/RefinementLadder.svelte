<script lang="ts">
  // The four refinements, side by side, for one relic.
  //
  // Answers the question the intact-only table could never reach: would
  // spending traces on this relic have paid for itself? The bars are the EV at
  // each rung, and the recommended rung is the highest-EV one whose extra plat
  // clears the plat-per-trace bar - per trace the rungs are nearly tied by
  // construction, so ranking on that alone would flip on rounding.
  //
  // Every figure is a SOLO crack. Said out loud, because a squad of four
  // changes the maths and a reader would otherwise assume whichever suits them.
  import { REFINE_WORTH_IT, type RelicDecision } from '../lib/relic-ev';

  let { decision }: { decision: RelicDecision } = $props();

  let max = $derived(Math.max(...decision.ladder.map((l) => l.ev), 0.0001));

  const SHORT: Record<string, string> = {
    intact: 'Int',
    exceptional: 'Exc',
    flawless: 'Flw',
    radiant: 'Rad',
  };
</script>

<div class="ladder">
  <div class="rungs">
    {#each decision.ladder as rung (rung.refinement)}
      <div
        class="rung"
        class:pick={decision.best?.refinement === rung.refinement}
        title="{rung.refinement}: {rung.ev.toFixed(1)}p expected, solo{rung.traces
          ? ` · ${rung.traces} traces · ${rung.platPerTrace?.toFixed(2)}p per trace`
          : ''}"
      >
        <span class="bar" style="height:{Math.max(4, (rung.ev / max) * 100)}%"></span>
        <span class="ev">{rung.ev.toFixed(0)}</span>
        <span class="lbl">{SHORT[rung.refinement] ?? rung.refinement}</span>
      </div>
    {/each}
  </div>
  <p class="verdict">
    {#if decision.verdict === 'refine' && decision.best}
      Refine to <strong>{decision.best.refinement}</strong> - {decision.best.gainOverIntact.toFixed(
        0,
      )}p more for {decision.best.traces} traces
      ({decision.best.platPerTrace?.toFixed(2)}p per trace).
    {:else if decision.verdict === 'crack'}
      Crack intact - refining doesn't clear {REFINE_WORTH_IT}p per trace here.
    {:else if decision.verdict === 'sell-intact'}
      Sell it intact: the relic clears more than its contents.
    {:else if decision.verdict === 'thin'}
      None of its rewards are trading - treat the EV as a guess.
    {:else}
      Not enough price data to judge.
    {/if}
    <span class="solo">Solo crack.</span>
  </p>
</div>

<style>
  .ladder {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }
  .rungs {
    display: flex;
    align-items: flex-end;
    gap: 0.35rem;
    height: 3.2rem;
  }
  .rung {
    flex: 1 1 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: flex-end;
    height: 100%;
    position: relative;
    cursor: help;
  }
  .bar {
    width: 100%;
    background: color-mix(in srgb, var(--fg) 22%, transparent);
    /* The bar is the only thing sized by data; everything else is a label. */
    min-height: 2px;
  }
  .rung.pick .bar {
    background: var(--good);
  }
  .ev {
    font-size: 0.68rem;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
    line-height: 1.2;
  }
  .rung.pick .ev {
    color: var(--fg);
  }
  .lbl {
    font-size: 0.62rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--faint);
  }
  .verdict {
    margin: 0;
    font-size: 0.8rem;
    color: var(--muted);
  }
  .verdict strong {
    color: var(--fg);
  }
  .solo {
    color: var(--faint);
  }
</style>
