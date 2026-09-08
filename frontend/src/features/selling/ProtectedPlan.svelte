<script lang="ts">
  import type { Market, OwnedRecord } from '../../contracts/data';
  import type { ProtectionController } from './protection.svelte';
  let { controller, owned, market }: { controller: ProtectionController; owned: Map<string, OwnedRecord>; market: Market | null } = $props();
  let editing = $state(false);
  let goal = $state('');
  let selected = $state('');
  let quantity = $state<number | undefined>(1);
  let reserves = $state<Record<string, number>>({});
  let formError = $state<string | null>(null);
  let choices = $derived([...owned.values()].filter(row => !row.subtype && !row.slug.endsWith('_set'))
    .filter((row, i, rows) => rows.findIndex(other => other.slug === row.slug) === i).sort((a, b) => a.name.localeCompare(b.name)));
  let names = $derived(new Map([...owned.values()].map(row => [row.slug, row.name])));
  function edit() {
    reserves = { ...controller.state?.plan.reserves };
    goal = controller.state?.plan.goal ?? '';
    formError = null;
    editing = true;
  }
  function reserve() {
    if (!selected || quantity == null || !Number.isSafeInteger(quantity) || quantity < 0 || quantity > 1_000_000) {
      formError = 'Choose an item and a whole quantity from 0 to 1000000.'; return;
    }
    reserves = { ...reserves, [selected]: quantity };
    formError = null;
  }
  async function save() {
    if (await controller.save({ reserves, goal: goal || null })) editing = false;
  }
</script>

<details class="ui-panel ui-stack">
  <summary>Protected selling plan · {controller.state?.plan.goal ? (market?.set_to_parts?.[controller.state.plan.goal]?.name ?? controller.state.plan.goal) : 'No pinned goal'}</summary>
  <p>Reserve unranked copies for a build or collection. A pinned set adds its required components to your manual quantities. Your global keep-copy setting still applies.</p>
  <p class="muted">Listed copies are already allocated. Unknown order or recipe data stays unavailable for new plans. Existing listings are changed only through listing review.</p>
  <div class="ui-toolbar">
    <button class="btn" onclick={() => controller.refresh()} disabled={controller.loading || controller.saving}>{controller.loading ? 'Checking allocation…' : 'Refresh allocation'}</button>
    <button class="btn" onclick={edit} disabled={!controller.state || controller.saving || editing}>Edit protection</button>
  </div>
  {#if controller.error}<p class="ui-notice" data-tone="bad" role="alert">{controller.error}</p>{/if}
  {#if controller.state?.issues.length}<ul class="ui-notice" data-tone="warn">{#each controller.state.issues as issue}<li>{issue}</li>{/each}</ul>{/if}
  {#if editing}
    <div class="ui-stack">
      <label class="ui-field">Pinned set goal
        <select class="ui-input" aria-label="Pinned set goal" bind:value={goal} disabled={controller.saving}>
          <option value="">No pinned goal</option>
          {#if goal && !market?.set_to_parts?.[goal]}<option value={goal}>{goal} · recipe unresolved</option>{/if}
          {#each Object.entries(market?.set_to_parts ?? {}).sort((a, b) => a[1].name.localeCompare(b[1].name)) as [slug, set]}
            <option value={slug}>{set.name}</option>
          {/each}
        </select>
      </label>
      <div class="ui-toolbar">
        <label class="ui-field">Item to protect
          <select class="ui-input" aria-label="Item to protect" bind:value={selected} disabled={controller.saving}><option value="">Choose an owned item</option>{#each choices as row}<option value={row.slug}>{row.name}</option>{/each}</select>
        </label>
        <label class="ui-field">Manual copies
          <input class="ui-input" type="number" min="0" max="1000000" step="1" bind:value={quantity} disabled={controller.saving} />
        </label>
        <button class="btn" onclick={reserve} disabled={controller.saving}>Set quantity</button>
      </div>
      {#each Object.entries(reserves) as [slug, count]}
        <div class="ui-toolbar"><span>{names.get(slug) ?? slug} ×{count}</span><button class="btn ghost xs" disabled={controller.saving} onclick={() => { const next = { ...reserves }; delete next[slug]; reserves = next; }}>Remove {names.get(slug) ?? slug}</button></div>
      {/each}
      {#if formError}<p class="ui-notice" data-tone="bad" role="alert">{formError}</p>{/if}
      <div class="ui-toolbar"><button class="btn primary" onclick={save} disabled={controller.saving}>{controller.saving ? 'Saving…' : 'Save protection'}</button><button class="btn ghost" disabled={controller.saving} onclick={() => editing = false}>Cancel protection edits</button></div>
    </div>
  {/if}
  {#if controller.state && Object.keys(controller.state.items).length}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex (Keyboard users must reach all allocation columns.) -->
    <div class="wrap tw" role="region" aria-label="Inventory allocation" tabindex="0">
      <table class="tw"><thead><tr><th class="l">Item</th><th>Owned</th><th>Protected / untradeable</th><th>Listed</th><th>Available</th></tr></thead>
        <tbody>{#each Object.entries(controller.state.items) as [slug, row]}<tr><td class="l">{names.get(slug) ?? slug}</td><td>{row.owned}</td><td>{row.protected}</td><td>{row.listed ?? 'Unknown'}</td><td>{row.available ?? 'Unknown'}</td></tr>{/each}</tbody>
      </table>
    </div>
  {/if}
</details>

<style>
  details > * + * { margin-top: var(--s3); }
  p, ul { margin-bottom: 0; }
  .ui-input { max-width: 100%; }
  input { width: 8rem; }
  table { min-width: 40rem; }
</style>
