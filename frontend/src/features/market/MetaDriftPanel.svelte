<script lang="ts">
  import { roving } from '../../ui/keyboard-actions';
  import { fitTable } from '../../ui/table-fit';
  import { searchShortcut } from '../../ui/keyboard.svelte';
  import Select from '../../ui/Select.svelte';
  import type { Market } from '../../contracts/data';
  import { buildMetaDrift, formatDeltaPp, type OnlyRow } from '../../domain/meta-drift';
  import { formatLocale, t } from '../../i18n/i18n.svelte';

  // `limit` trims each list where Meta Drift is one section of a longer page
  // (hosted, first run); the desktop Meta Drift view passes none.
  let { market, limit = null, embedded = false }: { market: Market | null | undefined; limit?: number | null; embedded?: boolean } = $props();
  let model = $derived(buildMetaDrift(market));
  let tab = $state<'gains' | 'losses' | 'only'>('gains');
  let category = $state('all');
  let query = $state('');
  let showAll = $state(false);
  // A search wants every match, so it lifts the limit.
  let capped = $derived(limit != null && !showAll && !query.trim());
  function trim<T>(rows: T[]): T[] { return capped ? rows.slice(0, limit!) : rows; }

  function matches(row: { name: string; category: string }): boolean {
    return (category === 'all' || row.category === category)
      && (!query.trim() || row.name.toLowerCase().includes(query.trim().toLowerCase()));
  }
  let driftRows = $derived((tab === 'losses' ? model?.losses ?? [] : model?.gains ?? []).filter(matches));
  let currentOnly = $derived((model?.onlyCurrent ?? []).filter(matches));
  let priorOnly = $derived((model?.onlyPrior ?? []).filter(matches));

  function metric(value: number | null, suffix = ''): string {
    return value === null ? '-' : `${value.toLocaleString(formatLocale(), { maximumFractionDigits: 0 })}${suffix}`;
  }
  const share = (value: number) => `${value.toLocaleString(formatLocale(), { minimumFractionDigits: 2, maximumFractionDigits: 2 })}%`;
  let label = $derived(model
    ? model.missingYears.length
      ? t('drift.labelMissing', { prior: String(model.priorYear), current: String(model.currentYear), missing: model.missingYears.join(', ') })
      : t('drift.label', { prior: String(model.priorYear), current: String(model.currentYear) })
    : '');
</script>

