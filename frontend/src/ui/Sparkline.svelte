<script lang="ts">
  import { sparklineGeometry } from './sparkline';

  // The line is the recent medians; the dotted rule is the 90-day median; the
  // endpoint dot takes the direction of the move against that baseline.
  let { series, width = 60, height = 18, baseline = null, trend = null, title = '', class: className = 'spark' }: {
    series: number[] | null | undefined;
    width?: number;
    height?: number;
    baseline?: number | null;
    trend?: number | null;
    title?: string;
    class?: string;
  } = $props();

  const R = 2.5;
  let g = $derived(sparklineGeometry(series, width, height, baseline, R));
</script>

{#if g}
  <svg class={className} viewBox="0 0 {width} {height}" {width} {height} aria-hidden="true">
    {#if title}<title>{title}</title>{/if}
    {#if g.baseline != null}<line class="base" x1="0" x2={width} y1={g.baseline} y2={g.baseline} />{/if}
    <polyline points={g.points} fill="none" stroke="currentColor" stroke-width="1.25" />
    <circle class="end" class:up={trend != null && trend > 0} class:down={trend != null && trend < 0} cx={g.end.x} cy={g.end.y} r={R} />
  </svg>
{/if}

<style>
  /* Ink line by default; a table's dimmed-row rule (global, more specific)
     still fades it. */
  svg { color: var(--accent); vertical-align: middle; }
  .base { stroke: var(--faint); stroke-width: 1; stroke-dasharray: 1 2; }
  .end { fill: currentColor; }
  .end.up { fill: var(--good); }
  .end.down { fill: var(--bad); }
</style>
