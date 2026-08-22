<script lang="ts">
  // Demand for one row: usage share, a liquidity read, and who buys it.
  //
  // Deliberately small — it sits in a table cell next to the price, because
  // its whole job is to break the tie between two rows that look identical on
  // price and spread. The tooltip carries the honesty the cell has no room
  // for: which parent the figure came from, and which year DE measured.
  import { masteryBand, type DemandRead } from '../lib/demand';

  let { demand }: { demand: DemandRead } = $props();

  const LABEL: Record<DemandRead['liquidity'], string> = {
    'sells-today': 'sells today',
    underpriced: 'underpriced',
    slow: 'slow',
    thin: 'thin',
    unknown: '—',
  };

  let band = $derived(demand.usage ? masteryBand(demand.usage.by_mr) : null);

  let title = $derived.by(() => {
    if (!demand.usage) return 'No usage telemetry for this item or its set.';
    const u = demand.usage;
    const parts = [
      `${u.share.toFixed(2)}% of ${u.category} usage in ${u.year}`,
      demand.inherited ? `measured on ${u.name}, not this part` : `measured on ${u.name}`,
    ];
    if (band) parts.push(`mostly MR ${band.from}–${band.to}`);
    return parts.join(' · ');
  });
</script>

<span class="demand" {title}>
  {#if demand.usage}
    <span class="share">{demand.usage.share.toFixed(1)}%</span>
    {#if demand.inherited}<span class="inh" aria-label="inherited from the set">↑</span>{/if}
  {:else}
    <span class="muted">—</span>
  {/if}
  <span class="liq liq-{demand.liquidity}">{LABEL[demand.liquidity]}</span>
</span>

<style>
  .demand {
    display: inline-flex;
    align-items: baseline;
    gap: 0.3rem;
    white-space: nowrap;
    cursor: help;
  }
  .share {
    font-variant-numeric: tabular-nums;
  }
  /* The arrow is the only marker that this number belongs to the parent, so it
     gets a label rather than being purely decorative. */
  .inh {
    color: var(--faint);
    font-size: 0.75em;
  }
  .muted {
    color: var(--muted);
  }
  .liq {
    font-size: 0.72rem;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .liq-sells-today,
  .liq-underpriced {
    color: var(--good);
  }
  .liq-slow {
    color: var(--warn);
  }
  .liq-thin,
  .liq-unknown {
    color: var(--faint);
  }
</style>
