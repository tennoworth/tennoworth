<script lang="ts">
  import { tableNavigation } from '../ui/keyboard.svelte';
  import { onMount } from 'svelte';
  import type { PublicData } from '../public-site/model';
  import type { PublicMarket } from '../contracts/data';
  import { replacesMarket } from '../domain/market';
  import { activityCandidates, decisionAnswer, ducatParts, partTotal, publicData } from '../public-site/model';
  import { itemPath } from '../public-site/pages';
  import Links from '../public-site/Links.svelte';
  import Faq from './Faq.svelte';
  import ItemHistory from '../public-site/ItemHistory.svelte';
  import DecisionSummary from '../public-site/DecisionSummary.svelte';
  import FeatureRail from '../features/market-context/FeatureRail.svelte';
  import { HostedTransport } from '../adapters/hosted';
  import { loadPublicMarket, revalidatePublicMarket, startHostedMarketRevalidation } from '../adapters/market';
  import type { ThemeController } from '../ui/theme';
  import ThemeSwitcher from '../ui/ThemeSwitcher.svelte';
  let { heading, kind, initial, slug = '', published, theme }: { heading: string; kind: string; initial: PublicData; slug?: string; published: string[]; theme?: ThemeController } = $props();
  // The build snapshot is the hydration boundary; replace its comparison atomically.
  // svelte-ignore state_referenced_locally
  let data = $state(initial);
  let status = $state('Dated build snapshot. Enable JavaScript to check for newer prices.');
  let enhanced = $state(false);
  let query = $state('');
  let sort = $state('cost');
  const price = (n: number | null | undefined) => n == null ? 'Unavailable' : `${Number(n.toFixed(2))} p`;
  const transport = new HostedTransport();
  let candidates = $derived(activityCandidates(data));
  let item = $derived(data.items.find(i => i.slug === slug));
  let decision = $derived(decisionAnswer(data, slug));
  let rows = $derived(ducatParts(data).filter(i => i.name.toLowerCase().includes(query.toLowerCase())).sort((a,b) => sort === 'cost' ? a.price! / a.ducats! - b.price! / b.ducats! : a.name.localeCompare(b.name)).slice(0, 60));
  onMount(() => {
    if (kind === 'desktop' || kind === 'troubleshooting' || kind === 'not-found') return;
    enhanced = true;
    status = 'Checking for a newer snapshot…';
    let active = true;
    let latest: PublicMarket | null = null;
    const accept = (market: PublicMarket) => {
      if (!active) return;
      latest = market;
      if (market.platform !== initial.platform) throw new Error('Snapshot platform changed');
      if (replacesMarket(data.stamp, market.updated_at, Date.now())) data = publicData(market);
      status = 'Snapshot checked. Prices are estimates, not live offers.';
    };
    void loadPublicMarket().then(accept).catch(() => { if (active) status = 'Refresh unavailable. The dated build snapshot remains visible.'; });
    const stop = startHostedMarketRevalidation(async () => {
      try {
        if (!latest) { accept(await loadPublicMarket()); return; }
        const next = await revalidatePublicMarket(latest);
        if (next) accept(next);
      } catch {
        if (active) status = 'Refresh unavailable. The displayed snapshot remains visible with its original date.';
      }
    });
    return () => { active = false; stop(); };
  });
