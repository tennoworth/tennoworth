<script lang="ts">
  import { fitTable } from '../../ui/table-fit';
  import { useDesktopServices } from '../shell/desktop-context';
  const { desktopRivenComps } = useDesktopServices();
  import RivenOffer from './RivenOffer.svelte';
  import CopyButton from '../../ui/CopyButton.svelte';
  import Pager from '../../ui/Pager.svelte';
  import { pageWindow } from '../../ui/paging';

  import type { RivenAuction, RivenStatFilter } from '../../contracts/desktop';
  import { humanError } from '../../contracts/errors';
  import { humanWindow, plat } from '../../ui/format';
  import { t } from '../../i18n/i18n.svelte';
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
  import { busy, roving } from '../../ui/keyboard-actions';
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

  let page = $state(0);
  let tableEl = $state<HTMLElement>();
  let win = $derived(pageWindow(sorted.length, page));
  // Only the page on screen is resolved; the key keeps the riven's place in
  // the whole sorted list so rows stay distinct across pages.
  let rows = $derived<Row[]>(
    sorted.slice(win.start, win.end).map((riven, j) => {
      const i = win.start + j;
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
  // Every row shares one log scale, so the bars compare down the column.
  // A band spans two or three orders of magnitude; linear would flatten
  // the median against the left edge.
  const BAND_SCALE_MAX = 10_000;
  const bandPos = (p: number) => (Math.log10(Math.min(Math.max(p, 1), BAND_SCALE_MAX)) / Math.log10(BAND_SCALE_MAX)) * 100;
  function bandText(r: OwnedRiven): { price: string; range: string; note: string; bar: { from: number; to: number; at: number } | null } | null {
    const band = bandForRiven(r, market?.riven_stats);
    if (!band || !(band.median > 0)) return null;
    const rerolled = r.rerolls > 0;
    const entry = market?.riven_stats?.[r.slug ?? ''];
    const wanted = rerolled ? entry?.rolled : entry?.unrolled;
    const usedOther = !!entry && !wanted;
    return {
      price: plat(band.median) + 'p',
      range: band.min > 0 || band.max > 0 ? plat(band.min) + '–' + plat(band.max) + 'p' : '',
      note: [usedOther ? t('riven.closestBand') : '', t(rerolled ? 'riven.rolled' : 'riven.unrolled'), t('riven.popularity', { pop: band.pop })].filter(Boolean).join(' · '),
      bar: band.min > 0 && band.max >= band.min ? { from: bandPos(band.min), to: bandPos(band.max), at: bandPos(band.median) } : null,
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
    const parts = [t('riven.sample.size', { count: read.size })];
    const spliced = comps.filter((a) => a.attributes.some((s) => isSplicedStat(s.url_name))).length;
    if (spliced > 0) parts.push(t('riven.sample.spliced', { count: spliced }));
    parts.push(read.dated === 0
      ? t('riven.sample.undated')
      : t('riven.sample.dated', { count: read.dated, oldest: read.oldestAskDays ?? 0, newest: read.newestAskDays ?? 0 }));
    const { online, ingame, offline, unknown } = read.status;
    const sellers = [
      online ? t('riven.sample.online', { count: online }) : '',
      ingame ? t('riven.sample.ingame', { count: ingame }) : '',
      offline ? t('riven.sample.offline', { count: offline }) : '',
      unknown ? t('riven.sample.unknown', { count: unknown }) : '',
    ].filter(Boolean);
    if (sellers.length) parts.push(t('riven.sample.sellers', { list: sellers.join(', ') }));
    parts.push(t('riven.sample.age'));
    return parts.join(' · ');
  }

  function attrName(slug: string): string {
    return attrs?.find((a) => a.slug === slug)?.name ?? slug;
  }

  function spliceText(o: SpliceOption): string {
    const name = attrs?.find((a) => a.slug === o.recipe.result)?.name ?? o.recipe.name;
    return t(o.randomIsNegative ? 'riven.spliceNegative' : 'riven.splice', { name, first: o.consumed[0].label, second: o.consumed[1].label });
  }

  function filterText(row: Row): string {
    if (!row.filter) return '';
    const names = row.filter.filter.positive.map((s) => '+' + attrName(s)).join(', ');
    return row.filter.complete ? names : t('riven.unrecognised', { names });
  }
</script>

<section class="view-header">
  <h2>{t('riven.title')}</h2>
  <p class="lede">{t('riven.lede')}</p>
  <div class="vh-end">
    <div class="ui-totals" role="group" aria-label={t('riven.summary')}>
      <div class="cell"><span class="k">{t('riven.owned')}</span><span class="v">{rivens.length}</span></div>
      {#if rivenStatsAge}<div class="cell"><span class="k">{t('riven.bandData')}</span><span class="v">{rivenStatsAge}</span></div>{/if}
    </div>
  </div>
</section>
<section class="wrap tw rivens" data-testid="rivens-view">
  <div class="rail">
    <h3>{t('riven.ownedTitle')}</h3>
    <span class="exp">{t('riven.ownedCount', { count: rivens.length })}</span>
  </div>

  {#if sorted.length === 0}
    <div class="line"><span class="exp">{t('riven.empty')}</span></div>
  {:else}
    <div class="scroll" use:fitTable bind:this={tableEl}>
      <table class="tw fixed">
        <colgroup>
          <col style="width:9rem" />
          <col style="width:12rem" />
          <col />
          <col style="width:3.5rem" />
          <col style="width:3.5rem" />
          <col style="width:7rem" />
          <col style="width:13rem" />
        </colgroup>
        <thead>
          <tr>
            <th class="l">{t('riven.col.weapon')}</th><th class="l"><span class="sr-only">{t('riven.col.actions')}</span></th><th class="l">{t('riven.col.stats')}</th><th>{t('riven.col.rolls')}</th><th data-drop="1">{t('riven.col.rank')}</th>
            <th class="l">{t('riven.col.dispo')}</th><th title={t('riven.col.deHelp')}>{t('riven.col.de')}</th>
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
                    <span class="veiled" title={t('riven.veiledHelp')}>{t('riven.veiled')}</span>
                  {:else}
                    <strong>{r.weaponName ?? t('riven.unknownWeapon')}</strong>
                    {#if r.pol}<span class="pol" title={t('riven.polarity')}>{polaritySymbol(r.pol)}</span>{/if}
                  {/if}
                </div>
              </td>
              <td class="l acts">
                <button
                  class="btn ghost"
                  onclick={() => showComps(row)}
                  disabled={!r.slug}
                  use:busy={compsBusy !== null}
                  aria-expanded={r.slug ? openRow === row.key : undefined}
                  title={t(r.slug ? 'riven.compsHelp' : 'riven.noComps')}
                >
                  {t(query != null && compsBusy === query.key ? 'riven.fetching' : openRow === row.key ? 'riven.hideComps' : 'riven.comps')}
                </button>
                {#if row.report}
                  <!-- DE's raw riven data, for reporting a trait or state the
                       app does not read yet. No account details: the path
                       and fingerprint only. -->
                  <span class="copy-data" title={t('riven.copyHelp')}>
                    <CopyButton text={row.report} label={t('riven.copyData')} name={t('riven.copyName', { weapon: r.weaponName ?? t('riven.thisRiven') })} />
                  </span>
                {/if}
              </td>
              <td class="l">
                <div class="stats" title={t(r.veiled ? 'riven.veiledStats' : 'riven.statsHelp')}>
                  {#if r.veiled}
                    <span class="muted">{t('riven.challenge')}</span>
                  {:else}
                    {#each row.stats as stat, si (si)}
                      <span class="stat" class:unknown={stat.slug == null}>{stat.label}{#if stat.spliced}<span class="spliced">{t('riven.spliced')}</span>{/if}</span>
                    {/each}
                  {/if}
                </div>
                {#if row.unread.length > 0}
                  <p class="unread" data-testid="unread-fingerprint">
                    {t('riven.unread', { keys: row.unread.join(', ') })}
                  </p>
                {/if}
                {#if row.splices.length > 0}
                  <p class="splices" data-testid="splice-options">
                    <span class="splices-label">{t('riven.canSplice')}</span>
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
                    <span class="dispo-move" class:up class:down={!up} title={t('riven.dispoMove', { from: change.from.toFixed(2), to: change.to.toFixed(2), date: change.seen_at.slice(0, 10) })}>
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
                    <!-- The text beside it carries the values; the bar only
                         shows where the median sits in the spread. -->
                    {#if band.bar}
                      <span class="range" aria-hidden="true">
                        <span class="span" style:left="{band.bar.from}%" style:width="{Math.max(band.bar.to - band.bar.from, 0.5)}%"></span>
                        <span class="mid" style:left="{band.bar.at}%"></span>
                      </span>
                    {/if}
                    <span class="muted small note">{band.note}</span>
                  </div>
                  <!-- Still no "this riven is worth N": the offer comes from
                       the user, and we supply the arithmetic against DE's
                       distribution - placement and reroll cost. -->
                  <details class="offer-check">
                    <summary>{t('riven.checkOffer')}</summary>
                    <RivenOffer riven={r} market={market} />
                  </details>
                {:else}
                  <span class="muted">-</span>
                {/if}
              </td>
            </tr>
            {#if openRow === row.key && query}
              {@const comps = compsCache.get(query.key)}
              <tr class="comps-row">
                <td colspan="7">
                  <div class="comps-bar">
                    <span class="ui-segmented xs" role="group" aria-label={t('riven.compareWith')} use:roving>
                      <button type="button" aria-pressed={scopeOf(row) === 'matched'} disabled={!row.filter || compsBusy !== null} onclick={() => setScope(row, 'matched')}>{t('riven.samePositive')}</button>
                      <button type="button" aria-pressed={scopeOf(row) === 'weapon'} disabled={compsBusy !== null} onclick={() => setScope(row, 'weapon')}>{t('riven.allOnWeapon')}</button>
                    </span>
                    {#if scopeOf(row) === 'matched' && row.filter}
                      <span class="muted small">{filterText(row)}</span>
                    {/if}
                  </div>
                  {#if compsError.get(query.key)}
                    <div class="muted bad">{t('riven.compsFailed', { error: compsError.get(query.key) ?? '' })}</div>
                  {:else if compsBusy === query.key}
                    <div class="muted">{t('riven.fetchingFor', { weapon: r.weaponName ?? t('riven.thisRiven') })}</div>
                  {:else if comps && comps.length === 0}
                    <div class="muted">
                      {scopeOf(row) === 'matched'
                        ? t('riven.noMatched', { all: t('riven.allOnWeapon') })
                        : t('riven.noBuyouts')}
                    </div>
                  {:else if comps}
                    <div class="comps">
                      <div class="comps-sample">
                        <span class="muted small" data-testid="comps-sample">{sampleLine(comps)}</span>
                        <button
                          class="btn ghost xs"
                          onclick={() => loadComps(row, true)}
                          disabled={compsBusy !== null}
                          title={t('riven.refreshHelp')}
                        >{t('riven.refresh')}</button>
                      </div>
                      {#each comps as a (a.id)}
                        {@const similarity = rivenSimilarity(r, a.attributes, attrs)}
                        <div class="comp">
                          <div class="comp-head">
                            <span class="mono price">{a.price}p</span>
                            <span class="muted small">
                              {t(a.is_direct_sell ? 'riven.buyout' : 'riven.auctionBuyout')}
                              {#if a.top_bid != null} · {t('riven.topBid', { price: a.top_bid })}{/if}
                            </span>
                            <span class="comp-owner" title={t('riven.wfmStatus')}>{a.owner ?? t('riven.unknown')}{#if a.owner_status}{` · ${a.owner_status}`}{/if}</span>
                            {#if similarity != null}
                              <span class="similarity" title={t('riven.matchHelp')}>{t('riven.match', { pct: similarity })}</span>
                            {/if}
                          </div>
                          <div class="comp-detail muted small">
                            {#if a.name}<span class="riven-name" title={t('riven.nameHelp')}>{a.name}</span>{/if}
                            <span>{t('riven.mr', { level: a.mastery_level })}</span>
                            <span>{t('riven.rerolls', { count: a.re_rolls })}</span>
                            {#if listedDaysAgo(a.created) != null}<span>{t('riven.listedAgo', { days: listedDaysAgo(a.created) ?? 0 })}</span>{/if}
                            {#if a.mod_rank > 0}<span>{t('riven.modRank', { rank: a.mod_rank })}</span>{/if}
                            {#if a.polarity}<span>{a.polarity}</span>{/if}
                          </div>
                          <div class="comp-stats small">
                            {#each a.attributes as s, ai (ai)}
                              <span class="stat">{formatAuctionStat(s.url_name, s.value, s.positive, attrs)}{#if isSplicedStat(s.url_name)}<span class="spliced">{t('riven.spliced')}</span>{/if}</span>
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
    <Pager window={win} total={sorted.length} label={t('riven.pages')} target={tableEl} onpage={(p) => (page = p)} />
  {/if}

</section>

<style>
  /* Shell + table come from the shared .wrap.tw / table.tw anatomy in
     app.css; only riven-specific content styles live here. */
  table { min-width: 65rem; }
  td:last-child .btn { white-space: nowrap; }
  /* Actions sit beside the weapon, so they stay in view while stats and the band scroll. */
  .acts { white-space: nowrap; }
  .copy-data { display: inline-block; margin-left: var(--s1); }
  .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
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
  /* min-max hairline and a median tick on a shared 1-10,000p log scale. */
  .range { position: relative; display: block; width: 100%; max-width: 10rem; height: 10px; margin: 3px 0 2px auto; }
  .range::before { content: ''; position: absolute; left: 0; right: 0; top: 50%; border-top: 1px dotted var(--faint); }
  .range .span { position: absolute; top: 50%; height: 0; border-top: 1px solid var(--muted); }
  .range .mid { position: absolute; top: 0; bottom: 0; width: 2px; margin-left: -1px; background: var(--fg); }
  .offer-check { margin-top: 4px; white-space: normal; text-align: left; }
  .offer-check > summary { color: var(--muted); font: var(--text-caption)/var(--leading-body) var(--font-body); }
  .band .note { font-family: var(--font-body); }
  .small { font-size: var(--text-caption); }
  .muted { color: var(--muted); }
  .bad { color: var(--bad); }
  .similarity { color: var(--accent); border: 1px solid var(--accent); border-radius: var(--radius-ctl); padding: 1px 5px; font: 600 var(--text-caption) var(--font-mono); white-space: nowrap; }
  /* Comps expand as an inset drawer under the row, on the panel-2 ground. */
  .comps-row td { background-color: var(--panel-2); height: auto; text-align: left; font-family: var(--font-body); white-space: normal; }
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
