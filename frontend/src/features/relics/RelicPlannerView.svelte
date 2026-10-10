<script lang="ts">
import type { DomainResult } from '../selling/domain-result.svelte';
import type { RelicPlanEntry } from '../../contracts/generated/domain';
import { wfmItemUrl } from '../../ui/format';
import { REFINE_WORTH_IT } from '../../domain/relic-ev';
import type { RelicDecision } from '../../contracts/generated/domain';
  let { relicShowAll = $bindable(false), relicResult, relicSurfaceAge, relicPlan, onretry }: {
    relicShowAll?: boolean;
    relicResult: DomainResult<RelicPlanEntry[]>;
    relicSurfaceAge: string | null;
    relicPlan: RelicPlanEntry[];
    onretry: () => void;
  } = $props();
const RELIC_PREVIEW = 6;
let relicVisible = $derived(relicShowAll ? relicPlan : relicPlan.slice(0, RELIC_PREVIEW));
// Every figure is a SOLO crack; a squad of four changes the maths.
const RUNGS = ['intact', 'exceptional', 'flawless', 'radiant'] as const;
let ownedTotal = $derived(relicPlan.reduce((n, p) => n + p.owned, 0));
let expectedTotal = $derived(relicPlan.reduce((n, p) => n + p.epp_owned, 0));
function rung(decision: RelicDecision | null | undefined, refinement: string) {
  return decision?.ladder.find((l) => l.refinement === refinement) ?? null;
}
function ladderMax(decision: RelicDecision | null | undefined): number {
  return Math.max(...(decision?.ladder.map((l) => l.ev) ?? []), 0.0001);
}
// Only a crack or refine verdict picks a rung; selling intact picks none.
function pickedRung(decision: RelicDecision | null | undefined): string | null {
  if (decision?.verdict === 'refine') return decision.best?.refinement ?? null;
  if (decision?.verdict === 'crack') return 'intact';
  return null;
}
function verdictWord(decision: RelicDecision | null | undefined): string {
  return decision?.verdict === 'sell-intact' ? 'Sell intact' : decision?.verdict === 'refine' ? 'Refine' : decision?.verdict === 'crack' ? 'Crack intact' : 'Review';
}
// The rung is chosen on plat per trace clearing a bar, not on the highest EV:
// per trace the rungs are nearly tied by construction, so ranking on that
// alone would flip on rounding.
function verdictText(decision: RelicDecision): string {
  if (decision.verdict === 'refine' && decision.best) return `Refine to ${decision.best.refinement}: ${decision.best.gainOverIntact.toFixed(0)}p more for ${decision.best.traces} traces (${decision.best.platPerTrace?.toFixed(2)}p per trace).`;
  if (decision.verdict === 'crack') return `Refining doesn't clear ${REFINE_WORTH_IT}p per trace here.`;
  if (decision.verdict === 'sell-intact') return 'The relic clears more intact than its contents.';
  if (decision.verdict === 'thin') return 'None of its rewards are trading - treat the EV as a guess.';
  return 'Not enough price data to judge.';
}
</script>

      <section data-shell class="view-header">
        <h2 data-shell>Relics</h2>
        <p data-shell class="lede">
          {#if relicResult.phase === 'loading'}
            Expected values are being calculated from your inventory and the current snapshot.
          {:else if relicResult.error}
            Expected values are unavailable until the calculation succeeds.
          {:else}
            Ranked by expected plat per solo crack; the ladder shows what refining would add.
          {/if}
          {#if relicSurfaceAge}
            <span data-shell class="muted">· ⚠ drop-table data {relicSurfaceAge}</span>
          {/if}
        </p>
        {#if relicResult.phase === 'done' && !relicResult.error && relicPlan.length > 0}
          <div data-shell class="vh-end">
            <div data-shell class="ui-totals" role="group" aria-label="Relics summary">
              <div data-shell class="cell"><span data-shell class="k">Relics</span><span data-shell class="v">{relicPlan.length} · ×{ownedTotal}</span></div>
              <div data-shell class="cell"><span data-shell class="k">Expected</span><span data-shell class="v" title="Expected value of cracking every relic you own; any single crack varies.">≈{expectedTotal.toFixed(0)}<span data-shell class="unit">p</span></span></div>
            </div>
          </div>
        {/if}
      </section>
      {#if relicResult.phase === 'loading'}
        <div class="ui-notice" role="status">Calculating relic values…</div>
      {:else if relicResult.error}
        <div class="ui-notice" data-tone="bad" role="alert">Relic recommendations unavailable: {relicResult.error} <button class="btn" onclick={onretry}>Retry calculations</button></div>
      {:else if relicPlan.length > 0}
        <section data-shell class="wrap tw relic-planner">
          <div data-shell class="rail"><h3 data-shell>Relic decisions</h3><span data-shell class="exp">{relicPlan.length > relicVisible.length ? `Top ${relicVisible.length} of ${relicPlan.length}` : `${relicPlan.length} ${relicPlan.length === 1 ? 'relic' : 'relics'}`} · expected platinum per solo crack</span></div>
          <!-- One row per relic, so thirty relics compare down columns instead
               of filling thirty cards. The refinement ladder is four cells on
               one scale per row; the recommended rung is the inverted cell. -->
          <div data-shell class="scroll">
          <table data-shell class="tw relic-table">
            <thead data-shell><tr data-shell>
              <th data-shell class="l relic-name">Relic</th><th data-shell>Own</th><th data-shell class="l">Decision</th>
              <th data-shell>Intact</th><th data-shell>Exceptional</th><th data-shell>Flawless</th><th data-shell>Radiant</th>
              <th data-shell>Expected</th><th data-shell>Or sell</th><th data-shell class="l why">Why</th>
            </tr></thead>
            <tbody data-shell>
            {#each relicVisible as p (p.relic_slug)}
              {@const max = ladderMax(p.decision)}
              <tr data-shell>
                <td data-shell class="l relic-name"><a data-shell href={wfmItemUrl(p.relic_slug)} target="_blank" rel="noopener noreferrer">{p.relic_name}</a></td>
                <td data-shell>×{p.owned}</td>
                <td data-shell class="l"><span data-shell class="tag solid">{verdictWord(p.decision)}</span></td>
                {#each RUNGS as refinement (refinement)}
                  {@const r = rung(p.decision, refinement)}
                  <td data-shell class="lad" class:pick={pickedRung(p.decision) === refinement}
                    title={r ? `${refinement}: ${r.ev.toFixed(1)}p expected, solo${r.traces ? ` · ${r.traces} traces · ${r.platPerTrace?.toFixed(2)}p per trace` : ''}` : undefined}>
                    {#if r}<span data-shell class="lv">{r.ev.toFixed(1)}</span><span data-shell class="lbar" aria-hidden="true"><i data-shell style="width:{Math.max(4, (r.ev / max) * 100)}%"></i></span>{:else}<span data-shell class="muted">-</span>{/if}
                  </td>
                {/each}
                <td data-shell class="price" title="Expected value of cracking every one you own; any single crack varies.">≈{p.epp_owned.toFixed(0)}<span data-shell class="unit">p</span></td>
                <td data-shell class:bad-text={p.sell_now > p.epp} title="What this relic clears at sold intact on WFM, no cracking. When this beats the crack EV, selling wins.">{#if p.sell_now > 0}{p.sell_now.toFixed(0)}<span data-shell class="unit">p ea</span>{:else}<span data-shell class="muted">-</span>{/if}</td>
                <td data-shell class="l why">
                  {#if p.decision}{verdictText(p.decision)}{/if}
                  <span data-shell class:bad-text={p.moving_count < p.total_rewards / 2}>{p.moving_count}/{p.total_rewards} rewards moving.</span>
                  <details data-shell class="relic-rewards">
                    <summary data-shell>Top drops</summary>
                    <ul data-shell>
                      {#each p.rewards.slice(0, 4) as r (r.slug)}
                        <li data-shell>
                          <span data-shell class="rarity rarity-{r.rarity.toLowerCase()}">{r.rarity[0]}</span>
                          <span data-shell class="reward-name">{r.name}</span>
                          <span data-shell class="muted small">{r.chance.toFixed(0)}%</span>
                          <span data-shell class={r.low_sell > 0 ? '' : 'muted'}>{r.low_sell || '-'}p</span>
                        </li>
                      {/each}
                    </ul>
                  </details>
                </td>
              </tr>
            {/each}
            </tbody>
          </table>
          </div>
          {#if relicPlan.length > RELIC_PREVIEW}
            <div data-shell class="relic-more">
              <button data-shell class="ghost" onclick={() => (relicShowAll = !relicShowAll)}>
                {relicShowAll ? 'Show fewer' : `Show ${relicPlan.length - RELIC_PREVIEW} more`}
              </button>
            </div>
          {/if}
        </section>
      {:else}
        <div data-shell class="card ui-panel empty">
          <div data-shell>
            <strong data-shell>No relics in your inventory.</strong>
            <p data-shell class="muted">Once you pick up relics, this planner ranks them by expected plat per crack.</p>
          </div>
        </div>
      {/if}

