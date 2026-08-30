<script lang="ts">
  // Build it or buy it, for one prime set.
  //
  // The set-vs-parts spread was already computed; what was missing is the
  // third option people actually take - buy only the parts you lack and
  // foundry the rest. DE's recipe tree is what makes that costable: credits,
  // real time, and the plat DE would charge to skip the wait.
  //
  // The resources are shown as an unchecked list, never as a solved cost. The
  // inventory scan sees tradeable items, not Orokin Cells, so claiming a build
  // is cheaper while silently assuming a full foundry would be wrong for
  // exactly the players who most need the answer.
  import {
    cheapestPath,
    humanBuildTime,
    planBuild,
    type BuildPathKind,
    type SetPart,
  } from '../lib/build-cost';
  import { glyphFor } from '../lib/glyphs';
  import type { Market, OwnedRecord } from '../lib/types';
  import Glyph from './Glyph.svelte';

  let {
    setSlug,
    setName,
    parts,
    market,
    owned = null,
  }: {
    setSlug: string;
    setName: string;
    parts: SetPart[];
    market: Market | null;
    owned?: Map<string, OwnedRecord> | null;
  } = $props();

  let plan = $derived(
    planBuild(setSlug, setName, parts, market, owned, market?.recipes ?? null),
  );
  let best = $derived(cheapestPath(plan));

  const LABEL: Record<BuildPathKind, string> = {
    'buy-set': 'Buy the set',
    'buy-parts-build': 'Buy missing parts, build',
    'buy-parts-rush': 'Buy missing parts, rush',
    'sell-spares': 'Sell your spares instead',
  };

  const OUTCOME: Record<BuildPathKind, string> = {
    'buy-set': 'assembled, ready now',
    'buy-parts-build': 'assembled, after the foundry',
    'buy-parts-rush': 'assembled, ready now',
    'sell-spares': 'plat, and no frame',
  };

  function credits(n: number): string {
    return n > 0 ? `${Math.round(n / 1000)}k cr` : '-';
  }
</script>

<section class="bvb">
  <header>
    <h4><Glyph name={glyphFor('set')} /> {setName}</h4>
    <p class="sub">
      {#if plan.have.length}
        You hold {plan.have.map((h) => h.name).join(', ')}.
      {/if}
      {#if plan.missing.length}
        Missing {plan.missing.length} of {parts.length}.
      {:else}
        You hold every part.
      {/if}
    </p>
  </header>

  <div class="scroll">
    <table>
      <thead>
        <tr>
          <th scope="col">Path</th>
          <th scope="col" class="num">Plat</th>
          <th scope="col" class="num">Credits</th>
          <th scope="col" class="num">Wait</th>
          <th scope="col">You end with</th>
          <th scope="col" class="num">vs set</th>
        </tr>
      </thead>
      <tbody>
        {#each plan.paths as path (path.kind)}
          <tr class:best={best?.kind === path.kind}>
            <th scope="row">{LABEL[path.kind]}</th>
            <td class="num">
              <!-- An unknown total renders as unknown. Printing the partial sum
                   with a warning underneath still asserts a number the data
                   cannot support, which is the failure this panel exists to
                   avoid. -->
              {#if !path.platKnown && path.kind !== 'sell-spares'}
                <span class="unknown" title="A part has no market price.">-</span>
              {:else if path.plat < 0}
                {path.platKnown ? '' : '≥ '}+{Math.round(-path.plat)}p
              {:else}
                {Math.round(path.plat)}p
              {/if}
            </td>
            <td class="num muted">{path.recipesKnown ? credits(path.credits) : '?'}</td>
            <td class="num muted">{path.recipesKnown ? humanBuildTime(path.seconds) : '?'}</td>
            <td class="muted">{OUTCOME[path.kind]}</td>
            <td class="num">
              {#if path.kind === 'sell-spares' || plan.setPrice == null || !path.platKnown}
                -
              {:else if path.savingVsSet > 0}
                <span class="good">−{Math.round(path.savingVsSet)}p</span>
              {:else if path.savingVsSet < 0}
                <span class="bad">+{Math.round(-path.savingVsSet)}p</span>
              {:else}
                0p
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

  {#if plan.incomplete}
    <p class="warn">
      No recommendation: {plan.missing
        .filter((m) => m.price == null)
        .map((m) => m.name)
        .join(', ')} has no market price, so any comparison would be a guess.
    </p>
  {/if}

  {#if !plan.paths.find((p) => p.kind === 'buy-parts-build')?.recipesKnown}
    <p class="note">
      No recipe data for this set in the current snapshot, so credits and foundry
      time are unknown rather than zero.
    </p>
  {/if}

  {#if plan.paths.find((p) => p.kind === 'buy-parts-build')?.unverified.length}
    <p class="note">
      Building also needs
      {plan.paths
        .find((p) => p.kind === 'buy-parts-build')!
        .unverified.map((i) => `${i.count}× ${i.name}`)
        .join(', ')} - resources the scan can't see, so they aren't costed above.
    </p>
  {/if}
</section>

<style>
  .bvb {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  h4 {
    margin: 0;
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }
  .sub,
  .note,
  .warn {
    margin: 0;
    font-size: 0.88rem;
    color: var(--muted);
  }
  .warn {
    color: var(--warn);
  }
  .scroll {
    overflow-x: auto;
    border: 1px solid var(--border);
    background: var(--panel);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    min-width: 38rem;
  }
  th,
  td {
    padding: 0.3rem 0.6rem;
    text-align: left;
    border-bottom: 1px solid var(--hairline);
    font-size: 0.88rem;
  }
  thead th {
    background: var(--panel-2);
    color: var(--muted);
    font-size: 0.75rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    white-space: nowrap;
  }
  tbody tr:last-child th,
  tbody tr:last-child td {
    border-bottom: 0;
  }
  tbody tr.best {
    background: color-mix(in srgb, var(--good) 8%, transparent);
  }
  .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .muted {
    color: var(--muted);
  }
  .good {
    color: var(--good);
  }
  .bad {
    color: var(--bad);
  }
  .unknown {
    color: var(--faint);
    cursor: help;
  }
</style>
