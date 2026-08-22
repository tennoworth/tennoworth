<script lang="ts">
  import RivenOffer from './RivenOffer.svelte';
  import { DesktopCmdError, desktopRivenComps, type RivenAuction } from '../lib/transport';
  import { humanError } from '../lib/errors';
  import { humanWindow } from '../lib/format';
  import {
    bandForRiven,
    dispoChangeFor,
    formatAuctionStat,
    formatRivenStat,
    polaritySymbol,
    type OwnedRiven,
  } from '../lib/rivens';
  import type { Market, RivenAttribute } from '../lib/types';

  interface Props {
    /** Price snapshot — powers the weapon join (game_ref → slug), the DE
     *  weekly band, the attributes manifest, and the disposition change. */
    market?: Market | null;
    /** Owned rivens, already resolved against the market. */
    rivens: OwnedRiven[];
  }
  let { market = null, rivens = [] }: Props = $props();

  const attrs = $derived<RivenAttribute[] | undefined>(market?.rivens?.attributes);

  const rivenStatsAge = $derived<string | null>(
    market?.surface_fetched_at?.riven_stats
      ? humanWindow(Date.now() - new Date(market.surface_fetched_at.riven_stats).getTime())
      : null,
  );

  // One comps drawer open at a time, keyed by weapon slug.
  let openSlug = $state<string | null>(null);
  let compsBusy = $state<string | null>(null);
  let compsCache = $state<Map<string, RivenAuction[]>>(new Map());
  let compsError = $state<Map<string, string>>(new Map());

  // Veiled rivens have no weapon to group by; keep them at the bottom.
  let sorted = $derived(
    [...rivens].sort((a, b) => {
      if (a.veiled !== b.veiled) return a.veiled ? 1 : -1;
      return (a.weaponName ?? '~').localeCompare(b.weaponName ?? '~');
    }),
  );

  // The DE weekly band for a riven, plus a note about which tier it is.
  // Falls back to the other tier when DE only published one.
  function bandText(r: OwnedRiven): { price: string; range: string; note: string } | null {
    const band = bandForRiven(r, market?.riven_stats);
    if (!band || band.pop === 0) return null;
    const rerolled = r.rerolls > 0;
    const entry = market?.riven_stats?.[r.slug ?? ''];
    const wanted = rerolled ? entry?.rolled : entry?.unrolled;
    const usedOther = !!entry && !wanted;
    return {
      price: band.median > 0 ? band.median.toFixed(0) + 'p' : '—',
      range: band.min > 0 || band.max > 0 ? band.min.toFixed(0) + '–' + band.max.toFixed(0) + 'p' : '',
      note: (usedOther ? 'closest band · ' : '') + (rerolled ? 'rolled' : 'unrolled') + ' · n=' + band.pop,
    };
  }

  async function showComps(r: OwnedRiven): Promise<void> {
    if (!r.slug) return;
    if (openSlug === r.slug) {
      openSlug = null;
      return;
    }
    openSlug = r.slug;
    if (compsCache.has(r.slug) || compsBusy === r.slug) return;
    compsBusy = r.slug;
    compsError = new Map(compsError).set(r.slug, '');
    try {
      const auctions = await desktopRivenComps(r.slug);
      const next = new Map(compsCache);
      next.set(r.slug, auctions);
      compsCache = next;
      if (auctions.length === 0) {
        compsError = new Map(compsError).set(r.slug, 'No open auctions for this weapon right now.');
      }
    } catch (e) {
      compsError = new Map(compsError).set(r.slug, e instanceof DesktopCmdError ? e.message : humanError(e));
    } finally {
      compsBusy = null;
    }
  }

  function statLines(r: OwnedRiven): string[] {
    const lines = r.buffs.map((s) => formatRivenStat(s.tag, s.value, true, attrs));
    lines.push(...r.curses.map((s) => formatRivenStat(s.tag, s.value, false, attrs)));
    return lines;
  }
</script>

