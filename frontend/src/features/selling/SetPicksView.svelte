<script lang="ts">
import type { Snippet } from 'svelte';
import type { InventoryController } from '../inventory/controller.svelte';
import type { DomainResult } from './domain-result.svelte';
import type { SetReco, Verdict } from '../../contracts/generated/domain';
import { wfmItemUrl } from '../../ui/format';
import BuildVsBuy from './BuildVsBuy.svelte';
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

</script>

      <section data-shell class="view-header">
        <h2 data-shell>Set picks</h2>
        <p data-shell class="lede">
          Inventory cross-referenced against {Object.keys(inventory.market?.set_to_parts ?? {}).length}
          prime sets. Ranked by net plat.
          {#if setSurfaceAge}
            <span data-shell class="warn">· ⚠ set/vault data {setSurfaceAge}</span>
          {/if}
        </p>
      </section>
      {@render keepSection()}
      {#if guidanceUnavailable}<p class="muted">Set recommendations will appear once quantities are available.</p>
      {:else if setResult.phase === 'loading'}
        <div class="ui-notice" role="status">Calculating set opportunities…</div>
      {:else if setResult.error}
        <div class="ui-notice" data-tone="bad" role="alert">Set recommendations unavailable: {setResult.error} <button class="btn" onclick={onretry}>Retry calculations</button></div>
      {:else if setRecos.length > 0}
        <section data-shell class="wrap tw set-recos">
          <div data-shell class="rail"><h3 data-shell>Set opportunities</h3></div>
          {#each setRecos as r (r.set_slug)}
            {@const av = adviceMap.get(r.set_slug)}
            <div data-shell class="reco row">
              <div data-shell class="reco-body">
                <div data-shell class="reco-title">
                  <strong data-shell class="reco-verb">
                    {#if r.kind === 'near-complete'}Complete{:else if r.kind === 'complete-with-extras'}List{:else}List{/if}
                  </strong>
                  <a data-shell
                    href={wfmItemUrl(r.set_slug)}
                    target="_blank"
                    rel="noopener noreferrer"
                  >{r.set_name}</a>
                  <span data-shell class="reco-net-inline">+{r.net_plat}p</span>
                  {#if av}
                    <span data-shell class="tag advice advice-{av.advice}" title={av.reasons.join(' · ')}>
                      {av.advice === 'sell_now' ? 'sell now' : av.advice}
                    </span>
                  {/if}
                  <span data-shell class="kind kind-{r.kind}">
                    {#if r.kind === 'near-complete'}
                      own {r.parts.reduce((n, p) => n + Math.min(p.count, p.required), 0)}/{r.parts.reduce((n, p) => n + p.required, 0)}
                    {:else if r.kind === 'complete-with-extras'}
                      {r.extras} spare{r.extras === 1 ? '' : 's'} + full set
                    {:else}
                      {r.extras} duplicate{r.extras === 1 ? '' : 's'}
                    {/if}
                  </span>
                  {#if r.set_vol !== undefined && (r.kind === 'near-complete' || r.kind === 'complete-with-extras')}
                    {#if r.set_vol < 1}
                      <span data-shell class="set-liq cold" title="The assembled set has traded under 1×/48h - a flip may sit unsold for a while.">set rarely trades</span>
                    {:else if r.set_vol < 5}
                      <span data-shell class="set-liq thin" title="Thin set volume - expect to wait for a buyer before you recoup the plat.">thin · {r.set_vol}/48h</span>
                    {:else}
                      <span data-shell class="set-liq moving" title="Healthy set volume.">{r.set_vol}/48h</span>
                    {/if}
                  {/if}
                </div>
                <p data-shell class="reco-detail muted">
                  {#if r.kind === 'near-complete'}
                    {@const ownedCount = r.parts.reduce((n, p) => n + Math.min(p.count, p.required), 0)}
                    Buy {(r.missing ?? []).map((m) => `${m.quantity > 1 ? `${m.quantity}× ` : ''}${m.name}`).join(' + ')} at current asks for
                    <strong data-shell class="bad-text">{r.missing_cost}p</strong>, then list the set at the current lowest ask,
                    <strong data-shell class="good-text">{r.set_low_sell}p</strong>.
                    That is <strong data-shell class="good-text">+{r.net_plat}p potential uplift</strong> versus listing your
                    {ownedCount} owned part{ownedCount === 1 ? '' : 's'} for {r.parts_low_sell}p.
                    {#if r.set_top_buy !== undefined && r.instant_uplift !== undefined}
                      Selling instantly to the {r.set_top_buy}p top bid would be
                      <strong data-shell class:good-text={r.instant_uplift >= 0} class:bad-text={r.instant_uplift < 0}>{r.instant_uplift >= 0 ? '+' : '−'}{Math.abs(r.instant_uplift)}p</strong>
                      versus those parts.
                    {/if}
                  {:else if r.kind === 'complete-with-extras'}
                    You hold a full set plus {r.extras} spare blueprint{r.extras === 1 ? '' : 's'}.
                    List the extras at <strong data-shell>{r.extras_plat}p</strong>.
                  {:else}
                    Duplicates of partial-set parts. List the {r.extras} spare {r.extras === 1 ? 'copy' : 'copies'}:
                    <strong data-shell>{r.extras_plat}p</strong>.
                  {/if}
                </p>
                <!-- The third option the spread above cannot express: buy only
                     what you lack and foundry the rest. Only for sets you are
                     actually assembling - on a spares play there is nothing to
                     build. -->
                {#if r.kind === 'near-complete' && inventory.market?.set_to_parts?.[r.set_slug]}
                  <details data-shell class="build-vs-buy">
                    <summary data-shell>build it or buy it</summary>
                    <BuildVsBuy
                      setSlug={r.set_slug}
                      setName={r.set_name}
                      parts={inventory.market.set_to_parts[r.set_slug].parts}
                      market={inventory.market}
                      owned={inventory.resolved.owned}
                    />
                  </details>
                {/if}
              </div>
            </div>
          {/each}
        </section>
      {:else}
        <div data-shell class="card ui-panel empty">
          <div data-shell>
            <strong data-shell>No set recommendations.</strong>
            <p data-shell class="muted">You don't currently own enough prime parts to surface near-complete sets or spare-blueprint plays.</p>
          </div>
        </div>
      {/if}

