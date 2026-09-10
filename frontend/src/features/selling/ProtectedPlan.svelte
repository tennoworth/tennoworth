<script lang="ts">
  import type { Market, OwnedRecord } from '../../contracts/data';
  import type { ProtectionController } from './protection.svelte';
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
  let goalName = $derived(controller.savedPlan?.goal ? (market?.set_to_parts?.[controller.savedPlan.goal]?.name ?? controller.savedPlan.goal) : null);
  let manualCount = $derived(Object.keys(controller.savedPlan?.reserves ?? {}).length);
  let conflicts = $derived(Object.values(controller.state?.items ?? {}).some(row => row.protected + (row.listed ?? 0) > row.owned));
  let invalidInventory = $derived(controller.error?.includes('Inventory quantities are invalid') ?? false);
  async function changeKeep(value: number) {
    keepBusy = true; formError = null;
    try { await onsetKeep(value); await controller.refresh(); }
    catch { formError = 'The copy rule could not be saved. Try again.'; }
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
      formError = 'Choose an item and a whole quantity from 0 to 1000000.'; return;
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
    <strong>! We can’t calculate what you can sell yet</strong>
    <p>{controller.error ?? 'Keep rules could not be applied to this inventory. Quantities and sale totals are unavailable.'}</p>
    <div class="ui-toolbar"><button class="btn primary" disabled={scanning || controller.saving} onclick={() => invalidInventory ? onscan() : controller.refresh()}>{invalidInventory ? 'Scan game again' : 'Recheck quantities'}</button></div>
    <p class="muted">Your inventory is not being treated as empty. No listings have been changed.</p>
  </section>
{:else if conflicts && !controller.loading}
  <p class="ui-notice" data-tone="warn">△ Some keep rules or existing listings exceed the copies you own. Review the quantity details below.</p>
{:else if unavailableCount && !controller.loading}
  <p class="ui-notice" data-tone="warn">△ Quantities unavailable for {unavailableCount} {unavailableCount === 1 ? 'item' : 'items'}. These items are excluded from opportunities and totals. <button class="btn" onclick={() => controller.refresh()}>Recheck quantities</button></p>
{/if}
<section class="ui-panel ui-stack keeping" aria-label="What I’m keeping">
  <div class="ui-toolbar keeping-header"><div><h3>What I’m keeping</h3>
    <p>{reserveCopies} {reserveCopies === 1 ? 'copy' : 'copies'} of each item · Leveled copies stay out</p>
    {#if controller.savedPlan}<p class="muted">{goalName ? `Parts for ${goalName}` : 'No saved crafting goal'} · {manualCount} extra item {manualCount === 1 ? 'rule' : 'rules'}</p>{/if}
  </div><button class="btn" onclick={edit} disabled={controller.saving}>Change what I keep</button></div>
  <p class="keep-status" class:verified={!conflicts && !unavailable && !unavailableCount && !!controller.state && !controller.loading && !controller.error}>{controller.loading ? 'Checking quantities…' : unavailable ? 'Waiting for valid quantities' : conflicts ? 'Some quantities need review' : unavailableCount ? 'Applied to items with known quantities' : controller.state ? '✓ Keep rules applied to this inventory' : 'Waiting for inventory'}</p>
  <details>
    <summary>View quantity details</summary>
    <div class="ui-toolbar"><button class="btn" onclick={() => controller.refresh()} disabled={controller.loading || controller.saving}>{controller.loading ? 'Checking quantities…' : 'Recheck quantities'}</button>
    {#if controller.state?.issues.some(issue => issue.includes('Unlock WFM'))}<button class="btn" onclick={onconnect}>Connect WFM</button>{/if}</div>
    <p class="muted">Own minus kept copies and checked listings gives what you can sell. Without WFM, estimates do not subtract existing listings.</p>
    {#if controller.error && !unavailable}<p class="ui-notice" data-tone="bad">{controller.error}</p>{/if}
    {#if controller.state?.issues.length}<ul class="ui-notice" data-tone="warn">{#each controller.state.issues as issue}<li>{issue}</li>{/each}</ul>{/if}
    {#if controller.state && Object.keys(controller.state.items).length}
      <label class="ui-field">Find an item in quantity details<input class="ui-input" type="search" bind:value={detailSearch} oninput={() => detailLimit = 25} /></label>
      <!-- svelte-ignore a11y_no_noninteractive_tabindex (Keyboard users must reach every quantity column.) -->
      <div class="wrap tw" role="region" aria-label="Inventory allocation" tabindex="0">
        <table class="tw"><thead><tr><th class="l">Item</th><th>Own</th><th>Keep / untradeable</th><th>Listed</th><th>Can sell</th></tr></thead>
        <tbody>{#each detailRows.slice(0, detailLimit) as [slug, row]}<tr><td class="l">{names.get(slug) ?? slug}</td><td>{row.owned}</td><td>{row.protected}</td><td>{row.listed ?? 'Unknown'}</td><td>{row.available ?? (row.estimated == null ? 'Unavailable' : `${row.estimated} estimated`)}</td></tr>{/each}</tbody></table>
      </div>
      <div class="ui-toolbar"><span class="muted">Showing {Math.min(detailLimit, detailRows.length)} of {detailRows.length} items</span>{#if detailRows.length > detailLimit}<button class="btn" onclick={() => detailLimit += 25}>Show 25 more</button>{/if}</div>
    {/if}
  </details>
</section>
<dialog bind:this={dialog} class="keep-dialog" aria-labelledby="keep-title" onclose={() => editing = false} onkeydown={(event) => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); if (!controller.saving && !keepBusy) dialog.close(); } }} oncancel={(event) => { if (controller.saving || keepBusy) event.preventDefault(); }}>
  <div class="ui-stack">
    <h2 id="keep-title">What I’m keeping</h2>
    <p>Choose once. Apply across your inventory.</p>
    <label class="ui-field">Copies of every item · applies immediately
      <input class="ui-input" type="number" min="0" max="1000000" step="1" value={reserveCopies} disabled={controller.saving || keepBusy} onchange={(e) => { const n = Number(e.currentTarget.value); if (Number.isSafeInteger(n) && n >= 0 && n <= 1000000) void changeKeep(n); else formError = 'Choose a whole quantity from 0 to 1000000.'; }} />
    </label>
    <p class="muted">Leveled copies are excluded automatically. Extra item rules and crafting needs can keep more than this minimum.</p>
  {#if editing}
    <div class="ui-stack">
      {#if !controller.savedPlan}<p class="ui-notice" data-tone="warn">Saved item rules could not be loaded. Saving replaces them; re-enter every item and crafting goal you want to keep.</p>{/if}
      <label class="ui-field">Keep parts for a set
        <select class="ui-input" aria-label="Keep parts for a set" bind:value={goal} disabled={controller.saving}>
          <option value="">No saved crafting goal</option>
          {#if goal && !market?.set_to_parts?.[goal]}<option value={goal}>{goal} · recipe unresolved</option>{/if}
          {#each Object.entries(market?.set_to_parts ?? {}).sort((a, b) => a[1].name.localeCompare(b[1].name)) as [slug, set]}
            <option value={slug}>{set.name}</option>
          {/each}
        </select>
      </label>
      <label class="ui-field">Find an owned item<input class="ui-input" type="search" bind:value={search} placeholder="Search by item name" /></label>
      <div class="ui-toolbar">
        <label class="ui-field">Item to keep
          <select class="ui-input" aria-label="Item to keep" bind:value={selected} disabled={controller.saving}><option value="">Choose an owned item</option>{#each filteredChoices as row}<option value={row.slug}>{row.name}</option>{/each}</select>
        </label>
        <label class="ui-field">Extra copies
          <input class="ui-input" type="number" min="0" max="1000000" step="1" bind:value={quantity} disabled={controller.saving} />
        </label>
        <button class="btn" onclick={reserve} disabled={controller.saving}>Set quantity</button>
      </div>
      {#each Object.entries(reserves) as [slug, count]}
        <div class="ui-toolbar"><span>{names.get(slug) ?? slug} ×{count}</span><button class="btn ghost xs" disabled={controller.saving} onclick={() => { const next = { ...reserves }; delete next[slug]; reserves = next; }}>Remove {names.get(slug) ?? slug}</button></div>
      {/each}
      {#if formError}<p class="ui-notice" data-tone="bad" role="alert">{formError}</p>{/if}
      <div class="ui-toolbar"><button class="btn primary" onclick={save} disabled={controller.saving || keepBusy}>{controller.saving ? 'Saving…' : 'Save keep rules'}</button><button class="btn ghost" disabled={controller.saving || keepBusy} onclick={() => dialog.close()}>Cancel</button></div>
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
  .keeping-header { justify-content: space-between; }
  h3, p { margin: 0; }
  .keep-status { color: var(--muted); }
  .keep-status.verified { color: var(--good); }
  .keep-dialog { width: min(40rem, calc(100vw - var(--s6))); max-height: calc(100dvh - var(--s6)); overflow: auto; padding: var(--s5); background: var(--panel); color: var(--fg); border: 1px solid var(--border); }
  .keep-dialog::backdrop { background: var(--scrim); }
  table { min-width: 40rem; }
</style>