<section class="card rivens" data-testid="rivens-view">
  <header class="row">
    <h2>Rivens</h2>
    <span class="muted">{rivens.length} owned</span>
  </header>

  <p class="muted lead">
    Your rivens against DE's weekly market band for the weapon (rolled vs unrolled) and the
    live comparables on warframe.market. No "this riven is worth N" — the band, the
    disposition trend, and the cheapest real auctions are the evidence.
    {#if rivenStatsAge}<span class="muted">· band data {rivenStatsAge}</span>{/if}
  </p>

  {#if sorted.length === 0}
    <div class="muted empty">No rivens in your scanned inventory. Crack some relics or buy veiled ones.</div>
  {:else}
    <div class="scroll">
      <table>
        <thead>
          <tr>
            <th>Weapon</th><th>Stats</th><th class="num">Rolls</th><th class="num">Rank</th>
            <th>Dispo</th><th class="num">DE weekly</th><th></th>
          </tr>
        </thead>
        <tbody>
          {#each sorted as r, i (r.path + i)}
            {@const change = dispoChangeFor(r.slug, market?.rivens)}
            {@const band = bandText(r)}
            <tr>
              <td>
                <div class="weapon">
                  {#if r.veiled}
                    <span class="veiled" title="Veiled riven — the stats are revealed by installing and completing its challenge.">Veiled</span>
                  {:else}
                    <strong>{r.weaponName ?? 'Unknown weapon'}</strong>
                    {#if r.pol}<span class="pol" title="Polarity">{polaritySymbol(r.pol)}</span>{/if}
                  {/if}
                </div>
              </td>
              <td>
                <div class="stats" title={r.veiled ? 'Veiled — stats hidden until revealed.' : statLines(r).join('\n')}>
                  {#if r.veiled}
                    <span class="muted">challenge to reveal</span>
                  {:else}
                    {#each statLines(r) as line (line)}
                      <span class="stat">{line}</span>
                    {/each}
                  {/if}
                </div>
              </td>
              <td class="num mono">{r.veiled ? '—' : r.rerolls}</td>
              <td class="num mono">{r.veiled ? '—' : r.lvl}</td>
              <td>
                {#if r.slug && market?.rivens?.weapons?.[r.slug]}
                  <span class="mono">{market.rivens.weapons[r.slug].disposition.toFixed(2)}</span>
                  {#if change}
                    <span class="dispo-move up" title={`Disposition ${change.from.toFixed(2)} → ${change.to.toFixed(2)} (${change.seen_at.slice(0, 10)})`}>
                      ▲ {((change.to - change.from) * 100).toFixed(0)}%
                    </span>
                  {/if}
                {:else}
                  <span class="muted">—</span>
                {/if}
              </td>
              <td class="num">
                {#if band}
                  <div class="band" title={band.note}>
                    <span class="mono">{band.price}</span>
                    {#if band.range}<span class="muted small"> {band.range}</span>{/if}
                    <span class="muted small"> · {band.note}</span>
                  </div>
                  <!-- Still no "this riven is worth N": the offer comes from
                       the user, and we supply the arithmetic against DE's
                       distribution — percentile, reroll odds and cost. -->
                  <details class="offer-check">
                    <summary>check an offer</summary>
                    <RivenOffer riven={r} market={market} />
                  </details>
                {:else}
                  <span class="muted">—</span>
                {/if}
              </td>
              <td>
                <button
                  class="tiny ghost"
                  onclick={() => showComps(r)}
                  disabled={!r.slug || compsBusy !== null}
                  title={r.slug ? 'Fetch the cheapest live auctions for this weapon (WFM caps at 10/min).' : 'Unknown weapon — no comps.'}
                >
                  {r.slug != null && compsBusy === r.slug ? 'Fetching…' : r.slug != null && openSlug === r.slug ? 'Hide comps' : 'Comps'}
                </button>
              </td>
            </tr>
            {#if openSlug === r.slug && r.slug}
              <tr class="comps-row">
                <td colspan="7">
                  {#if compsError.get(r.slug)}
                    <div class="muted bad">Couldn't load comps: {compsError.get(r.slug)}</div>
                  {:else if compsBusy === r.slug}
                    <div class="muted">Fetching the cheapest auctions for {r.weaponName}…</div>
                  {:else if (compsCache.get(r.slug) ?? []).length === 0}
                    <div class="muted">No comps to show.</div>
                  {:else}
                    <div class="comps">
                      {#each compsCache.get(r.slug) ?? [] as a (a.id)}
                        <div class="comp">
                          <div class="comp-head">
                            <span class="mono price">{a.price}p</span>
                            <span class="muted small">
                              {#if a.is_direct_sell}buyout{:else}bid{/if}
                              {#if a.buyout_price && !a.is_direct_sell} · buyout {a.buyout_price}p{/if}
                              {#if a.top_bid != null} · top bid {a.top_bid}p{/if}
                            </span>
                            <span class="comp-owner" title="WFM status">{a.owner ?? 'unknown'}{#if a.owner_status} · {a.owner_status}{/if}</span>
                          </div>
                          <div class="comp-detail muted small">
                            {#if a.name}<span class="riven-name" title="The riven's rolled name">{a.name}</span>{/if}
                            <span>MR {a.mastery_level}</span>
                            <span>{a.re_rolls} reroll{a.re_rolls === 1 ? '' : 's'}</span>
                            {#if a.mod_rank > 0}<span>rank {a.mod_rank}</span>{/if}
                            {#if a.polarity}<span>{a.polarity}</span>{/if}
                          </div>
                          <div class="comp-stats small">
                            {#each a.attributes as s (s.url_name)}
                              <span class="stat">{formatAuctionStat(s.url_name, s.value, s.positive, attrs)}</span>
                            {/each}
                          </div>
                        </div>
                      {/each}
                    </div>
                  {/if}
                </td>
              </tr>
            {/if}
          {/each}
        </tbody>
      </table>
    </div>
  {/if}

</section>

<style>
  .rivens { display: flex; flex-direction: column; gap: 10px; }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  h2 { margin: 0; font-size: 15px; }
  .lead { margin: 0; font-size: 12.5px; }
  table { width: 100%; border-collapse: collapse; font-size: 13px; }
  th { text-align: left; font-weight: 600; font-size: 11px; letter-spacing: .04em; text-transform: uppercase; color: var(--muted); padding: 6px 8px; border-bottom: 1px solid var(--border); }
  td { padding: 6px 8px; border-bottom: 1px solid var(--border); vertical-align: middle; }
  .num { text-align: right; }
  .mono { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .weapon { display: flex; align-items: center; gap: 6px; }
  .pol { color: var(--muted); font-size: 12px; }
  .veiled { color: var(--muted); font-style: italic; }
  .stats { display: flex; flex-wrap: wrap; gap: 2px 12px; max-width: 420px; }
  .stat { white-space: nowrap; }
  .dispo-move { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 11px; color: var(--good); margin-left: 6px; }
  .band { white-space: nowrap; }
  .offer-check { margin-top: 4px; white-space: normal; text-align: left; }
  .offer-check > summary { cursor: pointer; color: var(--muted); font-size: 0.75rem; }
  .small { font-size: 11px; }
  .muted { color: var(--muted); }
  .bad { color: var(--bad); }
  .empty { padding: 10px 0; }
  .scroll { overflow: auto; }
  .comps-row td { background: var(--panel-2); }
  .comps { display: flex; flex-direction: column; gap: 6px; max-height: 320px; overflow: auto; }
  .comp { border: 1px solid var(--border); border-radius: 8px; padding: 8px 10px; display: flex; flex-direction: column; gap: 4px; }
  .comp-head { display: flex; align-items: baseline; gap: 10px; }
  .comp .price { font-weight: 700; font-size: 14px; }
  .comp-owner { margin-left: auto; white-space: nowrap; }
  .comp-detail { display: flex; gap: 10px; flex-wrap: wrap; }
  .comp-stats { display: flex; flex-wrap: wrap; gap: 2px 12px; }
  .riven-name { font-style: italic; }
  button.ghost { background: transparent; color: var(--muted); border: 1px solid var(--border); padding: 4px 10px; border-radius: 6px; font-size: 12px; cursor: pointer; }
  button.ghost:hover:not(:disabled) { background: var(--panel-2); color: var(--fg); }
  button.tiny { padding: 2px 8px; font-size: 11px; }
  button:disabled { opacity: .5; cursor: default; }
</style>
