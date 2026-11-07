<script lang="ts">
  import { busy, focusLater } from '../../ui/keyboard-actions';
  import type { Market, OwnedRecord } from '../../contracts/data';
  import type { ProtectionController } from './protection.svelte';
  import Select from '../../ui/Select.svelte';
  import { t } from '../../i18n/i18n.svelte';
  let { controller, owned, market, onconnect, reserveCopies, onsetKeep, unavailableCount, unavailable, onscan, scanning = false }: { controller: ProtectionController; owned: Map<string, OwnedRecord>; market: Market | null; onconnect(): void; reserveCopies: number; onsetKeep(value: number): Promise<void>; unavailableCount: number; unavailable: boolean; onscan(): void; scanning?: boolean } = $props();
  let editing = $state(false);
  let dialog: HTMLDialogElement;
  let search = $state('');
  let detailSearch = $state('');
  let detailLimit = $state(25);
  let keepBusy = $state(false);
  let goal = $state('');
  let selected = $state('');
  let quantity = $state<number | undefined>(1);
  let reserves = $state<Record<string, number>>({});
  let formError = $state<string | null>(null);
  let choices = $derived([...owned.values()].filter(row => !row.subtype && !row.slug.endsWith('_set') && !row.slug.endsWith('_relic') && market?.items[row.slug])
    .filter((row, i, rows) => rows.findIndex(other => other.slug === row.slug) === i).sort((a, b) => a.name.localeCompare(b.name)));
  let filteredChoices = $derived(choices.filter(row => row.name.toLowerCase().includes(search.toLowerCase())));
  let goalOptions = $derived([
    { value: '', label: t('keep.noGoal') },
    ...(goal && !market?.set_to_parts?.[goal] ? [{ value: goal, label: t('keep.recipeUnresolved', { goal }) }] : []),
    ...Object.entries(market?.set_to_parts ?? {}).sort((a, b) => a[1].name.localeCompare(b[1].name)).map(([slug, set]) => ({ value: slug, label: set.name })),
  ]);
  let itemOptions = $derived([{ value: '', label: t('keep.chooseItem') }, ...filteredChoices.map(row => ({ value: row.slug, label: row.name }))]);
  let goalName = $derived(controller.savedPlan?.goal ? (market?.set_to_parts?.[controller.savedPlan.goal]?.name ?? controller.savedPlan.goal) : null);
  let manualCount = $derived(Object.keys(controller.savedPlan?.reserves ?? {}).length);
  let conflicts = $derived(Object.values(controller.state?.items ?? {}).some(row => row.protected + (row.listed ?? 0) > row.owned));
  let keepRules = $derived([
    t('keep.copiesRule', { count: reserveCopies }),
    t('keep.leveledOut'),
    ...(controller.savedPlan ? [goalName ? t('keep.partsFor', { set: goalName }) : t('keep.noGoal'), t('keep.extraRules', { count: manualCount })] : []),
  ].join(' · '));
  let invalidInventory = $derived(controller.error?.includes('Inventory quantities are invalid') ?? false);
  async function changeKeep(value: number) {
    keepBusy = true; formError = null;
    try { await onsetKeep(value); await controller.refresh(); }
    catch { formError = t('keep.saveFailed'); }
    finally { keepBusy = false; }
  }
  let names = $derived(new Map([...owned.values()].map(row => [row.slug, row.name])));
  let detailRows = $derived(Object.entries(controller.state?.items ?? {}).filter(([slug]) => (names.get(slug) ?? slug).toLowerCase().includes(detailSearch.toLowerCase())));
  function edit() {
    reserves = { ...controller.savedPlan?.reserves };
    goal = controller.savedPlan?.goal ?? '';
    formError = null;
    editing = true;
    dialog.showModal();
  }
  function reserve() {
    if (!selected || quantity == null || !Number.isSafeInteger(quantity) || quantity < 0 || quantity > 1_000_000) {
      formError = t('keep.quantityRange'); return;
    }
    const next = { ...reserves };
    if (quantity === 0) delete next[selected]; else next[selected] = quantity;
    reserves = next;
    formError = null;
  }
  async function save() {
    if (await controller.save({ reserves, goal: goal || null })) { editing = false; dialog.close(); }
  }
</script>

