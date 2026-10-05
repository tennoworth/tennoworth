<script lang="ts">
import type { DomainResult } from '../selling/domain-result.svelte';
import type { RelicPlanEntry } from '../../contracts/generated/domain';
import { wfmItemUrl } from '../../ui/format';
import RefinementLadder from './RefinementLadder.svelte';
  let { relicShowAll = $bindable(false), relicResult, relicSurfaceAge, relicPlan, onretry }: {
    relicShowAll?: boolean;
    relicResult: DomainResult<RelicPlanEntry[]>;
    relicSurfaceAge: string | null;
    relicPlan: RelicPlanEntry[];
    onretry: () => void;
  } = $props();
const RELIC_PREVIEW = 6;
let relicVisible = $derived(relicShowAll ? relicPlan : relicPlan.slice(0, RELIC_PREVIEW));
</script>

      <section data-shell class="view-header">
        <h2 data-shell>Relic planner</h2>
        <p data-shell class="lede">
          {#if relicResult.phase === 'loading'}
            Expected values are being calculated from your inventory and the current snapshot.
          {:else if relicResult.error}
            Expected values are unavailable until the calculation succeeds.
          {:else if relicPlan.length > relicVisible.length}
            Top {relicVisible.length} of {relicPlan.length} relics you own, ranked by expected plat per solo crack (Intact); the ladder shows what refining would add.
          {:else}
            Your {relicPlan.length} relic{relicPlan.length === 1 ? '' : 's'} ranked by expected plat per solo crack (Intact); the ladder shows what refining would add.
          {/if}
          {#if relicSurfaceAge}
            <span data-shell class="muted">· ⚠ drop-table data {relicSurfaceAge}</span>
          {/if}
        </p>
      </section>
      {#if relicResult.phase === 'loading'}
        <div class="ui-notice" role="status">Calculating relic values…</div>
      {:else if relicResult.error}
        <div class="ui-notice" data-tone="bad" role="alert">Relic recommendations unavailable: {relicResult.error} <button class="btn" onclick={onretry}>Retry calculations</button></div>
      {:else if relicPlan.length > 0}
        <section data-shell class="wrap tw relic-planner">
          <div data-shell class="rail"><h3 data-shell>Relic decisions</h3></div>
          <div data-shell class="relic-grid">
            {#each relicVisible as p (p.relic_slug)}
              <div data-shell class="relic-card">
                <div data-shell class="relic-title">
                  <strong data-shell class="reco-verb">{p.decision?.verdict === 'sell-intact' ? 'Sell intact' : p.decision?.verdict === 'refine' ? 'Refine' : p.decision?.verdict === 'crack' ? 'Crack intact' : 'Review'}</strong>
                  <a data-shell
                    href={wfmItemUrl(p.relic_slug)}
                    target="_blank"
                    rel="noopener noreferrer"
                  >{p.relic_name}</a>
                  <span data-shell class="muted small">×{p.owned}</span>
                </div>
                <div data-shell class="relic-epp">
                  {p.epp.toFixed(1)}<span data-shell class="unit">p / crack</span>
                </div>
                <div data-shell class="relic-meta">
                  <span data-shell class:bad-text={p.moving_count < p.total_rewards / 2}>
                    {p.moving_count}/{p.total_rewards} rewards moving
                  </span>
                  <span data-shell class="muted">·</span>
                  <span data-shell title="Expected value of cracking every one you own; any single crack varies.">
                    ≈{p.epp_owned.toFixed(0)}p expected across {p.owned}
                  </span>
                  {#if p.sell_now > 0}
                    <span data-shell class="muted">·</span>
                    <span data-shell
                      class:bad-text={p.sell_now > p.epp}
                      title="What this relic clears at sold intact on WFM, no cracking. When this beats the crack EV, selling wins."
                    >or sell: {p.sell_now.toFixed(0)}p ea</span>
                  {/if}
                </div>
                {#if p.decision}
                  <RefinementLadder decision={p.decision} />
                {/if}
                <details data-shell class="relic-rewards">
                  <summary data-shell>top drops</summary>
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
              </div>
            {/each}
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

