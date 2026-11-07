<script lang="ts">
  import { fitTable } from '../../ui/table-fit';
import type { DomainResult } from '../selling/domain-result.svelte';
import type { RelicPlanEntry } from '../../contracts/generated/domain';
import { plat, wfmItemUrl } from '../../ui/format';
import { formatLocale, t } from '../../i18n/i18n.svelte';
import ItemArt from '../../ui/ItemArt.svelte';
import { REFINE_WORTH_IT } from '../../domain/relic-ev';
import type { RelicDecision } from '../../contracts/generated/domain';
import type { CrackAnyway } from './crack-anyway.svelte';
  let { relicShowAll = $bindable(false), relicResult, relicSurfaceAge, relicPlan, onretry, crackAnyway }: {
    relicShowAll?: boolean;
    relicResult: DomainResult<RelicPlanEntry[]>;
    relicSurfaceAge: string | null;
    relicPlan: RelicPlanEntry[];
    onretry: () => void;
    /** The player's own "crack it anyway" calls; without it the advice stands. */
    crackAnyway?: CrackAnyway;
  } = $props();
const RELIC_PREVIEW = 6;
let relicVisible = $derived(relicShowAll ? relicPlan : relicPlan.slice(0, RELIC_PREVIEW));
// Every figure is a SOLO crack; a squad of four changes the maths.
const RUNGS = ['intact', 'exceptional', 'flawless', 'radiant'] as const;
let ownedTotal = $derived(relicPlan.reduce((n, p) => n + p.owned, 0));
// What following a row's decision is worth: selling intact where that clears
// more, otherwise cracking. A Sell intact row priced at its crack value
// undercounted the advice it gave, and the total with it.
// Selling intact and refining are the two pieces of advice a player can
// overrule with a plain crack; the row then counts the crack.
const overrulable = (p: RelicPlanEntry) => p.decision?.verdict === 'sell-intact' || p.decision?.verdict === 'refine';
const crackingAnyway = (p: RelicPlanEntry) => overrulable(p) && !!crackAnyway?.has(p.relic_slug);
const sellsIntact = (p: RelicPlanEntry) => p.decision?.verdict === 'sell-intact' && p.sell_now > 0 && !crackingAnyway(p);
const advisedValue = (p: RelicPlanEntry) => (sellsIntact(p) ? p.sell_now * p.owned : p.epp_owned);
let expectedTotal = $derived(relicPlan.reduce((n, p) => n + advisedValue(p), 0));
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
  const verdict = decision?.verdict;
  return t(verdict === 'sell-intact' || verdict === 'refine' || verdict === 'crack' ? `relic.verdict.${verdict}` : 'relic.verdict.review');
}
// The rung is chosen on plat per trace clearing a bar, not on the highest EV:
// per trace the rungs are nearly tied by construction, so ranking on that
// alone would flip on rounding.
function verdictText(decision: RelicDecision): string {
  if (decision.verdict === 'refine' && decision.best) return t('relic.why.refine', { rung: rungName(decision.best.refinement), gain: plat(decision.best.gainOverIntact), traces: decision.best.traces, perTrace: decimal(decision.best.platPerTrace, 2) });
  if (decision.verdict === 'crack') return t('relic.why.crack', { threshold: REFINE_WORTH_IT });
  if (decision.verdict === 'sell-intact') return t('relic.why.sell-intact');
  if (decision.verdict === 'thin') return t('relic.why.thin');
  return t('relic.why.none');
}
function rungName(refinement: string): string {
  return (RUNGS as readonly string[]).includes(refinement) ? t(`relic.rung.${refinement as (typeof RUNGS)[number]}`) : refinement;
}
function decimal(value: number | null | undefined, digits: number): string {
  return value == null ? '-' : value.toLocaleString(formatLocale(), { minimumFractionDigits: digits, maximumFractionDigits: digits });
}
function rarityInitial(rarity: string): string {
  const key = rarity.toLowerCase();
  return key === 'common' || key === 'uncommon' || key === 'rare' ? t(`relic.rarity.${key}`) : rarity[0];
}
</script>

      <section data-shell class="view-header">
        <h2 data-shell>{t('relic.title')}</h2>
        <p data-shell class="lede">
          {#if relicResult.phase === 'loading'}
            {t('relic.loading')}
          {:else if relicResult.error}
            {t('relic.failed')}
          {:else}
            {t('relic.lede')}
          {/if}
          {#if relicSurfaceAge}
            <span data-shell class="muted">· {t('relic.dropAge', { age: relicSurfaceAge })}</span>
          {/if}
        </p>
        {#if relicResult.phase === 'done' && !relicResult.error && relicPlan.length > 0}
          <div data-shell class="vh-end">
            <div data-shell class="ui-totals" role="group" aria-label={t('relic.summary')}>
              <div data-shell class="cell"><span data-shell class="k">{t('relic.title')}</span><span data-shell class="v">{relicPlan.length} · ×{ownedTotal}</span></div>
              <div data-shell class="cell"><span data-shell class="k">{t('relic.expected')}</span><span data-shell class="v" title={t('relic.expectedHelp')}>≈{plat(expectedTotal)}<span data-shell class="unit">p</span></span></div>
            </div>
          </div>
        {/if}
      </section>
      {#if relicResult.phase === 'loading'}
        <div class="ui-notice" role="status">{t('relic.calculating')}</div>
      {:else if relicResult.error}
        <div class="ui-notice" data-tone="bad" role="alert">{t('relic.unavailable', { error: relicResult.error })} <button class="btn" onclick={onretry}>{t('relic.retry')}</button></div>
      {:else if relicPlan.length > 0}
        <section data-shell class="wrap tw relic-planner">
          <div data-shell class="rail"><h3 data-shell>{t('relic.decisions')}</h3><span data-shell class="exp">{relicPlan.length > relicVisible.length ? t('relic.topOf', { shown: relicVisible.length, total: relicPlan.length }) : t('relic.count', { count: relicPlan.length })} · {t('relic.perCrack')}</span></div>
          <!-- One row per relic, so thirty relics compare down columns instead
               of filling thirty cards. The refinement ladder is four cells on
               one scale per row; the recommended rung is the inverted cell. -->
          <div data-shell class="scroll" use:fitTable>
          <table data-shell class="tw relic-table">
            <thead data-shell><tr data-shell>
              <th data-shell class="l relic-name">{t('relic.col.relic')}</th><th data-shell>{t('relic.col.own')}</th><th data-shell class="l">{t('relic.col.decision')}</th>
              <th data-shell>{t('relic.rung.intact')}</th><th data-shell data-drop="1">{t('relic.rung.exceptional')}</th><th data-shell data-drop="1">{t('relic.rung.flawless')}</th><th data-shell>{t('relic.rung.radiant')}</th>
              <th data-shell>{t('relic.expected')}</th><th data-shell data-drop="3">{t('relic.col.orSell')}</th><th data-shell class="l why" data-drop="2">{t('relic.col.why')}</th>
            </tr></thead>
            <tbody data-shell>
            {#each relicVisible as p (p.relic_slug)}
              {@const max = ladderMax(p.decision)}
              {@const cracking = crackingAnyway(p)}
              <tr data-shell>
                <td data-shell class="l relic-name"><span class="item-id"><ItemArt slug={p.relic_slug} /><a data-shell href={wfmItemUrl(p.relic_slug)} target="_blank" rel="noopener noreferrer">{p.relic_name}</a></span></td>
                <td data-shell>×{p.owned}</td>
                <td data-shell class="l decision">
                  <span data-shell class="tag solid">{cracking ? t('relic.cracking') : verdictWord(p.decision)}</span>
                  <!-- One button that changes its words, so focus stays on it. -->
                  {#if crackAnyway && overrulable(p)}
                    <button data-shell type="button" class="overrule" onclick={() => crackAnyway?.set(p.relic_slug, !cracking)}
                      aria-label={cracking ? t('relic.follow', { relic: p.relic_name, verdict: verdictWord(p.decision) }) : t('relic.crackName', { relic: p.relic_name })}
                    >{cracking ? t('relic.fine', { verdict: verdictWord(p.decision).toLocaleLowerCase(formatLocale()) }) : t('relic.crackAnyway')}</button>
                  {/if}
                </td>
                {#each RUNGS as refinement (refinement)}
                  {@const r = rung(p.decision, refinement)}
                  <td data-shell class="lad" class:pick={(cracking ? 'intact' : pickedRung(p.decision)) === refinement}
                    title={r ? (r.traces ? t('relic.rungTraces', { rung: rungName(refinement), ev: decimal(r.ev, 1), traces: r.traces, perTrace: decimal(r.platPerTrace, 2) }) : t('relic.rungHelp', { rung: rungName(refinement), ev: decimal(r.ev, 1) })) : undefined}>
                    {#if r}<span data-shell class="lv">{decimal(r.ev, 1)}</span><span data-shell class="lbar" aria-hidden="true"><i data-shell style="width:{Math.max(4, (r.ev / max) * 100)}%"></i></span>{:else}<span data-shell class="muted">-</span>{/if}
                  </td>
                {/each}
                <td data-shell class="price" title={sellsIntact(p) ? t('relic.sellAll', { count: p.owned, price: plat(p.sell_now) }) : t('relic.crackAll')}>≈{plat(advisedValue(p))}<span data-shell class="unit">p</span></td>
                <td data-shell class:bad-text={p.sell_now > p.epp} title={t('relic.orSellHelp')}>{#if p.sell_now > 0}{plat(p.sell_now)}<span data-shell class="unit">{t('relic.each')}</span>{:else}<span data-shell class="muted">-</span>{/if}</td>
                <td data-shell class="l why">
                  {#if cracking && p.decision}{t('relic.yourCall', { advice: verdictText(p.decision) })}
                  {:else if p.decision}{verdictText(p.decision)}{/if}
                  <span data-shell class:bad-text={p.moving_count < p.total_rewards / 2}>{t('relic.moving', { moving: p.moving_count, total: p.total_rewards })}</span>
                  <details data-shell class="relic-rewards">
                    <summary data-shell>{t('relic.topDrops')}</summary>
                    <ul data-shell>
                      {#each p.rewards.slice(0, 4) as r (r.slug)}
                        <li data-shell>
                          <span data-shell class="rarity rarity-{r.rarity.toLowerCase()}">{rarityInitial(r.rarity)}</span>
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
                {relicShowAll ? t('relic.showFewer') : t('relic.showMore', { count: relicPlan.length - RELIC_PREVIEW })}
              </button>
            </div>
          {/if}
        </section>
      {:else}
        <div data-shell class="card ui-panel empty">
          <div data-shell>
            <strong data-shell>{t('relic.empty')}</strong>
            <p data-shell class="muted">{t('relic.emptyHelp')}</p>
          </div>
        </div>
      {/if}

