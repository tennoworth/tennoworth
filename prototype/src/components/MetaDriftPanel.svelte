<script lang="ts">
  import type { Market } from '../lib/types';
  import { buildMetaDrift, type OnlyRow } from '../lib/meta-drift';

  let { market }: { market: Market | null | undefined } = $props();
  let model = $derived(buildMetaDrift(market));
  let tab = $state<'gains' | 'losses' | 'only'>('gains');
  let category = $state('all');
  let query = $state('');

  function matches(row: { name: string; category: string }): boolean {
    return (category === 'all' || row.category === category)
      && (!query.trim() || row.name.toLowerCase().includes(query.trim().toLowerCase()));
  }
  let driftRows = $derived((tab === 'losses' ? model?.losses ?? [] : model?.gains ?? []).filter(matches));
  let currentOnly = $derived((model?.onlyCurrent ?? []).filter(matches));
  let priorOnly = $derived((model?.onlyPrior ?? []).filter(matches));

  function pp(value: number): string {
    return `${value >= 0 ? '+' : ''}${value.toFixed(2)} pp`;
  }
</script>

{#if model}
<section class="wrap tw meta-drift" data-testid="meta-drift">
  <div class="rail">
    <h3>Meta Drift</h3>
    <span class="exp">{model.label}</span>
  </div>
  <div class="intro">
    Annual DE equip-share snapshots, published in arrears. Deltas compare the same item within the same category; they are percentage points, not causes or forecasts.
    {#if model.categoryChanges}<span class="muted"> {model.categoryChanges} category {model.categoryChanges === 1 ? 'change was' : 'changes were'} incomparable and excluded.</span>{/if}
  </div>
  <div class="controls">
    <div class="tabs" role="tablist" aria-label="Meta drift view">
      <button class:active={tab === 'gains'} onclick={() => tab = 'gains'}>Gains</button>
      <button class:active={tab === 'losses'} onclick={() => tab = 'losses'}>Losses</button>
      <button class:active={tab === 'only'} onclick={() => tab = 'only'}>Only in year data</button>
    </div>
    <input bind:value={query} placeholder="Search equipment" aria-label="Search meta drift" />
    <select bind:value={category} aria-label="Filter meta drift category">
      <option value="all">All categories</option>
      {#each model.categories as value}<option value={value}>{value}</option>{/each}
    </select>
  </div>

  {#if tab !== 'only'}
    <div class="scroll">
      <table class="tw fixed">
        <colgroup><col /><col style="width:8rem" /><col style="width:5rem" /><col style="width:5rem" /><col style="width:6rem" /><col style="width:5rem" /><col style="width:5rem" /></colgroup>
        <thead><tr><th class="l">Name</th><th class="l">Category</th><th>{model.priorYear}</th><th>{model.currentYear}</th><th>Δ share</th><th>Low sell</th><th>Vol 48h</th></tr></thead>
        <tbody>
          {#each driftRows as row (row.slug)}
            <tr><td class="l">{row.name}</td><td class="l">{row.category}</td><td>{row.priorShare.toFixed(2)}%</td><td>{row.currentShare.toFixed(2)}%</td><td class:up={row.deltaPp >= 0} class:down={row.deltaPp < 0}><strong>{pp(row.deltaPp)}</strong></td><td>{row.lowSell.toFixed(0)}p</td><td>{row.volume48h.toLocaleString()}</td></tr>
          {:else}<tr><td colspan="7" class="empty">No matching {tab}.</td></tr>{/each}
        </tbody>
      </table>
    </div>
  {:else}
    <div class="only-grid">
      {@render onlyTable(`Only in ${model.currentYear} data`, currentOnly)}
      {@render onlyTable(`Only in ${model.priorYear} data`, priorOnly)}
    </div>
  {/if}
</section>
{/if}

{#snippet onlyTable(title: string, rows: OnlyRow[])}
  <div class="only-card">
    <h4>{title}</h4>
    <div class="scroll"><table class="tw fixed">
      <colgroup><col /><col style="width:8rem" /><col style="width:5rem" /><col style="width:5rem" /><col style="width:5rem" /></colgroup>
      <thead><tr><th class="l">Name</th><th class="l">Category</th><th>Share</th><th>Low sell</th><th>Vol 48h</th></tr></thead>
      <tbody>{#each rows as row (row.slug)}<tr><td class="l">{row.name}</td><td class="l">{row.category}</td><td>{row.share.toFixed(2)}%</td><td>{row.lowSell.toFixed(0)}p</td><td>{row.volume48h.toLocaleString()}</td></tr>{:else}<tr><td colspan="5" class="empty">No matching items.</td></tr>{/each}</tbody>
    </table></div>
  </div>
{/snippet}

<style>
  .meta-drift { min-width: 0; }
  .rail .exp { color: var(--on-ink-muted); }
  .intro { padding: .75rem 1rem; border-bottom: 1px dotted var(--hairline); line-height: 1.45; }
  .controls { display: flex; gap: .6rem; padding: .65rem 1rem; align-items: center; border-bottom: 1px dotted var(--hairline); }
  .tabs { display: flex; gap: .25rem; }
  button, input, select { font: inherit; border: 1px solid var(--hairline); background: var(--panel-2); color: inherit; padding: .35rem .55rem; }
  button.active { background: var(--ink-bar); color: var(--on-ink); }
  input { margin-left: auto; min-width: 12rem; }
  .only-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 1px; background: var(--hairline); }
  .only-card { background: var(--panel); min-width: 0; }
  h4 { margin: 0; padding: .6rem 1rem; font-size: .8rem; }
  .empty { text-align: center; padding: 1rem; color: var(--muted); }
  @media (max-width: 700px) {
    .rail { flex-wrap: wrap; }
    .rail .exp { width: 100%; white-space: normal; overflow: visible; }
    .controls { align-items: stretch; flex-wrap: wrap; }
    .tabs { width: 100%; overflow-x: auto; }
    input { margin-left: 0; flex: 1 1 10rem; min-width: 0; }
    .only-grid { grid-template-columns: 1fr; }
    table { min-width: 42rem; }
  }
</style>