</script>
<a class="skip-navigation" href="#public-content">Skip to content</a>
<main data-shell class="landing public-page" id="public-content" tabindex="-1">
  <header class="ui-toolbar public-brand"><a href="/">TennoWorth</a><span>What’s worth selling in Warframe</span></header>
  {#if !decision}<Links />{/if}
  <section class="ui-panel public-prose">
    <h1>{heading}</h1>
    {#if decision}<DecisionSummary answer={decision} buildStamp={initial.stamp} {enhanced} />{/if}
    {#if kind === 'not-found'}
      <p>There is no page at this address. The link may be mistyped, or the page may have moved.</p>
      <form class="ui-toolbar public-controls" action="/" method="get" role="search">
        <label>Search the market <input class="ui-input" type="search" name="q" autocomplete="off" /></label>
        <button class="btn primary lg" type="submit">Search</button>
      </form>
      <p>Or choose a selling guide above, or go to the <a href="/">homepage</a>.</p>
    {:else if kind === 'desktop'}
      <p>Find the tradeable items you own that are worth a closer look. TennoWorth ranks your inventory, compares sets and parts, and helps you review your next sale.</p>
      <p>Free and open source for Windows and Linux. No Overwolf. No TennoWorth account. Your inventory stays on your machine.</p>
      <h2>From inventory to selling decision</h2>
      <ol><li>Launch Warframe and log in, then open TennoWorth.</li><li>Scan inventory. Review your ranked sell list and keep the items you still need.</li><li>Compare sets, parts and ducat value. Check market activity before choosing a price.</li><li>Optionally sign in to warframe.market in the app to review and manage listings.</li></ol>
      <h2>See the selling workflow</h2>
      <p>Illustrative sample data: these miniatures show the desktop workflow, not your inventory or current market prices. Select a view to explore; your actual results depend on the inventory you scan.</p>
      <FeatureRail />
      <h2>Windows</h2>
      <p>Download the .exe installer from the latest release. The installer is unsigned, so Windows SmartScreen may warn. Check the release checksums and trust information before running it.</p>
      <p><a class="btn primary" href="https://github.com/tennoworth/tennoworth/releases/latest">Download for Windows</a></p>
      <h2>Linux</h2>
      <p>Download the AppImage, make it executable, and launch it. Reading the running game can require an additional ptrace permission step; follow the Linux setup instructions for your system. Linux support alone does not establish Steam Deck game-mode support.</p>
      <p><a class="btn primary" href="https://github.com/tennoworth/tennoworth/releases/latest/download/TennoWorth-x86_64.AppImage">Download Linux AppImage</a> <a href="https://github.com/tennoworth/tennoworth#linux">Linux setup and permissions</a></p>
      <h2>What stays local?</h2>
      <p>The desktop app reads the running game’s memory to obtain short-lived credentials, then requests inventory from Digital Extremes. It does not write game memory or inject gameplay input. Optional warframe.market operations make network requests. Usage and price sharing are optional.</p>
      <p>No third-party tool can promise freedom from bans. Read the <a href="https://github.com/tennoworth/tennoworth/blob/main/SECURITY.md">security and privacy explanation</a> before scanning. This website has no inventory uploads or visitor analytics.</p>
      <p><a href="/#desktop">See the interactive product demonstration</a> · <a href="/">Try public market search first</a></p>
    {:else if kind !== 'troubleshooting'}
      {#if kind === 'decisions'}
        <p>Start with spare tradeable items you can afford to let go, then compare the price with reported activity. Check whether parts complete a set before listing them separately, and protect anything you still need for crafting.</p>
        <p>This public shortlist shows up to eight priced Prime listings with the most reported volume in the snapshot’s 48-hour window. It is ordered by volume, not your inventory or a prediction of sale speed. A valuable item with little activity may take longer to find a buyer.</p>
      {:else if kind === 'sets'}
        <p>A full Prime set can have a different market price from the sum of its parts. Compare both on the same price basis, then consider how many separate sales you would need and whether buyers are active.</p>
        <p>These examples compare latest daily medians for sets and the required component quantities. A positive gap is a nominal set premium, not guaranteed profit. Medians from separate markets may represent different trading times.</p>
      {:else if kind === 'ducats'}
        <p>Turning a Prime part into ducats gives up its potential platinum value. Compare platinum per 100 ducats: a lower number means less estimated platinum sacrificed for the same ducats.</p>
        <p>Keep crafting copies and anything you still want. This public comparison cannot know your inventory or goals. There is no universal “safe to scrap” threshold.</p>
      {:else}
        <p>Compare {heading}’s dated market median with its longer-term baseline before choosing a listing price. Check current offers and trading activity: a historical median does not guarantee a buyer.</p>
      {/if}
      {#if !decision}<p class="ui-notice">Snapshot <time datetime={data.stamp}>{data.stamp.replace('T', ' ').replace('Z', ' UTC')}</time> · Platform: {data.platform}. Source: warframe.market. Latest daily medians, not live orders. Data may be stale.</p>{/if}
      <p role="status">{status}</p>
      {#if kind === 'decisions'}
        <h2>Prime listings with observed activity</h2>
        <div class="public-table" use:tableNavigation={'Active Prime listings'}><table class="tw"><thead><tr><th>Item</th><th>Daily median</th><th>Reported volume (48h)</th><th>Next comparison</th></tr></thead><tbody>
          {#each candidates as row}<tr><td>{@render itemLink(row.slug)}</td><td>{price(row.price)}</td><td>{row.volume}</td><td><a href="/guides/sell-prime-sets-or-parts/">Sets or parts?</a></td></tr>{:else}<tr><td colspan="4">No priced Prime listings have known positive activity in this snapshot.</td></tr>{/each}
        </tbody></table></div>
        <h2>Choose your next selling decision</h2>
        <ol><li><strong>Keep what you need.</strong> This site does not know your inventory, crafting plans or available trades. Start by separating spare copies.</li><li><strong>Compare complete sets.</strong> A nominal premium must account for all required parts. <a href="/guides/sell-prime-sets-or-parts/">Work through a set comparison</a>.</li><li><strong>Price the ducat alternative.</strong> Compare the platinum value you would give up by converting an eligible part. <a href="/tools/ducats-vs-platinum/">Review individual Prime parts</a>.</li><li><strong>Check current offers.</strong> Daily medians describe reported trades; current bids and asks may differ. Choose between waiting for your asking price and accepting an available buyer’s offer.</li><li><strong>Review your selling session.</strong> <a href="/guides/warframe-market-items-not-selling/">Check visibility, presence and pricing</a> when listings are quiet.</li></ol>
        <p>Volume is reported item activity, not a count of distinct buyers or a guarantee of a quick sale. Set and component markets overlap, so do not add their volumes together. Items without a known positive unit price or activity are absent from this shortlist.</p>
      {/if}
      {#if kind === 'sets' || kind === 'item' && data.sets.some(s => s.slug === slug || s.parts.some(p => p.slug === slug))}
        {#each data.sets.filter(s => kind === 'sets' || s.slug === slug || s.parts.some(p => p.slug === slug)) as set}
          <h2 id={decision ? 'set-vs-parts' : undefined}>{set.name}: set versus components</h2>
          <div class="public-table" use:tableNavigation={`${set.name} price comparison`}><table class="tw"><thead><tr><th>Item</th><th>Quantity</th><th>Daily median per unit</th>{#if decision}<th>Orders</th>{/if}</tr></thead><tbody>
            <tr><td>{@render itemLink(set.slug)}</td><td>1 set</td><td>{price(data.items.find(i => i.slug === set.slug)?.price)}</td>{#if decision}<td><a href={`https://warframe.market/items/${set.slug}`} target="_blank" rel="noopener noreferrer">Check set orders</a></td>{/if}</tr>
            {#each set.parts as part}<tr><td>{@render itemLink(part.slug)}</td><td>{part.quantity ?? 'Unknown'}</td><td>{price(data.items.find(i => i.slug === part.slug)?.price)}</td>{#if decision}<td><a href={`https://warframe.market/items/${part.slug}`} target="_blank" rel="noopener noreferrer">Check part orders</a></td>{/if}</tr>{/each}
          </tbody></table></div>
          {@const total = partTotal(set, data)}
          {@const setPrice = data.items.find(i => i.slug === set.slug)?.price}
          <p>Required parts total: <strong>{price(total)}</strong>. Set minus parts: <strong>{price(total == null || setPrice == null ? null : setPrice - total)}</strong>.</p>
        {/each}
        <h2>Choose the sale, not just the larger number</h2>
        <p>Separate listings take time and may consume more daily trades. A set premium can disappear before every component sells. Check current orders and activity, protect your crafting copies, and compare only the same platform and item variant.</p>
        <p>A tradeable set is a collection of its required components. Do not craft components expecting the finished equipment to remain tradeable; check each item’s trading requirements in game. <a href="https://forums.warframe.com/announcement/44-trading-guide-faq-tips-guidelines/">Read Digital Extremes’ trading guide</a>.</p>
      {/if}
      {#if kind === 'ducats'}
        <div class="ui-toolbar public-controls"><label>Find a Prime part <input class="ui-input" type="search" disabled={!enhanced} bind:value={query} /></label><label>Order by <select class="ui-input" disabled={!enhanced} bind:value={sort}><option value="cost">Lowest platinum per 100 ducats</option><option value="name">Item name</option></select></label></div>
        <p>Showing up to 60 priced parts. Platinum per 100 ducats = daily median ÷ ducats × 100. Items without a positive price or ducat value are excluded.</p>
        <div class="public-table" use:tableNavigation={"Ducat opportunity costs"}><table class="tw"><thead><tr><th>Prime part</th><th>Daily median</th><th>Ducats</th><th>Platinum per 100 ducats</th></tr></thead><tbody>{#each rows as row}<tr><td>{@render itemLink(row.slug)}</td><td>{price(row.price)}</td><td>{row.ducats}</td><td>{price(row.price! / row.ducats! * 100)}</td></tr>{:else}<tr><td colspan="4">No priced parts match this search.</td></tr>{/each}</tbody></table></div>
      {/if}
      {#if kind === 'item'}
        <h2>How to read this comparison</h2>
        <p>The latest daily median reflects the latest reported day in the price snapshot. The 90-day baseline summarizes daily medians across a longer window; comparing the two gives price context, not a forecast. Reported 48-hour volume helps distinguish a price observation from a market with very little activity.</p>
        {#if item?.price != null && item.baseline != null}
          <p>The latest median of {price(item.price)} is {item.price > item.baseline ? 'above' : item.price < item.baseline ? 'below' : 'equal to'} the 90-day baseline of {price(item.baseline)}. Check current orders before deciding whether that difference is worth acting on.</p>
        {:else}<p>There is not enough comparable price evidence to assess the latest median against the baseline.</p>{/if}
        <h2>Price context</h2>
        <dl><dt>Latest daily median</dt><dd>{price(item?.price)}</dd><dt>90-day median of daily medians</dt><dd>{price(item?.baseline)}</dd><dt>Snapshot volume (48 hours)</dt><dd>{item?.volume ?? 'Unavailable'}</dd><dt>Individual-part ducats</dt><dd>{item?.ducatPart ? item.ducats : 'Unavailable'}</dd></dl>
        <h2>Recent daily median observations</h2>
        <p>Available observations in source order, oldest first. The snapshot does not provide an exact date for each entry; these are not a continuous dated history.</p>
        <ol>{#each item?.recent ?? [] as value}<li>{price(value)}</li>{:else}<li>Recent observations unavailable.</li>{/each}</ol>
        <ItemHistory {slug} load={() => transport.loadHistory()} />
        <p><a href="https://warframe.market/items/{slug}" rel="noopener noreferrer">Check current {heading} orders on warframe.market</a></p>
        <p><a href="/guides/sell-prime-sets-or-parts/">How to compare sets and parts</a> · <a href="/tools/ducats-vs-platinum/">Compare ducat opportunity cost</a></p>
      {/if}
      <h2>Make it personal</h2>
      <p>This page compares public prices. <a href="/desktop/">TennoWorth for Windows and Linux</a> adds your locally scanned inventory so you can review which of these items you actually own.</p>
    {/if}
    {#if kind === 'troubleshooting'}
      <div class="ui-stack selling-article">
        <p>Before lowering your price, check whether buyers can find the listing, whether you can answer them, and whether the price matches the exact item. A quiet listing does not by itself mean the item is worthless.</p>
        <h2>1. Be available when your status says you are</h2>
        <p>Check your own warframe.market status. Use Online in game when you are in the game and ready to respond. A status that does not match your availability can lead buyers to overlook you or messages to go unanswered.</p>
        <p>In TennoWorth, your WFM status is in the desktop status strip and Settings. Following the game is optional; review the setting before relying on it. Pause selling when you cannot handle more messages.</p>
        <h2>2. Check that the listing is visible</h2>
        <p>A hidden listing is not an active public offer. Open My orders in the desktop app or your orders on warframe.market and check its visibility. Make only the listings you can fulfil available; you do not need to offer everything you own at once.</p>
        <h2>3. Compare the same item and variant</h2>
        <p>Check the exact component or set, mod or arcane rank, quantity and platform context. An unranked mod and a maxed mod are different offers. A full set and one component are different products. Do not copy a price from the wrong variant.</p>
        <h2>4. Separate asking prices from completed trades</h2>
        <p>A sell order is an asking price; a buy order is what a buyer offers. A historical median summarizes reported trades and is not a live offer. Compare current orders from available players before choosing your price. The single lowest listing can be unrepresentative.</p>
        <p>In TennoWorth, open My orders and choose Check live. Review the current ask and bid for the exact tier, then choose whether to change your listing. A check does not automatically reprice it. Matching another ask does not guarantee that your item sells next.</p>
        <h2>5. Check activity, not just price</h2>
        <p>An expensive item with little reported activity may take patience. Reported volume is context, not a prediction of your sale time. If your play window is short, compare an available buyer’s bid with your asking price and decide whether the difference is worth waiting for.</p>
        <p><a href="/what-to-sell/">Compare public Prime prices and activity</a> or <a href="/guides/sell-prime-sets-or-parts/">check whether a complete set changes the decision</a>.</p>
        <h2>6. Keep offers you can actually fulfil</h2>
        <p>Remove sold listings, check quantities against your latest inventory and keep crafting copies. If messages arrive faster than you can answer, hide some listings and work through a smaller batch. Check the actual item and total in Warframe before confirming the trade.</p>
        <p><a href="https://forums.warframe.com/announcement/44-trading-guide-faq-tips-guidelines/">Digital Extremes’ trading guide</a> explains the in-game trade. Refer to <a href="https://warframe.market/faq">warframe.market’s current help</a> for marketplace questions.</p>
        <h2>Review the problem with your own inventory</h2>
        <p><a href="/desktop/">TennoWorth for Windows and Linux</a> brings your locally scanned inventory and listing checks together. You can use the explanations and public comparisons on this site without installing anything or signing in.</p>

      </div>
    {/if}
  </section>
  {#if decision}<Links />{/if}
  {#if kind === 'desktop'}<Faq />{/if}
  <footer class="ui-toolbar public-footer"><a href="/">Market search</a><a href="https://github.com/tennoworth/tennoworth">Source code</a><span>Fan project. Not affiliated with Digital Extremes or warframe.market.</span>{#if theme}<ThemeSwitcher {theme} compact />{/if}</footer>
</main>
{#snippet itemLink(value: string)}
  {@const name = data.items.find(i => i.slug === value)?.name ?? value.replaceAll('_', ' ')}
  <a href={published.includes(value) ? itemPath(value) : `https://warframe.market/items/${value}`}>{name}</a>
{/snippet}
