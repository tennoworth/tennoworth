<script lang="ts">
  import type { Market } from '../../contracts/data';
  import { buildMetaDrift, formatDeltaPp, type OnlyRow } from '../../domain/meta-drift';

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

  function metric(value: number | null, suffix = ''): string {
    return value === null ? '-' : `${value.toLocaleString(undefined, { maximumFractionDigits: 0 })}${suffix}`;
  }
</script>

{#if model}
<section class="view-header"><h2>Meta Drift</h2><p class="lede">{model.label}</p></section>
<section class="wrap tw meta-drift" data-testid="meta-drift">
  <div class="rail">
    <h3>Equip-share changes</h3>
  </div>
  <div class="intro">
    Annual DE equip-share snapshots, published in arrears. Deltas compare the same item within the same category; they are percentage points, not causes or forecasts.
    {#if model.categoryChanges}<span class="muted"> {model.categoryChanges} category {model.categoryChanges === 1 ? 'change was' : 'changes were'} incomparable and excluded.</span>{/if}
  </div>
  <div class="controls">
    <div class="ui-field"><span>Movement</span><div class="tabs" role="group" aria-label="Meta drift view">
      <button class:active={tab === 'gains'} aria-pressed={tab === 'gains'} onclick={() => tab = 'gains'}>Gains</button>
      <button class:active={tab === 'losses'} aria-pressed={tab === 'losses'} onclick={() => tab = 'losses'}>Losses</button>
      <button class:active={tab === 'only'} aria-pressed={tab === 'only'} onclick={() => tab = 'only'}>Only in year data</button>
    </div>
    </div>
    <label class="ui-field meta-search"><span>Equipment</span><input bind:value={query} placeholder="Search equipment" aria-label="Search meta drift" /></label>
    <label class="ui-field"><span>Category</span><select bind:value={category} aria-label="Filter meta drift category">
      <option value="all">All categories</option>
      {#each model.categories as value}<option value={value}>{value}</option>{/each}
    </select></label>
  </div>

  {#if tab !== 'only'}
    <div class="scroll">
      <table class="tw fixed">
        <colgroup><col /><col style="width:8rem" /><col style="width:5rem" /><col style="width:5rem" /><col style="width:6rem" /><col style="width:5rem" /><col style="width:5rem" /></colgroup>
        <thead><tr><th class="l">Name</th><th class="l">Category</th><th>{model.priorYear}</th><th>{model.currentYear}</th><th>Δ share</th><th>Low sell</th><th>Vol 48h</th></tr></thead>
        <tbody>
          {#each driftRows as row (row.slug)}
            <tr><td class="l">{row.name}</td><td class="l">{row.category}</td><td>{row.priorShare.toFixed(2)}%</td><td>{row.currentShare.toFixed(2)}%</td><td class:up={row.deltaPp >= 0} class:down={row.deltaPp < 0}><strong>{formatDeltaPp(row.deltaPp)}</strong></td><td>{metric(row.lowSell, 'p')}</td><td>{metric(row.volume48h)}</td></tr>
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
      <tbody>{#each rows as row (row.slug)}<tr><td class="l">{row.name}</td><td class="l">{row.category}</td><td>{row.share.toFixed(2)}%</td><td>{metric(row.lowSell, 'p')}</td><td>{metric(row.volume48h)}</td></tr>{:else}<tr><td colspan="5" class="empty">No matching items.</td></tr>{/each}</tbody>
    </table></div>
  </div>
{/snippet}

<style>
  .meta-drift { min-width: 0; }
  .intro { padding: .75rem 1rem; border-bottom: 1px dotted var(--hairline); line-height: 1.45; }
  .controls { display: flex; flex-wrap: wrap; gap: var(--s3); padding: var(--s3) var(--inset); align-items: end; border-bottom: 1px dotted var(--hairline); }
  .tabs { display: flex; gap: .25rem; }
  button, input, select { font: inherit; font-size: var(--text-control); min-height: var(--ctl-lg); border: 1px solid var(--border); border-radius: var(--radius-ctl); background: var(--panel-2); color: inherit; padding: var(--s1) var(--s2); }
  button.active { background: var(--ink-bar); color: var(--on-ink); }
  input { width: 100%; min-width: 0; }
  .meta-search { flex: 1 1 15rem; }
  .only-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 1px; background: var(--hairline); }
  .only-card { background: var(--panel); min-width: 0; }
  table { min-width: 46rem; }
  .only-card table { min-width: 35rem; }
  h4 { margin: 0; padding: var(--s3) var(--inset); font-size: var(--text-control); }
  .empty { text-align: center; padding: 1rem; color: var(--muted); }
  @media (max-width: 700px) {
    .wrap.tw.meta-drift > .rail {
      flex-wrap: wrap;
      height: auto;
      min-height: var(--rail);
      padding-top: .5rem;
      padding-bottom: .5rem;
    }
    .controls { align-items: stretch; flex-wrap: wrap; }
    .tabs { width: 100%; overflow-x: auto; }
    input { margin-left: 0; flex: 1 1 10rem; min-width: 0; }
    .only-grid { grid-template-columns: 1fr; }
    table { min-width: 42rem; }
  }
</style>