{#if model}
{#if !embedded}<section class="view-header"><h2>{t('drift.title')}</h2><p class="lede">{label}</p></section>{/if}
<section class="wrap tw meta-drift" data-testid="meta-drift">
  <div class="rail">
    <h3>{t('drift.changes')}</h3>
    {#if embedded}<span class="exp">{label}</span>{/if}
  </div>
  <div class="intro">
    {t('drift.intro')}
    {#if model.categoryChanges}<span class="muted"> {t('drift.excluded', { count: model.categoryChanges })}</span>{/if}
  </div>
  <div class="controls">
    <div class="ui-field"><span>{t('drift.movement')}</span><div class="ui-segmented" role="group" aria-label={t('drift.movement')} use:roving>
      <button type="button" aria-pressed={tab === 'gains'} onclick={() => tab = 'gains'}>{t('drift.gains')}</button>
      <button type="button" aria-pressed={tab === 'losses'} onclick={() => tab = 'losses'}>{t('drift.losses')}</button>
      <button type="button" aria-pressed={tab === 'only'} onclick={() => tab = 'only'}>{t('drift.only')}</button>
    </div>
    </div>
    <label class="ui-field meta-search"><span>{t('drift.equipment')}</span><input bind:value={query} use:searchShortcut placeholder={t('drift.search')} /></label>
    <div class="ui-field"><span>{t('drift.category')}</span><Select label={t('drift.category')} value={category} options={[{ value: 'all', label: t('drift.allCategories') }, ...model.categories.map((value) => ({ value, label: value }))]} onchange={(next) => (category = next)} /></div>
  </div>

  {#if tab !== 'only'}
    <div class="scroll" use:fitTable>
      <table class="tw fixed">
        <!-- Δ share sits beside the name: it is the point of the table, and a
             narrow window must reach it without scrolling. -->
        <colgroup><col /><col style="width:6rem" /><col style="width:5rem" /><col style="width:5rem" /><col style="width:8rem" /><col style="width:5rem" /><col style="width:5rem" /></colgroup>
        <thead><tr><th class="l">{t('drift.col.name')}</th><th>{t('drift.col.delta')}</th><th data-drop="3">{model.priorYear}</th><th>{model.currentYear}</th><th class="l" data-drop="1">{t('drift.category')}</th><th data-drop="2">{t('market.col.lowAsk')}</th><th data-drop="1">{t('market.col.vol')}</th></tr></thead>
        <tbody>
          {#each trim(driftRows) as row (row.slug)}
            <tr><td class="l">{row.name}</td><td class:up={row.deltaPp >= 0} class:down={row.deltaPp < 0}><strong>{formatDeltaPp(row.deltaPp)}</strong></td><td>{share(row.priorShare)}</td><td>{share(row.currentShare)}</td><td class="l">{row.category}</td><td>{metric(row.lowSell, 'p')}</td><td>{metric(row.volume48h)}</td></tr>
          {:else}<tr><td colspan="7" class="empty">{t(tab === 'losses' ? 'drift.noLosses' : 'drift.noGains')}</td></tr>{/each}
        </tbody>
      </table>
    </div>
    {@render more(driftRows.length)}
  {:else}
    <div class="only-grid">
      {@render onlyTable(t('drift.onlyIn', { year: String(model.currentYear) }), currentOnly)}
      {@render onlyTable(t('drift.onlyIn', { year: String(model.priorYear) }), priorOnly)}
    </div>
    {@render more(Math.max(currentOnly.length, priorOnly.length))}
  {/if}
</section>
{/if}

{#snippet more(total: number)}
  {#if limit != null && !query.trim() && total > limit}
    <div class="foot">
      <span class="exp">{showAll ? t('market.all', { count: total }) : t('market.topOf', { shown: limit, total })}</span>
      <span class="grow"></span>
      <button type="button" class="btn xs" aria-expanded={showAll} onclick={() => (showAll = !showAll)}>{showAll ? t('market.showTop', { count: limit }) : t('market.showAll', { count: total })}</button>
    </div>
  {/if}
{/snippet}

{#snippet onlyTable(title: string, rows: OnlyRow[])}
  <div class="only-card">
    <h4>{title}</h4>
    <div class="scroll" use:fitTable><table class="tw fixed">
      <colgroup><col /><col style="width:8rem" /><col style="width:5rem" /><col style="width:5rem" /><col style="width:5rem" /></colgroup>
      <thead><tr><th class="l">{t('drift.col.name')}</th><th class="l" data-drop="2">{t('drift.category')}</th><th>{t('drift.col.share')}</th><th>{t('market.col.lowAsk')}</th><th data-drop="1">{t('market.col.vol')}</th></tr></thead>
      <tbody>{#each trim(rows) as row (row.slug)}<tr><td class="l">{row.name}</td><td class="l">{row.category}</td><td>{share(row.share)}</td><td>{metric(row.lowSell, 'p')}</td><td>{metric(row.volume48h)}</td></tr>{:else}<tr><td colspan="5" class="empty">{t('drift.noItems')}</td></tr>{/each}</tbody>
    </table></div>
  </div>
{/snippet}

<style>
  .meta-drift { min-width: 0; }
  .intro { padding: .75rem 1rem; border-bottom: 1px dotted var(--hairline); line-height: 1.45; }
  .controls { display: flex; flex-wrap: wrap; gap: var(--s3); padding: var(--s3) var(--inset); align-items: end; border-bottom: 1px dotted var(--hairline); }
  input, .controls :global(.ui-select) { font: inherit; font-size: var(--text-control); min-height: var(--ctl-lg); border: 1px solid var(--border); border-radius: var(--radius-ctl); background: var(--panel-2); color: inherit; padding: var(--s1) var(--s2); }
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
    .only-grid { grid-template-columns: 1fr; }
    table { min-width: 42rem; }
  }
</style>