{#if unavailable && !controller.loading}
  <section class="ui-notice ui-stack" data-tone="bad" role="alert">
    <strong>{t('keep.cannotCalculate')}</strong>
    <p>{controller.error ?? t('keep.notApplied')}</p>
    <div class="ui-toolbar"><button class="btn primary" disabled={scanning || controller.saving} onclick={() => invalidInventory ? onscan() : controller.refresh()}>{t(invalidInventory ? 'keep.scanAgain' : 'keep.recheck')}</button></div>
    <p class="muted">{t('keep.notEmpty')}</p>
  </section>
{:else if conflicts && !controller.loading}
  <p class="ui-notice" data-tone="warn">{t('keep.conflicts')}</p>
{:else if unavailableCount && !controller.loading}
  <p class="ui-notice" data-tone="warn">{t('keep.unavailable', { count: unavailableCount })} <button class="btn" onclick={() => controller.refresh()}>{t('keep.recheck')}</button></p>
{/if}
<!-- One line while healthy: problems already surface as the notices above, so
     the settings themselves stay quiet. Details open below the line. -->
<section class="keeping" aria-label={t('keep.title')}>
  <h3>{t('keep.title')}</h3>
  <p class="keep-rules">{keepRules}</p>
  <p class="keep-status" class:verified={!conflicts && !unavailable && !unavailableCount && !!controller.state && !controller.loading && !controller.error}>{t(controller.loading ? 'keep.checking' : unavailable ? 'keep.waitingValid' : conflicts ? 'keep.needsReview' : unavailableCount ? 'keep.appliedKnown' : controller.state ? 'keep.applied' : 'keep.waitingInventory')}</p>
  <details>
    <summary>{t('keep.details')}</summary>
    <div class="ui-toolbar"><button class="btn" onclick={() => controller.refresh()} disabled={controller.loading || controller.saving}>{t(controller.loading ? 'keep.checking' : 'keep.recheck')}</button>
    {#if controller.state?.issues.some(issue => issue.includes('Unlock WFM'))}<button class="btn" onclick={onconnect}>{t('keep.connectWfm')}</button>{/if}</div>
    <p class="muted">{t('keep.detailsNote')}</p>
    {#if controller.error && !unavailable}<p class="ui-notice" data-tone="bad">{controller.error}</p>{/if}
    {#if controller.state?.issues.length}<ul class="ui-notice" data-tone="warn">{#each controller.state.issues as issue}<li>{issue}</li>{/each}</ul>{/if}
    {#if controller.state && Object.keys(controller.state.items).length}
      <label class="ui-field">{t('keep.findInDetails')}<input class="ui-input" type="search" bind:value={detailSearch} oninput={() => detailLimit = 25} /></label>
      <!-- svelte-ignore a11y_no_noninteractive_tabindex (Keyboard users must reach every quantity column.) -->
      <div class="wrap tw" role="region" aria-label={t('keep.allocation')} tabindex="0">
        <table class="tw"><thead><tr><th class="l">{t('keep.col.item')}</th><th>{t('keep.col.own')}</th><th>{t('keep.col.keep')}</th><th>{t('keep.col.listed')}</th><th>{t('keep.col.canSell')}</th></tr></thead>
        <tbody>{#each detailRows.slice(0, detailLimit) as [slug, row]}<tr><td class="l">{names.get(slug) ?? slug}</td><td>{row.owned}</td><td>{row.protected}</td><td>{row.listed ?? t('keep.unknown')}</td><td>{row.available ?? (row.estimated == null ? t('keep.unavailableCell') : t('keep.estimated', { count: row.estimated }))}</td></tr>{/each}</tbody></table>
      </div>
      <div class="ui-toolbar"><span class="muted">{t('keep.showing', { shown: Math.min(detailLimit, detailRows.length), count: detailRows.length })}</span>{#if detailRows.length > detailLimit}<button class="btn" onclick={() => detailLimit += 25}>{t('keep.showMore')}</button>{/if}</div>
    {/if}
  </details>
  <button class="btn xs change" onclick={edit} use:busy={controller.saving}>{t('keep.change')}</button>
</section>
<dialog bind:this={dialog} class="keep-dialog" aria-labelledby="keep-title" onclose={() => editing = false} onkeydown={(event) => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); if (!controller.saving && !keepBusy) dialog.close(); } }} oncancel={(event) => { if (controller.saving || keepBusy) event.preventDefault(); }}>
  <header class="keep-head"><h2 id="keep-title">{t('keep.title')}</h2></header>
  <div class="ui-stack">
    <p>{t('keep.chooseOnce')}</p>
    <label class="ui-field">{t('keep.copiesOfEvery')}
      <input class="ui-input" type="number" min="0" max="1000000" step="1" value={reserveCopies} use:busy={controller.saving || keepBusy} onchange={(e) => { const n = Number(e.currentTarget.value); if (Number.isSafeInteger(n) && n >= 0 && n <= 1000000) void changeKeep(n); else formError = t('keep.wholeQuantity'); }} />
    </label>
    <p class="muted">{t('keep.leveledNote')}</p>
  {#if editing}
    <div class="ui-stack">
      {#if !controller.savedPlan}<p class="ui-notice" data-tone="warn">{t('keep.notLoaded')}</p>{/if}
      <div class="ui-field"><span>{t('keep.partsForSet')}</span>
        <Select label={t('keep.partsForSet')} value={goal} options={goalOptions} onchange={(next) => (goal = next)} disabled={controller.saving} />
      </div>
      <label class="ui-field">{t('keep.findOwned')}<input class="ui-input" type="search" bind:value={search} placeholder={t('keep.searchPlaceholder')} /></label>
      <div class="ui-toolbar">
        <div class="ui-field"><span>{t('keep.itemToKeep')}</span>
          <Select label={t('keep.itemToKeep')} value={selected} options={itemOptions} onchange={(next) => (selected = next)} disabled={controller.saving} />
        </div>
        <label class="ui-field">{t('keep.extraCopies')}
          <input class="ui-input" type="number" min="0" max="1000000" step="1" bind:value={quantity} use:busy={controller.saving} onkeydown={(event) => { if (event.key === 'Enter' && !event.isComposing) { event.preventDefault(); reserve(); } }} />
        </label>
        <button class="btn" onclick={reserve} use:busy={controller.saving}>{t('keep.setQuantity')}</button>
      </div>
      {#each Object.entries(reserves) as [slug, count], index (slug)}
        <div class="ui-toolbar"><span>{names.get(slug) ?? slug} ×{count}</span><button class="btn ghost xs remove-rule" use:busy={controller.saving} onclick={() => {
          const next = { ...reserves }; delete next[slug]; reserves = next;
          // The row is gone; the next rule's Remove, or the item picker, takes focus.
          focusLater(() => {
            const left = [...dialog.querySelectorAll<HTMLElement>('.remove-rule')];
            return left[Math.min(index, left.length - 1)] ?? dialog.querySelector<HTMLElement>(`[aria-label="${CSS.escape(t('keep.itemToKeep'))}"], .ui-select`);
          });
        }}>{t('keep.remove', { item: names.get(slug) ?? slug })}</button></div>
      {/each}
      {#if formError}<p class="ui-notice" data-tone="bad" role="alert">{formError}</p>{/if}
      <div class="ui-toolbar"><button class="btn primary" onclick={save} use:busy={controller.saving || keepBusy}>{t(controller.saving ? 'keep.saving' : 'keep.save')}</button><button class="btn ghost" use:busy={controller.saving || keepBusy} onclick={() => dialog.close()}>{t('keep.cancel')}</button></div>
    </div>
  {/if}
    {#if controller.error}<p class="ui-notice" data-tone="bad" role="alert">{controller.error}</p>{/if}
  </div>
</dialog>

<style>
  details > * + * { margin-top: var(--s3); }
  p, ul { margin-bottom: 0; }
  .ui-input { max-width: 100%; }
  input[type="number"] { width: 8rem; }
  h3, p { margin: 0; }
  /* A line, not a panel: dotted rules above and below, like the other one-line strips under a view title. */
  .keeping { display: flex; flex-wrap: wrap; align-items: center; gap: var(--s1) var(--s3); min-width: 0; padding: var(--s1) 0; border-block: 1px var(--rule) var(--hairline); font-size: var(--text-control); }
  .keeping h3 { font: 500 var(--text-caption)/var(--leading-control) var(--font-ui); letter-spacing: 0.14em; text-transform: uppercase; color: var(--muted); }
  .keep-rules { min-width: 0; overflow-wrap: anywhere; }
  /* Expanded quantities get a full row without moving the keep-rule controls apart. */
  .keeping > details { min-width: 0; margin-left: 0; }
  .keeping > details[open] { flex: 1 0 100%; order: 1; margin-left: 0; }
  .keeping > details[open] ~ .change { margin-left: 0; }
  .keep-status { color: var(--muted); }
  .keep-status.verified { color: var(--good); }
  .keep-dialog { width: min(40rem, calc(100vw - var(--s6))); max-height: calc(100dvh - var(--s6)); overflow: auto; padding: var(--s5); background: var(--panel); color: var(--fg); border: 1px solid var(--border); }
  .keep-dialog::backdrop { background: var(--scrim); }
  /* Dialog titles are rails, as in listing review. */
  .keep-head { margin: calc(-1 * var(--s5)) calc(-1 * var(--s5)) var(--s4); padding: var(--s2) var(--s5); background: var(--rail-bg); color: var(--rail-fg); box-shadow: inset 0 2px 0 var(--rail-edge); }
  .keep-head h2 { margin: 0; font: 500 var(--text-caption)/1.6 var(--font-ui); letter-spacing: 0.16em; text-transform: uppercase; }
  table { min-width: 40rem; }
</style>
