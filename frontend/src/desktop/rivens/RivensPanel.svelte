<script lang="ts">
  import { fitTable } from '../../ui/table-fit';
  import { useDesktopServices } from '../shell/desktop-context';
  const { desktopRivenComps, desktopPriceSharing, desktopRollVotes, desktopSendRollVote, desktopCommunityRivenPrices } = useDesktopServices();
  import RivenOffer from './RivenOffer.svelte';
  import RollRating from './RollRating.svelte';
  import { onMount } from 'svelte';
  import type { CommunityRivenPrice, RollVote } from '../../contracts/price-reports';
  import CopyButton from '../../ui/CopyButton.svelte';
  import Pager from '../../ui/Pager.svelte';
  import { pageWindow } from '../../ui/paging';

  import type { RivenAuction, RivenStatFilter } from '../../contracts/desktop';
  import { humanError } from '../../contracts/errors';
  import { humanWindow, plat } from '../../ui/format';
  import { t } from '../../i18n/i18n.svelte';
  import {
    bandForRiven,
    dispoChangeFor,
    formatAuctionStat,
    isSplicedStat,
    polaritySymbol,
    readRivenRows,
    rivenReport,
    rivenSimilarities,
    type OwnedRiven,
    type RivenRowRead,
    type RollCheck,
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
  // The offer check opens under its row too, independently of comps.
  let openOffer = $state<string | null>(null);
  let scopeByRow = $state<Map<string, CompsScope>>(new Map());
  // Samples are cached per query, so two rows asking the same question share
  // one request against WFM's 10/min auction budget.
  let compsBusy = $state<string | null>(null);
  let compsCache = $state<Map<string, RivenAuction[]>>(new Map());
  let compsError = $state<Map<string, string>>(new Map());

  // Grouped by weapon first; veiled rivens have no weapon, so they go last.
  let byWeapon = $derived(
    [...rivens].sort((a, b) => {
      if (a.veiled !== b.veiled) return a.veiled ? 1 : -1;
      return (a.weaponName ?? '~').localeCompare(b.weaponName ?? '~');
    }),
  );

  interface Row extends RivenRowRead {
    key: string;
    riven: OwnedRiven;
    report: string | null;
    verdict: Verdict;
  }

  type Verdict = 'sellable' | 'almost' | 'weak' | 'unrated';
  type VerdictFilter = 'all' | Verdict;
  const VERDICT_ORDER: Record<Verdict, number> = { sellable: 0, almost: 1, weak: 2, unrated: 3 };
  const FILTERS: VerdictFilter[] = ['all', 'sellable', 'almost', 'weak', 'unrated'];
  const FILTER_LABEL = {
    all: 'riven.filter.all', sellable: 'riven.filter.sellable', almost: 'riven.filter.almost', weak: 'riven.filter.weak', unrated: 'riven.filter.unrated',
  } as const;

  // Every riven is read, not just the page on screen: the list is ordered by
  // verdict, so the verdicts are needed before paging. Rivens number in the
  // tens, and the read is one native call.
  let reads = $derived.by<Row[]>(() => {
    const read = readRivenRows(
      byWeapon.map((riven) => ({ riven, rivenType: riven.slug ? market?.rivens?.weapons?.[riven.slug]?.riven_type : undefined })),
      attrs,
    );
    return byWeapon.map((riven, i) => {
      const row = read[i]!;
      return {
        ...row,
        // The key keeps the riven's place in the weapon order, so rows stay
        // distinct and keep their open drawers when the filter changes.
        key: `${i}:${riven.path}:${riven.compat ?? ''}:${riven.rerolls}:${row.stats.map((s) => (s.positive ? '+' : '-') + s.tag).join(',')}`,
        riven,
        report: rivenReport(riven),
        verdict: row.roll?.kind === 'read' ? row.roll.verdict : 'unrated',
      };
    });
  });

  // Best verdict first, so the rivens worth listing lead; weapon order holds
  // within a verdict. Array sort is stable.
  let ordered = $derived([...reads].sort((a, b) => VERDICT_ORDER[a.verdict] - VERDICT_ORDER[b.verdict]));
  let counts = $derived.by(() => {
    const c: Record<VerdictFilter, number> = { all: reads.length, sellable: 0, almost: 0, weak: 0, unrated: 0 };
    for (const r of reads) c[r.verdict]++;
    return c;
  });

  let verdictFilter = $state<VerdictFilter>('all');
  let shown = $derived(verdictFilter === 'all' ? ordered : ordered.filter((r) => r.verdict === verdictFilter));
  function setFilter(f: VerdictFilter) {
    verdictFilter = f;
    page = 0;
  }

  let page = $state(0);
  let tableEl = $state<HTMLElement>();
  let win = $derived(pageWindow(shown.length, page));
  let rows = $derived(shown.slice(win.start, win.end));

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

  // Ratings and players' prices ride on price sharing; without it, or when it
  // cannot load, the view is exactly as before.
  let sharing = $state<'on' | 'off' | 'unavailable'>('unavailable');
  let myVotes = $state<RollVote[]>([]);
  let community = $state<CommunityRivenPrice[]>([]);
  onMount(() => {
    desktopPriceSharing()
      .then((p) => { sharing = !p.available ? 'unavailable' : p.enabled ? 'on' : 'off'; })
      .catch(() => {});
    desktopRollVotes().then((v) => { myVotes = v; }).catch(() => {});
    desktopCommunityRivenPrices().then((c) => { community = c; }).catch(() => {});
  });
  async function vote(v: RollVote): Promise<void> {
    myVotes = await desktopSendRollVote(v);
  }
  /** Players' reported price for this riven's weapon under its verdict. */
  function playersFor(row: Row): CommunityRivenPrice | undefined {
    if (!row.riven.slug || row.verdict === 'unrated') return undefined;
    return community.find((c) => c.weapon === row.riven.slug && c.verdict === row.verdict);
  }

  // Changes to the roll guide come in as suggestions; the form asks for the
  // weapon, the change and the evidence for it.
  const ROLL_GUIDE_ISSUE = 'https://github.com/tennoworth/tennoworth/issues/new?template=riven-roll.yml';
  const VERDICT = { sellable: 'riven.roll.sellable', almost: 'riven.roll.almost', weak: 'riven.roll.weak' } as const;

  type RollRead = Extract<RollCheck, { kind: 'read' }>;

  // A word joiner keeps the sign on the line with its stat; a bare hyphen is a
  // break opportunity, which split "-" from "Zoom" in a narrow cell.
  function statNames(slugs: string[], sign: '+' | '-'): string {
    return slugs.map((s) => sign + '\u2060' + attrName(s)).join(', ');
  }

  /** Why the verdict is what it is: what is missing, unwanted or harmful, or
   *  for a sellable roll, that its negative does no harm here. */
  function rollReasons(read: RollRead): string {
    const parts: string[] = [];
    if (read.verdict === 'sellable') parts.push(t('riven.roll.matches'));
    if (read.missing.length) parts.push(t('riven.roll.missing', { stats: statNames(read.missing, '+') }));
    if (read.dead.length) parts.push(t('riven.roll.dead', { stats: statNames(read.dead, '+'), count: read.dead.length }));
    if (read.hurts) parts.push(t('riven.roll.hurts', { stat: statNames([read.hurts], '-') }));
    else if (read.harmless) parts.push(t('riven.roll.harmless', { stat: statNames([read.harmless], '-') }));
    return parts.join(' · ');
  }

  /** The stats the guide lists for the weapon, shown on a weak roll. */
  function wantedStats(read: RollRead): string {
    return statNames([...read.need, ...read.pick], '+');
  }

  /** A stat's name without its sign, which the list draws in its own gutter. */
  function statName(label: string): string {
    return label.replace(/^[+-]/, '');
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
      {#if rivens.length > 0}<div class="cell"><span class="k">{t('riven.sellableCount')}</span><span class="v">{counts.sellable}</span></div>{/if}
      {#if rivenStatsAge}<div class="cell"><span class="k">{t('riven.bandData')}</span><span class="v">{rivenStatsAge}</span></div>{/if}
    </div>
  </div>
</section>
<section class="wrap tw rivens" data-testid="rivens-view">
  <div class="rail">
    <h3>{t('riven.ownedTitle')}</h3>
    <span class="exp">{t('riven.ownedCount', { count: rivens.length })}</span>
  </div>

  {#if byWeapon.length === 0}
    <div class="line"><span class="exp">{t('riven.empty')}</span></div>
  {:else}
    <div class="bar">
      <span class="ui-segmented xs" role="group" aria-label={t('riven.filter')} data-testid="verdict-filter" use:roving>
        {#each FILTERS as f (f)}
          <button type="button" aria-pressed={verdictFilter === f} disabled={f !== 'all' && counts[f] === 0} onclick={() => setFilter(f)}>{t(FILTER_LABEL[f], { count: counts[f] })}</button>
        {/each}
      </span>
      <span class="exp">{t('riven.sortedByVerdict')}</span>
    </div>
    <div class="scroll" use:fitTable bind:this={tableEl}>
      <table class="tw fixed">
        <colgroup>
          <col style="width:9.5rem" />
          <col style="width:13rem" />
          <col />
          <col style="width:4.75rem" />
          <col style="width:4rem" />
          <col style="width:4rem" />
          <col style="width:5.5rem" />
          <col style="width:10rem" />
        </colgroup>
        <thead>
          <tr>
            <th class="l">{t('riven.col.weapon')}</th>
            <th class="l" data-group data-key>{t('riven.col.verdict')}</th>
            <th class="l" data-group>{t('riven.col.stats')}</th>
            <th data-group data-drop="3">{t('riven.col.dispo')}</th>
            <th data-drop="2">{t('riven.col.rolls')}</th>
            <th data-drop="1">{t('riven.col.rank')}</th>
            <th data-group title={t('riven.col.deHelp')}>{t('riven.col.median')}</th>
            <th class="l" title={t('riven.col.deHelp')}>{t('riven.col.range')}</th>
          </tr>
        </thead>
        <tbody>
          {#each rows as row (row.key)}
            {@const r = row.riven}
            {@const change = dispoChangeFor(r.slug, market?.rivens)}
            {@const band = bandText(r)}
            {@const query = queryOf(row)}
            <tr>
              <td class="l top">
                <div class="weapon">
                  {#if r.veiled}
                    <span class="veiled" title={t('riven.veiledHelp')}>{t('riven.veiled')}</span>
                  {:else}
                    <strong>{r.weaponName ?? t('riven.unknownWeapon')}</strong>
                    {#if r.pol}<span class="pol" title={t('riven.polarity')}>{polaritySymbol(r.pol)}</span>{/if}
                  {/if}
                </div>
                <!-- Actions sit under the weapon, so they stay in view while the
                     market columns hide on a narrow window. -->
                <div class="acts">
                  <button
                    class="btn ghost xs"
                    onclick={() => showComps(row)}
                    disabled={!r.slug}
                    use:busy={compsBusy !== null}
                    aria-expanded={r.slug ? openRow === row.key : undefined}
                    title={t(r.slug ? 'riven.compsHelp' : 'riven.noComps')}
                  >
                    {t(query != null && compsBusy === query.key ? 'riven.fetching' : openRow === row.key ? 'riven.hideComps' : 'riven.comps')}
                  </button>
                  {#if band}
                    <button class="btn ghost xs" aria-expanded={openOffer === row.key} onclick={() => (openOffer = openOffer === row.key ? null : row.key)}>
                      {t(openOffer === row.key ? 'riven.hideOffer' : 'riven.offer')}
                    </button>
                  {/if}
                  {#if row.report}
                    <!-- DE's raw riven data, for reporting a trait or state the
                         app does not read yet. No account details: the path
                         and fingerprint only. -->
                    <span class="copy-data" title={t('riven.copyHelp')}>
                      <CopyButton text={row.report} label={t('riven.copyData')} name={t('riven.copyName', { weapon: r.weaponName ?? t('riven.thisRiven') })} />
                    </span>
                  {/if}
                </div>
              </td>
              <td class="l top">
                {#if row.roll?.kind === 'read'}
                  {@const roll = row.roll}
                  <div class="roll" data-testid="riven-roll">
                    <span class="tag roll-{roll.verdict}" title={t('riven.roll.help')}>{t(VERDICT[roll.verdict])}</span>
                    {#if rollReasons(roll)}<p class="why">{rollReasons(roll)}</p>{/if}
                    {#if roll.verdict === 'weak'}
                      <details class="wants"><summary>{t('riven.roll.buyersWant')}</summary><p>{wantedStats(roll)}</p></details>
                    {/if}
                    {#if roll.note}<p class="why muted">{t('riven.roll.note')} <span lang="en">{roll.note}</span></p>{/if}
                    {#if r.slug}
                      <RollRating weapon={r.slug} read={roll} stats={row.stats} mine={myVotes.find((v) => v.weapon === r.slug && v.verdict === roll.verdict)} {sharing} statName={attrName} suggestUrl={ROLL_GUIDE_ISSUE} onvote={vote} />
                    {/if}
                  </div>
                {:else if row.roll}
                  <p class="roll why muted" data-testid="riven-roll">{t(row.roll.kind === 'no_guide' ? 'riven.roll.noGuide' : 'riven.roll.unknownStat')}</p>
                {:else if r.veiled}
                  <p class="roll why muted">{t('riven.roll.veiled')}</p>
                {/if}
              </td>
              <td class="l top">
                {#if r.veiled}
                  <span class="muted why" title={t('riven.veiledStats')}>{t('riven.challenge')}</span>
                {:else}
                  <ul class="stats" title={t('riven.statsHelp')}>
                    {#each row.stats as stat, si (si)}
                      <li class="stat" class:neg={!stat.positive} class:unknown={stat.slug == null}><span class="sign">{stat.positive ? '+' : '−'}</span>{statName(stat.label)}{#if stat.spliced}<span class="spliced">{t('riven.spliced')}</span>{/if}</li>
                    {/each}
                  </ul>
                {/if}
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
              <td class="top">
                {#if r.slug && market?.rivens?.weapons?.[r.slug]}
                  <span class="num">{market.rivens.weapons[r.slug].disposition.toFixed(2)}</span>
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
              <td class="top"><span class="num">{r.veiled ? '-' : r.rerolls}</span></td>
              <td class="top"><span class="num">{r.veiled ? '-' : r.lvl}</span></td>
              <td class="top">
                {#if band}
                  <span class="num median">{band.price}</span>
                {:else}
                  <span class="muted">-</span>
                {/if}
              </td>
              <td class="l top">
                {#if band}
                  <div class="band">
                    {#if band.range}<span class="num">{band.range}</span>{/if}
                    <!-- The text carries the values; the bar only shows where
                         the median sits in the spread. -->
                    {#if band.bar}
                      <span class="range" aria-hidden="true">
                        <span class="span" style:left="{band.bar.from}%" style:width="{Math.max(band.bar.to - band.bar.from, 0.5)}%"></span>
                        <span class="mid" style:left="{band.bar.at}%"></span>
                      </span>
                    {/if}
                    <span class="muted small note">{band.note}</span>
                  </div>
                {:else}
                  <span class="muted">-</span>
                {/if}
                {#if playersFor(row)}
                  {@const players = playersFor(row)!}
                  <span class="small note players" data-testid="players-price" title={t('riven.players.help', { verdict: t(VERDICT[players.verdict as keyof typeof VERDICT]), week: players.week })}>
                    <span>{t('riven.players', { price: plat(players.median) + 'p' })}</span>
                    <span class="muted">· {t('riven.players.count', { count: players.contributors })}</span>
                  </span>
                {/if}
              </td>
            </tr>
            {#if openOffer === row.key && band}
              <!-- Still no "this riven is worth N": the offer comes from the
                   user, and we supply the arithmetic against DE's
                   distribution - placement and reroll cost. -->
              <tr class="drawer-row">
                <td colspan="8">
                  <div class="offer-check" data-testid="offer-check"><RivenOffer riven={r} market={market} /></div>
                </td>
              </tr>
            {/if}
            {#if openRow === row.key && query}
              {@const comps = compsCache.get(query.key)}
              <tr class="drawer-row">
                <td colspan="8">
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
                    {@const similarity = rivenSimilarities(r, comps.map((a) => a.attributes), attrs)}
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
                      {#each comps as a, ci (a.id)}
                        {@const pct = similarity[ci]}
                        <div class="comp">
                          <div class="comp-head">
                            <span class="mono price">{a.price}p</span>
                            <span class="muted small">
                              {t(a.is_direct_sell ? 'riven.buyout' : 'riven.auctionBuyout')}
                              {#if a.top_bid != null} · {t('riven.topBid', { price: a.top_bid })}{/if}
                            </span>
                            <span class="comp-owner" title={t('riven.wfmStatus')}>{a.owner ?? t('riven.unknown')}{#if a.owner_status}{` · ${a.owner_status}`}{/if}</span>
                            {#if pct != null}
                              <span class="similarity" title={t('riven.matchHelp')}>{t('riven.match', { pct })}</span>
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
          {#if rows.length === 0}
            <tr><td colspan="8" class="l"><span class="muted">{t('riven.filterEmpty')}</span></td></tr>
          {/if}
        </tbody>
      </table>
    </div>
    <Pager window={win} total={shown.length} label={t('riven.pages')} target={tableEl} onpage={(p) => (page = p)} />
    <p class="guide-credit" data-testid="roll-guide-credit">
      {t('riven.guideCredit')}
      <a href={ROLL_GUIDE_ISSUE} target="_blank" rel="noopener noreferrer">{t('riven.guideSuggest')}</a>
    </p>
  {/if}

</section>

<style>
  /* Shell + table come from the shared .wrap.tw / table.tw anatomy in
     app.css; only riven-specific content styles live here. Every cell starts
     at the top, so a riven's weapon, verdict, first stat and numbers share one
     line however tall its stats and reasons make the row. Caption lines are
     a whole 20px rather than the 19.5px leading gives 13px text: a row
     boundary on a half pixel loses its 1px rule in Chromium. */
  table { min-width: 62rem; }
  /* Rows here are as tall as their stats, so their edges land on any
     fraction of a pixel. The shared row rule is a 1px background tile, which
     Chromium blurs across two pixel rows when it starts on a half pixel; a
     border is snapped whole. A list of at most 100 rows does not need the
     tile's cheaper raster. */
  tbody > tr:not(:first-child) > td { border-top-color: var(--hairline); background-image: none; }
  td.top { vertical-align: top; padding-block: var(--s2); white-space: normal; line-height: 1.5rem; }
  .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
  .num { display: inline-block; line-height: 1.5rem; color: var(--fg); font-family: var(--font-mono); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .median { font-weight: 600; }
  .weapon { display: flex; align-items: baseline; flex-wrap: wrap; gap: 0 var(--s1); line-height: 1.5rem; }
  .pol { color: var(--muted); font: var(--text-caption) var(--font-ui); }
  .veiled { color: var(--muted); font-style: italic; }
  .acts { display: flex; flex-wrap: wrap; gap: var(--s1); margin-top: var(--s1); }
  .copy-data :global(.copybtn) { min-height: var(--ctl-xs); }
  /* The verdict tag leads its cell, level with the weapon name. */
  .roll .tag { margin: 0.25rem 0 0; }
  .why { margin: var(--s1) 0 0; color: var(--fg); font: var(--text-caption)/1.25rem var(--font-body); }
  .wants { margin-top: 2px; color: var(--muted); font: var(--text-caption)/1.25rem var(--font-body); }
  .wants p { margin: 2px 0 0; }
  /* One stat per line, signs in their own gutter so the names align. */
  .stats { list-style: none; margin: 0; padding: 0; line-height: 1.5rem; font-family: var(--font-body); color: var(--fg); }
  .stat { white-space: nowrap; }
  .sign { display: inline-block; width: 1ch; margin-right: var(--s1); font: 600 1em var(--font-mono); color: var(--good); }
  .stat.neg, .stat.neg .sign, .stat.unknown { color: var(--muted); }
  .spliced { margin-left: var(--s1); color: var(--muted); font: var(--text-caption)/1 var(--font-ui); }
  .splices { margin: var(--s1) 0 0; color: var(--muted); font: var(--text-caption)/1.25rem var(--font-body); }
  .splices-label { font-family: var(--font-ui); color: var(--fg); }
  .unread { margin: var(--s1) 0 0; color: var(--warn); font: var(--text-caption)/1.25rem var(--font-body); }
  .dispo-move { display: block; font-family: var(--font-mono); font-size: var(--text-caption); line-height: 1.25rem; }
  .dispo-move.up { color: var(--good); }
  .dispo-move.down { color: var(--warn); }
  .note { display: block; font-family: var(--font-body); line-height: 1.25rem; white-space: normal; }
  .band { white-space: nowrap; }
  /* Players' reported price sits under DE's band as a second, labelled source. */
  .players { display: block; margin-top: var(--s1); color: var(--fg); }
  /* Breaks only between the price and the count. */
  .players > span { white-space: nowrap; }
  /* min-max hairline and a median tick on a shared 1-10,000p log scale. */
  .range { position: relative; display: block; width: 100%; max-width: 8rem; height: 10px; margin: 2px 0; }
  .range::before { content: ''; position: absolute; left: 0; right: 0; top: 50%; border-top: 1px dotted var(--faint); }
  .range .span { position: absolute; top: 50%; height: 0; border-top: 1px solid var(--muted); }
  .range .mid { position: absolute; top: 0; bottom: 0; width: 2px; margin-left: -1px; background: var(--fg); }
  .guide-credit { margin: 0; padding: var(--s2) var(--inset); border-top: 1px var(--rule) var(--hairline); color: var(--muted); font: var(--text-caption)/var(--leading-body) var(--font-body); }
  .guide-credit a { color: var(--fg); }
  .comps-bar { display: flex; align-items: center; flex-wrap: wrap; gap: var(--s1) var(--s2); padding-top: var(--s1); }
  .offer-check { padding: var(--s1) 0; text-align: left; max-width: 48rem; }
  .small { font-size: var(--text-caption); }
  .muted { color: var(--muted); }
  .bad { color: var(--bad); }
  .mono { font-family: var(--font-mono); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .similarity { color: var(--accent); border: 1px solid var(--accent); border-radius: var(--radius-ctl); padding: 1px 5px; font: 600 var(--text-caption) var(--font-mono); white-space: nowrap; }
  /* Comps and the offer check open as an inset drawer under the row, on the panel-2 ground. */
  .drawer-row td { background-color: var(--panel-2); height: auto; text-align: left; font-family: var(--font-body); white-space: normal; }
  .comps { display: flex; flex-direction: column; gap: var(--s1); max-height: 320px; overflow: auto; padding: var(--s1) 0; }
  .comps-sample { display: flex; align-items: baseline; gap: var(--s2); flex-wrap: wrap; }
  .comp { border: 1px solid var(--border); border-radius: var(--radius-ctl); padding: var(--s2); display: flex; flex-direction: column; gap: 4px; background: var(--panel); }
  .comp-head { display: flex; align-items: baseline; gap: var(--s2); }
  .comp .price { font-weight: 700; font-size: var(--text-body); }
  .comp-owner { margin-left: auto; white-space: nowrap; font-family: var(--font-mono); }
  .comp-detail { display: flex; gap: var(--s2); flex-wrap: wrap; }
  .comp-stats { display: flex; flex-wrap: wrap; gap: 2px 12px; font-family: var(--font-body); color: var(--fg); }
  .comp-stats .stat { white-space: nowrap; }
  .riven-name { font-style: italic; }
</style>
