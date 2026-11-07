<script lang="ts">
  import { fitTable } from '../../ui/table-fit';
import type { Snippet } from 'svelte';
import type { InventoryController } from '../inventory/controller.svelte';
import type { DomainResult } from './domain-result.svelte';
import type { SetReco, Verdict } from '../../contracts/generated/domain';
import { wfmItemUrl } from '../../ui/format';
import BuildVsBuy from './BuildVsBuy.svelte';
import ItemArt from '../../ui/ItemArt.svelte';
import Rich from '../../ui/Rich.svelte';
import { formatLocale, t } from '../../i18n/i18n.svelte';
  let { inventory, setSurfaceAge, keepSection, guidanceUnavailable, setResult, setRecos, adviceMap, onretry }: {
    inventory: InventoryController;
    setSurfaceAge: string | null;
    keepSection: Snippet;
    guidanceUnavailable: boolean;
    setResult: DomainResult<SetReco[]>;
    setRecos: SetReco[];
    adviceMap: Map<string, Verdict>;
    onretry: () => void;
  } = $props();
  // Each comparison is a native calculation over the snapshot, so it runs
  // when its section is first opened rather than for every collapsed row.
  let buildOpened = $state<Record<string, boolean>>({});

</script>

      <section data-shell class="view-header">
        <h2 data-shell>{t('sets.title')}</h2>
        <p data-shell class="lede">
          {t('sets.lede', { count: Object.keys(inventory.market?.set_to_parts ?? {}).length })}
          {#if setSurfaceAge}
            <span data-shell class="warn">{t('sets.dataAge', { age: setSurfaceAge })}</span>
          {/if}
        </p>
        {#if setResult.phase === 'done' && !guidanceUnavailable && setRecos.length > 0}
          <div data-shell class="vh-end">
            <div data-shell class="ui-totals" role="group" aria-label={t('sets.summary')}>
              <div data-shell class="cell"><span data-shell class="k">{t('sets.opportunities')}</span><span data-shell class="v">{setRecos.length}</span></div>
              <div data-shell class="cell"><span data-shell class="k">{t('sets.net')}</span><span data-shell class="v up">+{setRecos.reduce((n, r) => n + r.net_plat, 0).toLocaleString(formatLocale())}<span data-shell class="unit">p</span></span></div>
            </div>
          </div>
        {/if}
      </section>
      {@render keepSection()}
      {#if guidanceUnavailable}<p class="muted">{t('sets.waiting')}</p>
      {:else if setResult.phase === 'loading'}
        <div class="ui-notice" role="status">{t('sets.calculating')}</div>
      {:else if setResult.error}
        <div class="ui-notice" data-tone="bad" role="alert">{t('sets.unavailable', { error: setResult.error })} <button class="btn" onclick={onretry}>{t('sets.retry')}</button></div>
      {:else if setRecos.length > 0}
        <section data-shell class="wrap tw set-recos">
          <div data-shell class="rail"><h3 data-shell>{t('sets.panel')}</h3><span data-shell class="exp">{t('sets.count', { count: setRecos.length })}</span></div>
          <!-- A table like every other ranked list, so net plat and set volume
               compare down a column. The verb is a plain "Do" column: the old
               uppercase LIST read as a button and was only a label. -->
          <div data-shell class="scroll" use:fitTable>
          <table data-shell class="tw set-table">
            <thead data-shell><tr data-shell><th data-shell class="l set-name">{t('sets.col.set')}</th><th data-shell class="l verb">{t('sets.col.do')}</th><th data-shell>{t('sets.col.net')}</th><th data-shell>{t('sets.col.volume')}</th><th data-shell class="l reco-detail">{t('sets.col.detail')}</th></tr></thead>
            <tbody data-shell>
          {#each setRecos as r, i (r.set_slug)}
            {@const av = adviceMap.get(r.set_slug)}
            <tr data-shell>
              <td data-shell class="l set-name">
                <span data-shell class="rank">{i + 1}</span>
                <ItemArt slug={r.set_slug} />
                <a data-shell href={wfmItemUrl(r.set_slug)} target="_blank" rel="noopener noreferrer">{r.set_name}</a>
                <span data-shell class="kind kind-{r.kind}">
                  {#if r.kind === 'near-complete'}
                    {t('sets.ownParts', { owned: r.parts.reduce((n, p) => n + Math.min(p.count, p.required), 0), required: r.parts.reduce((n, p) => n + p.required, 0) })}
                  {:else if r.kind === 'complete-with-extras'}
                    {t('sets.sparesFull', { count: r.extras ?? 0 })}
                  {:else}
                    {t('sets.duplicates', { count: r.extras ?? 0 })}
                  {/if}
                </span>
                {#if av}
                  <span data-shell class="tag advice advice-{av.advice}" title={av.reasons.join(' · ')}>
                    {av.advice === 'sell_now' ? t('table.advice.sell_now') : av.advice === 'hold' ? t('table.advice.hold') : av.advice === 'neutral' ? t('table.advice.neutral') : av.advice}<span class="visually-hidden">: {av.reasons.join('; ')}</span>
                  </span>
                {/if}
              </td>
              <td data-shell class="l verb">{t(r.kind === 'near-complete' ? 'sets.completeSet' : 'sets.listSpares')}</td>
              <td data-shell class="price">+{r.net_plat}<span data-shell class="unit">p</span></td>
              <td data-shell>
                {#if r.set_vol !== undefined && (r.kind === 'near-complete' || r.kind === 'complete-with-extras')}
                  {#if r.set_vol < 1}
                    <span data-shell class="set-liq cold" title={t('sets.rarelyTitle')}>{t('sets.rarely')}<span class="visually-hidden">{t('sets.rarelySr')}</span></span>
                  {:else if r.set_vol < 5}
                    <span data-shell class="set-liq thin" title={t('sets.thinTitle')}>{t('sets.thin', { count: r.set_vol })}<span class="visually-hidden">{t('sets.thinSr')}</span></span>
                  {:else}
                    <span data-shell class="set-liq moving" title={t('sets.healthy')}>{r.set_vol}</span>
                  {/if}
                {:else}<span data-shell class="muted">-</span>{/if}
              </td>
              <td data-shell class="l reco-detail">
                {#if r.kind === 'near-complete'}
                  {@const ownedCount = r.parts.reduce((n, p) => n + Math.min(p.count, p.required), 0)}
                  <Rich text={t('sets.buy', { parts: (r.missing ?? []).map((m) => `${m.quantity > 1 ? `${m.quantity}× ` : ''}${m.name}`).join(' + '), cost: r.missing_cost ?? '', ask: r.set_low_sell ?? '' })} />
                  <Rich text={t('sets.uplift', { count: ownedCount, uplift: r.net_plat, parts: r.parts_low_sell ?? '' })} />
                  {#if r.set_top_buy !== undefined && r.instant_uplift !== undefined}
                    <Rich text={t(r.instant_uplift >= 0 ? 'sets.instantGain' : 'sets.instantLoss', { bid: r.set_top_buy, amount: Math.abs(r.instant_uplift) })} />
                  {/if}
                {:else if r.kind === 'complete-with-extras'}
                  <Rich text={t('sets.fullPlusExtras', { count: r.extras ?? 0, plat: r.extras_plat ?? '' })} />
                {:else}
                  <Rich text={t('sets.duplicateParts', { count: r.extras ?? 0, plat: r.extras_plat ?? '' })} />
                {/if}
                <!-- The third option the spread above cannot express: buy only
                     what you lack and foundry the rest. Only for sets you are
                     actually assembling - on a spares play there is nothing to
                     build. -->
                {#if r.kind === 'near-complete' && inventory.market?.set_to_parts?.[r.set_slug]}
                  <details data-shell class="build-vs-buy" ontoggle={(event) => { if (event.currentTarget.open) buildOpened[r.set_slug] = true; }}>
                    <summary data-shell>{t('sets.buildOrBuy')}</summary>
                    {#if buildOpened[r.set_slug]}
                      <BuildVsBuy
                        setSlug={r.set_slug}
                        setName={r.set_name}
                        parts={inventory.market.set_to_parts[r.set_slug].parts}
                        market={inventory.market}
                        owned={inventory.resolved.owned}
                      />
                    {/if}
                  </details>
                {/if}
              </td>
            </tr>
          {/each}
            </tbody>
          </table>
          </div>
        </section>
      {:else}
        <div data-shell class="card ui-panel empty">
          <div data-shell>
            <strong data-shell>{t('sets.noneTitle')}</strong>
            <p data-shell class="muted">{t('sets.noneBody')}</p>
          </div>
        </div>
      {/if}

