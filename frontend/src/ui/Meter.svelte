<script lang="ts">
  /** A share of a whole: a label, the value in words, and an ink fill on one
   *  square track. The words carry the value; the fill only shows how much
   *  of the whole it is, so a part of zero still reads "0 of 5". */
  let { label, value, max, text }: { label: string; value: number; max: number; text: string } = $props();
  const shown = $derived(max > 0 && Number.isFinite(value) ? Math.min(Math.max(value, 0), max) : 0);
</script>

<div class="ui-meter">
  <div class="head"><span class="k">{label}</span><span class="v">{text}</span></div>
  <div class="track" role="meter" aria-label={label} aria-valuemin={0} aria-valuemax={Math.max(max, 0)} aria-valuenow={shown} aria-valuetext={text}>
    <div class="fill" style:width="{max > 0 ? (shown / max) * 100 : 0}%"></div>
  </div>
</div>

<style>
  .ui-meter { display: grid; gap: 4px; min-width: 0; }
  .head { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: baseline; gap: var(--s1) var(--s3); }
  .k { font: 600 var(--text-caption)/1 var(--font-ui); letter-spacing: 0.08em; text-transform: uppercase; color: var(--muted); }
  .v { font: 600 var(--text-caption)/1rem var(--font-mono); font-variant-numeric: tabular-nums; color: var(--fg); }
  .track { height: 8px; background: var(--panel-2); border: 1px solid var(--border); }
  .fill { height: 100%; background: var(--ink-bar); }
</style>
