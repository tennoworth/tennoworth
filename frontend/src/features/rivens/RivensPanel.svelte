<script lang="ts">
  import { useDesktopServices } from '../../ui/desktop-context';
  const { desktopRivenComps } = useDesktopServices();
  import RivenOffer from './RivenOffer.svelte';
  import CopyBtn from '../../ui/CopyBtn.svelte';

  import type { RivenAuction, RivenStatFilter } from '../../contracts/desktop';
  import { humanError } from '../../contracts/errors';
  import { humanWindow } from '../../ui/format';
  import {
    bandForRiven,
    compsFilterFor,
    dispoChangeFor,
    formatAuctionStat,
    isSplicedStat,
    polaritySymbol,
    resolveRivenStats,
    rivenReport,
    rivenSimilarity,
    spliceOptions,
    unreadFingerprintKeys,
    type OwnedRiven,
    type ResolvedRivenStat,
    type SpliceOption,
  } from '../../domain/rivens';
  import { describeComps } from '../../domain/riven-appraise';
  import type { Market, RivenAttribute } from '../../contracts/data';

  interface Props {
    /** Price snapshot - powers the weapon join (game_ref → slug), the DE
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

  type CompsScope = 'matched' | 'weapon';

  // One comps drawer open at a time, keyed by ROW: two rivens for the same
  // weapon are different rolls and must not share a drawer.
  let openRow = $state<string | null>(null);
  let scopeByRow = $state<Map<string, CompsScope>>(new Map());
  // Samples are cached per query, so two rows asking the same question share
  // one request against WFM's 10/min auction budget.
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

  interface Row {
    key: string;
    riven: OwnedRiven;
    stats: ResolvedRivenStat[];
    splices: SpliceOption[];
    filter: ReturnType<typeof compsFilterFor>;
    report: string | null;
    unread: string[];
  }

  let rows = $derived<Row[]>(
    sorted.map((riven, i) => {
      const stats = resolveRivenStats(riven, attrs);
      const rivenType = riven.slug ? market?.rivens?.weapons?.[riven.slug]?.riven_type : undefined;
      return {
        key: `${i}:${riven.path}:${riven.compat ?? ''}:${riven.rerolls}:${stats.map((s) => (s.positive ? '+' : '-') + s.tag).join(',')}`,
        riven,
        stats,
        splices: riven.veiled ? [] : spliceOptions(stats, rivenType),
        filter: compsFilterFor(stats),
        report: rivenReport(riven),
        unread: unreadFingerprintKeys(riven),
      };
    }),
  );

  // The DE weekly band for a riven, plus a note about which tier it is.
  // Falls back to the other tier when DE only published one.
  function bandText(r: OwnedRiven): { price: string; range: string; note: string } | null {
    const band = bandForRiven(r, market?.riven_stats);
    if (!band || !(band.median > 0)) return null;
    const rerolled = r.rerolls > 0;
    const entry = market?.riven_stats?.[r.slug ?? ''];
    const wanted = rerolled ? entry?.rolled : entry?.unrolled;
    const usedOther = !!entry && !wanted;
    return {
      price: band.median.toFixed(0) + 'p',
      range: band.min > 0 || band.max > 0 ? band.min.toFixed(0) + '–' + band.max.toFixed(0) + 'p' : '',
      note: (usedOther ? 'closest band · ' : '') + (rerolled ? 'rolled' : 'unrolled') + ' · popularity ' + band.pop + '/100',
    };
  }

  function scopeOf(row: Row): CompsScope {
    return scopeByRow.get(row.key) ?? (row.filter ? 'matched' : 'weapon');
  }

  function queryOf(row: Row): { key: string; stats: RivenStatFilter | null } | null {
    const slug = row.riven.slug;
    if (!slug) return null;
    if (scopeOf(row) === 'matched' && row.filter) {
      return { key: `${slug}|+${row.filter.filter.positive.join(',')}`, stats: row.filter.filter };
    }
    return { key: `${slug}|*`, stats: null };
  }

  async function loadComps(row: Row, force: boolean): Promise<void> {
    const q = queryOf(row);
    if (!q || !row.riven.slug) return;
    if (compsBusy === q.key) return;
    if (!force && compsCache.has(q.key)) return;
    compsBusy = q.key;
    compsError = new Map(compsError).set(q.key, '');
    try {
      const auctions = await desktopRivenComps(row.riven.slug, q.stats);
      compsCache = new Map(compsCache).set(q.key, auctions);
    } catch (e) {
      compsError = new Map(compsError).set(q.key, humanError(e));
    } finally {
      compsBusy = null;
    }
  }

  async function showComps(row: Row): Promise<void> {
    if (!row.riven.slug) return;
    if (openRow === row.key) {
      openRow = null;
      return;
    }
    openRow = row.key;
    await loadComps(row, false);
  }

  async function setScope(row: Row, scope: CompsScope): Promise<void> {
    if (scopeOf(row) === scope) return;
    scopeByRow = new Map(scopeByRow).set(row.key, scope);
    await loadComps(row, false);
  }

  /** Whole days since a listing instant; null when it is missing, malformed or
   *  in the future - an unknown age is never rendered as a fresh listing. */
  function listedDaysAgo(iso: string | null): number | null {
    const at = Date.parse(iso ?? '');
    if (!Number.isFinite(at)) return null;
    const days = Math.floor((Date.now() - at) / 86_400_000);
    return days < 0 ? null : days;
  }

  /** What this sample is, stated as facts. Listing age is not time-to-sale, so
   *  the line describes the sample and stops there. */
  function sampleLine(comps: RivenAuction[]): string {
    const read = describeComps(comps, Date.now());
    const parts = [`${read.size} cheapest buyout${read.size === 1 ? '' : 's'} in this sample`];
    const spliced = comps.filter((a) => a.attributes.some((s) => isSplicedStat(s.url_name))).length;
    if (spliced > 0) parts.push(`${spliced} with a spliced trait`);
    parts.push(read.dated === 0
      ? 'no listing dates in this response'
      : `${read.dated} dated · oldest ${read.oldestAskDays} d, newest ${read.newestAskDays} d`);
    const { online, ingame, offline, unknown } = read.status;
    const sellers = [
      online ? `${online} online` : '',
      ingame ? `${ingame} in game` : '',
      offline ? `${offline} offline` : '',
      unknown ? `${unknown} status unknown` : '',
    ].filter(Boolean);
    if (sellers.length) parts.push(`sellers ${sellers.join(', ')}`);
    parts.push('listing age is not time-to-sale');
    return parts.join(' · ');
  }

  function attrName(slug: string): string {
    return attrs?.find((a) => a.slug === slug)?.name ?? slug;
  }

  function spliceText(o: SpliceOption): string {
    const name = attrs?.find((a) => a.slug === o.recipe.result)?.name ?? o.recipe.name;
    const from = `${o.consumed[0].label} and ${o.consumed[1].label}`;
    return `${name} from ${from}${o.randomIsNegative ? ' (the new random trait is negative)' : ''}`;
  }

  function filterText(row: Row): string {
    if (!row.filter) return '';
    const names = row.filter.filter.positive.map((s) => '+' + attrName(s)).join(', ');
    return row.filter.complete ? names : `${names} (unrecognised stats left out)`;
  }
</script>

<section class="view-header"><h2>Rivens</h2><p class="lede">DE’s weekly band, the disposition trend, splice options, and live comparables — no single “worth N” number.</p></section>
<section class="wrap tw rivens" data-testid="rivens-view">
  <div class="rail">
    <h3>Owned rivens</h3>
    <span class="grow"></span>
    <span class="count"><b>{rivens.length}</b> owned{#if rivenStatsAge}&nbsp;· band data {rivenStatsAge}{/if}</span>
  </div>

  {#if rows.length === 0}
    <div class="line"><span class="exp">No rivens in your scanned inventory. Crack some relics or buy veiled ones.</span></div>
  {:else}
    <div class="scroll">
      <table class="tw fixed">
        <colgroup>
          <col style="width:13rem" />
          <col />
          <col style="width:3.5rem" />
          <col style="width:3.5rem" />
          <col style="width:7rem" />
          <col style="width:13rem" />
          <col style="width:8rem" />
        </colgroup>
        <thead>
          <tr>
            <th class="l">Weapon</th><th class="l">Stats</th><th>Rolls</th><th>Rank</th>
            <th class="l">Dispo</th><th>DE weekly</th><th>Actions</th>
          </tr>
        </thead>
        <tbody>
          {#each rows as row (row.key)}
            {@const r = row.riven}
            {@const change = dispoChangeFor(r.slug, market?.rivens)}
            {@const band = bandText(r)}
            {@const query = queryOf(row)}
            <tr>
              <td class="l">
                <div class="weapon">
                  {#if r.veiled}
                    <span class="veiled" title="Veiled riven - the stats are revealed by installing and completing its challenge.">Veiled</span>
                  {:else}
                    <strong>{r.weaponName ?? 'Unknown weapon'}</strong>
                    {#if r.pol}<span class="pol" title="Polarity">{polaritySymbol(r.pol)}</span>{/if}
                  {/if}
                </div>
              </td>
              <td class="l">
                <div class="stats" title={r.veiled ? 'Veiled - stats hidden until revealed.' : 'Stat names from the inventory fingerprint. Exact values require DE’s full Riven formula and are not guessed.'}>
                  {#if r.veiled}
                    <span class="muted">challenge to reveal</span>
                  {:else}
                    {#each row.stats as stat, si (si)}
                      <span class="stat" class:unknown={stat.slug == null}>{stat.label}{#if stat.spliced}<span class="spliced">spliced</span>{/if}</span>
                    {/each}
                  {/if}
                </div>
                {#if row.unread.length > 0}
                  <p class="unread" data-testid="unread-fingerprint">
                    This riven carries data the app does not read yet ({row.unread.join(', ')}), so a trait may be missing above. Copy data and report it.
                  </p>
                {/if}
                {#if row.splices.length > 0}
                  <p class="splices" data-testid="splice-options">
                    <span class="splices-label">Can splice:</span>
                    {row.splices.map(spliceText).join('; ')}
                  </p>
                {/if}
              </td>
              <td>{r.veiled ? '-' : r.rerolls}</td>
              <td>{r.veiled ? '-' : r.lvl}</td>
              <td class="l">
                {#if r.slug && market?.rivens?.weapons?.[r.slug]}
                  <span class="mono">{market.rivens.weapons[r.slug].disposition.toFixed(2)}</span>
                  {#if change && change.to !== change.from}
                    {@const up = change.to > change.from}
                    <span class="dispo-move" class:up class:down={!up} title={`Disposition ${change.from.toFixed(2)} → ${change.to.toFixed(2)} (${change.seen_at.slice(0, 10)})`}>
                      {up ? '▲' : '▼'} {Math.abs((change.to - change.from) * 100).toFixed(0)}%
                    </span>
                  {/if}
                {:else}
                  <span class="muted">-</span>
                {/if}
              </td>
              <td>
                {#if band}
                  <div class="band" title={band.note}>
                    <span class="mono">{band.price}</span>
                    {#if band.range}<span class="muted small"> {band.range}</span>{/if}
                    <span class="muted small note">{band.note}</span>
                  </div>
                  <!-- Still no "this riven is worth N": the offer comes from
                       the user, and we supply the arithmetic against DE's
                       distribution - placement and reroll cost. -->
                  <details class="offer-check">
                    <summary>check an offer</summary>
                    <RivenOffer riven={r} market={market} />
                  </details>
                {:else}
                  <span class="muted">-</span>
                {/if}
              </td>
              <td>
                <button
                  class="btn ghost"
                  onclick={() => showComps(row)}
                  disabled={!r.slug || compsBusy !== null}
                  title={r.slug ? 'Fetch the cheapest live buyouts for this riven (WFM caps at 10/min).' : 'Unknown weapon - no comps.'}
                >
                  {query != null && compsBusy === query.key ? 'Fetching…' : openRow === row.key ? 'Hide comps' : 'Comps'}
                </button>
                {#if row.report}
                  <!-- DE's raw riven data, for reporting a trait or state the
                       app does not read yet. No account details: the path
                       and fingerprint only. -->
                  <span class="copy-data" title="Copy DE's raw data for this riven, to paste into a bug report. It holds no account details.">
                    <CopyBtn text={row.report} label="Copy data" name={`Copy raw riven data for ${r.weaponName ?? 'this riven'}`} />
                  </span>
                {/if}
              </td>
            </tr>
            {#if openRow === row.key && query}
              {@const comps = compsCache.get(query.key)}
              <tr class="comps-row">
                <td colspan="7">
                  <div class="comps-bar">
                    <span class="ui-segmented xs" role="group" aria-label="Compare with">
                      <button type="button" aria-pressed={scopeOf(row) === 'matched'} disabled={!row.filter || compsBusy !== null} onclick={() => setScope(row, 'matched')}>Same positive stats</button>
                      <button type="button" aria-pressed={scopeOf(row) === 'weapon'} disabled={compsBusy !== null} onclick={() => setScope(row, 'weapon')}>All on weapon</button>
                    </span>
                    {#if scopeOf(row) === 'matched' && row.filter}
                      <span class="muted small">{filterText(row)}</span>
                    {/if}
                  </div>
                  {#if compsError.get(query.key)}
                    <div class="muted bad">Couldn't load comps: {compsError.get(query.key)}</div>
                  {:else if compsBusy === query.key}
                    <div class="muted">Fetching the cheapest buyouts for {r.weaponName}…</div>
                  {:else if comps && comps.length === 0}
                    <div class="muted">
                      {scopeOf(row) === 'matched'
                        ? 'No buyouts for this weapon share these positive stats right now. Try All on weapon.'
                        : 'No buyout auctions for this weapon right now.'}
                    </div>
                  {:else if comps}
                    <div class="comps">
                      <div class="comps-sample">
                        <span class="muted small" data-testid="comps-sample">{sampleLine(comps)}</span>
                        <button
                          class="btn ghost xs"
                          onclick={() => loadComps(row, true)}
                          disabled={compsBusy !== null}
                          title="Ask warframe.market again for the cheapest buyouts."
                        >Refresh sample</button>
                      </div>
                      {#each comps as a (a.id)}
                        {@const similarity = rivenSimilarity(r, a.attributes, attrs)}
                        <div class="comp">
                          <div class="comp-head">
                            <span class="mono price">{a.price}p</span>
                            <span class="muted small">
                              {a.is_direct_sell ? 'buyout' : 'auction buyout'}
                              {#if a.top_bid != null} · top bid {a.top_bid}p{/if}
                            </span>
                            <span class="comp-owner" title="WFM status">{a.owner ?? 'unknown'}{#if a.owner_status} · {a.owner_status}{/if}</span>
                            {#if similarity != null}
                              <span class="similarity" title="Matching signed stat names only; roll strength is not compared">{similarity}% stat match</span>
                            {/if}
                          </div>
                          <div class="comp-detail muted small">
                            {#if a.name}<span class="riven-name" title="The riven's rolled name">{a.name}</span>{/if}
                            <span>MR {a.mastery_level}</span>
                            <span>{a.re_rolls} reroll{a.re_rolls === 1 ? '' : 's'}</span>
                            {#if listedDaysAgo(a.created) != null}<span>listed {listedDaysAgo(a.created)} d ago</span>{/if}
                            {#if a.mod_rank > 0}<span>rank {a.mod_rank}</span>{/if}
                            {#if a.polarity}<span>{a.polarity}</span>{/if}
                          </div>
                          <div class="comp-stats small">
                            {#each a.attributes as s, ai (ai)}
                              <span class="stat">{formatAuctionStat(s.url_name, s.value, s.positive, attrs)}{#if isSplicedStat(s.url_name)}<span class="spliced">spliced</span>{/if}</span>
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
  /* Shell + table come from the shared .wrap.tw / table.tw anatomy in
     app.css; only riven-specific content styles live here. */
  table { min-width: 65rem; }
  td:last-child .btn { white-space: nowrap; }
  .copy-data { display: block; margin-top: var(--s1); }
  .copy-data :global(.copybtn) { min-height: var(--ctl-xs); }
  .mono { font-family: var(--font-mono); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .weapon { display: flex; align-items: center; gap: var(--s1); }
  .pol { color: var(--muted); font-size: var(--text-caption); }
  .veiled { color: var(--muted); font-style: italic; }
  /* Stats wrap to several lines; td height acts as a minimum in tables. */
  .stats { display: flex; flex-wrap: wrap; gap: 2px var(--s3); max-width: 420px; padding: var(--s1) 0; font-family: var(--font-body); color: var(--fg); }
  .stat { white-space: nowrap; }
  .dispo-move { font-family: var(--font-mono); font-size: var(--text-caption); margin-left: var(--s1); }
  .dispo-move.up { color: var(--good); }
  .dispo-move.down { color: var(--warn); }
  .stat.unknown { color: var(--muted); }
  .spliced { margin-left: var(--s1); color: var(--muted); font: var(--text-caption) var(--font-ui); }
  .splices { margin: 0 0 var(--s1); max-width: 420px; color: var(--muted); font: var(--text-caption)/var(--leading-body) var(--font-body); }
  .splices-label { font-family: var(--font-ui); color: var(--fg); }
  .unread { margin: 0 0 var(--s1); max-width: 420px; color: var(--warn); font: var(--text-caption)/var(--leading-body) var(--font-body); }
  .comps-bar { display: flex; align-items: center; flex-wrap: wrap; gap: var(--s1) var(--s2); padding-top: var(--s1); }
  .band { white-space: nowrap; }
  /* The tier and popularity note wraps under the price rather than being
     clipped by the fixed column. */
  .band .note { display: block; white-space: normal; }
  .offer-check { margin-top: 4px; white-space: normal; text-align: left; }
  .offer-check > summary { color: var(--muted); font: var(--text-caption)/var(--leading-body) var(--font-body); }
  .band .note { font-family: var(--font-body); }
  .small { font-size: var(--text-caption); }
  .muted { color: var(--muted); }
  .bad { color: var(--bad); }
  .similarity { color: var(--accent); border: 1px solid var(--accent); border-radius: var(--radius-ctl); padding: 1px 5px; font: 600 var(--text-caption) var(--font-mono); white-space: nowrap; }
  /* Comps expand as an inset drawer under the row, on the panel-2 ground. */
  .comps-row td { background: var(--panel-2); height: auto; text-align: left; font-family: var(--font-body); white-space: normal; }
  .comps { display: flex; flex-direction: column; gap: var(--s1); max-height: 320px; overflow: auto; padding: var(--s1) 0; }
  .comps-sample { display: flex; align-items: baseline; gap: var(--s2); flex-wrap: wrap; }
  .comp { border: 1px solid var(--border); border-radius: var(--radius-ctl); padding: var(--s2); display: flex; flex-direction: column; gap: 4px; background: var(--panel); }
  .comp-head { display: flex; align-items: baseline; gap: var(--s2); }
  .comp .price { font-weight: 700; font-size: var(--text-body); }
  .comp-owner { margin-left: auto; white-space: nowrap; font-family: var(--font-mono); }
  .comp-detail { display: flex; gap: var(--s2); flex-wrap: wrap; }
  .comp-stats { display: flex; flex-wrap: wrap; gap: 2px 12px; font-family: var(--font-body); color: var(--fg); }
  .riven-name { font-style: italic; }
</style>
