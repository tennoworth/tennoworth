<script lang="ts">
  import { tableNavigation } from '../ui/keyboard.svelte';
  import { onMount } from 'svelte';
  import type { PublicData } from '../public-site/model';
  import { replacesMarket } from '../domain/market';
  import { partTotal, publicData } from '../public-site/model';
  import { itemPath } from '../public-site/pages';
  import Links from '../public-site/Links.svelte';
  import Faq from './Faq.svelte';
  import { loadMarket, revalidateMarket, startHostedMarketRevalidation } from '../adapters/market';
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
  let item = $derived(data.items.find(i => i.slug === slug));
  let rows = $derived(data.items.filter(i => i.ducats && i.price && i.name.toLowerCase().includes(query.toLowerCase())).sort((a,b) => sort === 'cost' ? a.price! / a.ducats! - b.price! / b.ducats! : a.name.localeCompare(b.name)).slice(0, 60));
  onMount(() => {
    if (kind === 'desktop') return;
    enhanced = true;
    status = 'Checking for a newer snapshot…';
    let active = true;
    let latest: Awaited<ReturnType<typeof loadMarket>> | null = null;
    const accept = (market: Awaited<ReturnType<typeof loadMarket>>) => {
      if (!active) return;
      latest = market;
      if (market.platform !== initial.platform) throw new Error('Snapshot platform changed');
      if (replacesMarket(data.stamp, market.updated_at, Date.now()) || market.updated_at === data.stamp) data = publicData(market);
      status = 'Snapshot checked. Prices are estimates, not live offers.';
    };
    void loadMarket().then(accept).catch(() => { if (active) status = 'Refresh unavailable. The dated build snapshot remains visible.'; });
    const stop = startHostedMarketRevalidation(async () => {
      try {
        if (!latest) { accept(await loadMarket()); return; }
        const next = await revalidateMarket(latest);
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
  <Links />
  <section class="ui-panel public-prose">
    <h1>{heading}</h1>
    {#if kind === 'desktop'}
      <p>Find the tradeable items you own that are worth a closer look. TennoWorth ranks your inventory, compares sets and parts, and helps you review your next sale.</p>
      <p>Free and open source for Windows and Linux. No Overwolf. No TennoWorth account. Your inventory stays on your machine.</p>
      <h2>From inventory to selling decision</h2>
      <ol><li>Launch Warframe and log in, then open TennoWorth.</li><li>Scan inventory. Review your ranked sell list and keep the items you still need.</li><li>Compare sets, parts and ducat value. Check market activity before choosing a price.</li><li>Optionally sign in to warframe.market in the app to review and manage listings.</li></ol>
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
    {:else}
      {#if kind === 'sets'}
        <p>A full Prime set can have a different market price from the sum of its parts. Compare both on the same price basis, then consider how many separate sales you would need and whether buyers are active.</p>
        <p>These examples compare latest daily medians for sets and the required component quantities. A positive gap is a nominal set premium, not guaranteed profit. Medians from separate markets may represent different trading times.</p>
      {:else if kind === 'ducats'}
        <p>Turning a Prime part into ducats gives up its potential platinum value. Compare platinum per 100 ducats: a lower number means less estimated platinum sacrificed for the same ducats.</p>
        <p>Keep crafting copies and anything you still want. This public comparison cannot know your inventory or goals. There is no universal “safe to scrap” threshold.</p>
      {:else}
        <p>Compare {heading}’s dated market median with its longer-term baseline before choosing a listing price. Check current offers and trading activity: a historical median does not guarantee a buyer.</p>
      {/if}
      <p class="ui-notice">Snapshot <time datetime={data.stamp}>{data.stamp.replace('T', ' ').replace('Z', ' UTC')}</time> · Platform: {data.platform}. Source: warframe.market. Latest daily medians, not live orders. Data may be stale.</p>
      <p role="status">{status}</p>
      {#if kind === 'sets' || kind === 'item' && data.sets.some(s => s.slug === slug || s.parts.some(p => p.slug === slug))}
        {#each data.sets.filter(s => kind === 'sets' || s.slug === slug || s.parts.some(p => p.slug === slug)) as set}
          <h2>{set.name}: set versus components</h2>
          <div class="public-table" use:tableNavigation={`${set.name} price comparison`}><table class="tw"><thead><tr><th>Item</th><th>Quantity</th><th>Daily median per unit</th></tr></thead><tbody>
            <tr><td>{@render itemLink(set.slug)}</td><td>1 set</td><td>{price(data.items.find(i => i.slug === set.slug)?.price)}</td></tr>
            {#each set.parts as part}<tr><td>{@render itemLink(part.slug)}</td><td>{part.quantity ?? 'Unknown'}</td><td>{price(data.items.find(i => i.slug === part.slug)?.price)}</td></tr>{/each}
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
        <h2>Price context</h2>
        <dl><dt>Latest daily median</dt><dd>{price(item?.price)}</dd><dt>90-day median of daily medians</dt><dd>{price(item?.baseline)}</dd><dt>Snapshot volume (48 hours)</dt><dd>{item?.volume ?? 'Unavailable'}</dd><dt>Ducat value</dt><dd>{item?.ducats ?? 'Unavailable'}</dd></dl>
        <h2>Recent daily median observations</h2>
        <p>Available observations in source order, oldest first. The snapshot does not provide an exact date for each entry; these are not a continuous dated history.</p>
        <ol>{#each item?.recent ?? [] as value}<li>{price(value)}</li>{:else}<li>Recent observations unavailable.</li>{/each}</ol>
        <p><a href="https://warframe.market/items/{slug}" rel="noopener noreferrer">Check current {heading} orders on warframe.market</a></p>
        <p><a href="/guides/sell-prime-sets-or-parts/">How to compare sets and parts</a> · <a href="/tools/ducats-vs-platinum/">Compare ducat opportunity cost</a></p>
      {/if}
      <h2>Make it personal</h2>
      <p>This page compares public prices. <a href="/desktop/">TennoWorth for Windows and Linux</a> adds your locally scanned inventory so you can review which of these items you actually own.</p>
    {/if}
  </section>
  {#if kind === 'desktop'}<Faq />{/if}
  <footer class="ui-toolbar public-footer"><a href="/">Market search</a><a href="https://github.com/tennoworth/tennoworth">Source code</a><span>Fan project. Not affiliated with Digital Extremes or warframe.market.</span>{#if theme}<ThemeSwitcher {theme} compact />{/if}</footer>
</main>
{#snippet itemLink(value: string)}
  {@const name = data.items.find(i => i.slug === value)?.name ?? value.replaceAll('_', ' ')}
  <a href={published.includes(value) ? itemPath(value) : `https://warframe.market/items/${value}`}>{name}</a>
{/snippet}
