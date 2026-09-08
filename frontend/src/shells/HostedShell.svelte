<script lang="ts">
import { onMount } from 'svelte';
import MarketBrowser from '../features/market-context/MarketBrowser.svelte';
import DesktopShowcase from '../features/market-context/DesktopShowcase.svelte';
import FeatureRail from '../features/market-context/FeatureRail.svelte';
import ThemeSwitcher from '../ui/ThemeSwitcher.svelte';
import Faq from './Faq.svelte';
import { loadMarket } from '../adapters/market';
import { HostedTransport } from '../adapters/hosted';
import { baroLocation, humanWindow } from '../ui/format';
import type { ThemeController } from '../ui/theme';
import type { Market } from '../contracts/data';
let { theme }: { theme: ThemeController } = $props();
const APP_COMMIT = __APP_COMMIT__;
const transport = new HostedTransport();
let market = $state<Market | null>(null);
onMount(() => { let active = true; void loadMarket().then(value => { if(active) market = value; }).catch(console.error); return () => { active = false; }; });
let snapshotStamp = $derived.by(() => {
    const t = Date.parse(market?.updated_at ?? '');
    if (!Number.isFinite(t)) return '';
    return new Date(t).toISOString().slice(0, 16).replace('T', ' ') + ' UTC';
  });
let voidTrader = $derived.by(() => {
    const b = market?.baro;
    if (!b) return null;
    return { ...b, location: baroLocation(b.location) };
  });
let baroState = $derived.by(() => {
    if (!voidTrader) return null;
    const now = Date.now();
    const arr = Date.parse(voidTrader.activation);
    const exp = Date.parse(voidTrader.expiry);
    if (Number.isFinite(exp) && now < exp && Number.isFinite(arr) && now >= arr) {
      // Baro is currently visiting.
      const leavesIn = exp - now;
      return { phase: 'here', label: 'Baro is here', windowMs: leavesIn };
    }
    if (Number.isFinite(arr) && now < arr) {
      return { phase: 'incoming', label: 'Baro arrives in', windowMs: arr - now };
    }
    return { phase: 'unknown', label: 'Next Baro visit', windowMs: null };
  });
let marketStaleness = $derived(ago(market?.updated_at));
let marketFreshness = $derived.by<'unknown' | 'fresh' | 'aging' | 'stale'>(() => {
    if (!market?.updated_at) return 'unknown';
    const h = (Date.now() - new Date(market.updated_at).getTime()) / 3.6e6;
    if (h <= 3) return 'fresh';
    if (h <= 24) return 'aging';
    return 'stale';
  });
function ago(ts: string | number | null | undefined) {
    if (!ts) return null;
    // Clamp at 0 - a cron runner with skewed clock can produce
    // `updated_at` in the future, which used to render "-120 min ago".
    const minutes = Math.max(0, Math.round((Date.now() - new Date(ts).getTime()) / 60000));
    if (minutes < 1) return 'just now';
    if (minutes < 60) return `${minutes} min ago`;
    if (minutes < 60 * 24) return `${Math.round(minutes / 60)} h ago`;
    return `${Math.round(minutes / 1440)} d ago`;
  }
</script>

<main data-shell class="landing" >
  {@render statusStrip()}
  <!-- One lede line under the strip (the strip's descriptor already says what
       this is). The old pitch paragraph / hero is gone - search is the first
       control. The theme switcher used to ride this line's right end; it now
       lives in Settings → Appearance, with a quiet copy in the site footer for
       visitors who never search their way into the shell. -->
  <header data-shell class="landing-head">
    <p data-shell class="lede">
      
        What's worth selling in Warframe right now - search any item, spot the movers, see what's vaulted. No install. No login.
      
    </p>
  </header>

  

  
    

    {#if market}
      
        <!-- Hosted: the browser hands the visitor off to the desktop app with
             the same rows completed (DesktopShowcase renders inside the
             browser's flow so it can use the browser's sample rows). -->
        <MarketBrowser market={market} staleness={marketStaleness} freshness={marketFreshness} loadHistory={() => transport.loadHistory()} handoff={handoffPanel} />
      
    {:else}
      <DesktopShowcase />
    {/if}
  

  

  <!-- The hosted landing reads as a price-lookup tool: search, movers,
       vaulted, hand-off. Nothing above this reveals that the app also does
       set picks, relics, rivens, watches, the ledger, orders and the
       advisor. The rail is that reveal - one miniature per surface, built
       from sample data rather than screenshots so it re-skins with the
       theme and can never go stale. Hosted only: a desktop visitor has the
       real thing in the sidebar. -->
  
    <FeatureRail />
  

  <Faq />

  <footer data-shell class="sitefoot">
    <span data-shell class="grow">TennoWorth is a fan project, not affiliated with Digital Extremes or warframe.market. Open source · MIT · data from warframe.market and warframestat.us.</span>
    {#if market?.updated_at}<span data-shell title="When the market snapshot was taken">Snapshot {snapshotStamp}</span>{/if}
    <a data-shell href="#trust">Trust &amp; safety</a>
    <span data-shell class="ver" title="build {APP_COMMIT}">{APP_COMMIT}</span>
    <!-- The theme control's home is Settings → Appearance, inside the shell.
         A visitor who never searches never reaches the shell, so the mode
         control also sits here - quiet, right-aligned, on the footer's own
         type scale - rather than leaving the hosted site unable to override
         the OS scheme. -->
    <div data-shell class="foot-theme"><ThemeSwitcher {theme} compact label="Colour mode" /></div>
  </footer>
</main>

{#snippet statusStrip()}
  <!-- Shell-level status strip: one 40px spine on the landing AND the
       workspace. In the shell its brand cell sits exactly over the sidebar
       column; the rest answers "is what I'm looking at still true?" -
       inventory age, market age, orders to fix, Baro, WFM session. Rare
       inventory actions (Export / Restore / Clear) live one click deeper in
       the Refresh menu. -->
  <header data-shell class="statusbar"  >
    <div data-shell class="brand">
      <h1 data-shell>TennoWorth</h1>
      <span data-shell class="sub">warframe.market prices, ranked by what actually sells</span>
    </div>
    
    <div data-shell class="cell">
      <span data-shell class="dot {marketFreshness}" role="img" aria-label="Market data {marketFreshness}"></span>
      <span data-shell>Market</span>
      <b data-shell>{marketStaleness ?? '-'}</b>
      {#if marketFreshness !== 'unknown'}<span data-shell>· {marketFreshness}</span>{/if}
    </div>
    
    {#if baroState && baroState.phase !== 'unknown'}
      <div data-shell class="cell baro">
        <span data-shell class="ducat" aria-hidden="true">⌬</span>
        <span data-shell>{baroState.phase === 'here' ? 'Baro leaves in' : 'Baro arrives in'}</span>
        <b data-shell>{humanWindow(baroState.windowMs)}</b>
      </div>
    {/if}
    <span data-shell class="grow"></span>
    
      <nav data-shell class="cell end site-links" aria-label="Site">
        <a data-shell href="#faq">FAQ</a>
        <a data-shell href="#desktop">Desktop app ↓</a>
        {@render projectLinkAnchors()}
      </nav>
    
  </header>
{/snippet}
{#snippet projectLinkAnchors()}
  <a data-shell class="project-link" href="https://github.com/tennoworth/tennoworth" target="_blank" rel="noopener noreferrer">
    <svg data-shell class="github-mark" viewBox="0 0 24 24" aria-hidden="true">
      <path data-shell d="M12 2.7a9.5 9.5 0 0 0-3 18.5c.5.1.7-.2.7-.5v-1.9c-2.8.6-3.4-1.2-3.4-1.2-.5-1.2-1.1-1.5-1.1-1.5-.9-.6.1-.6.1-.6 1 0 1.6 1 1.6 1 .9 1.6 2.4 1.1 3 .8.1-.7.4-1.1.7-1.3-2.2-.3-4.6-1.1-4.6-4.7 0-1 .4-1.9 1-2.6-.1-.3-.4-1.3.1-2.6 0 0 .8-.3 2.7 1a9.2 9.2 0 0 1 4.9 0c1.9-1.3 2.7-1 2.7-1 .5 1.3.2 2.3.1 2.6.6.7 1 1.6 1 2.6 0 3.7-2.4 4.5-4.6 4.7.4.3.7.9.7 1.8v2.8c0 .4.2.6.7.5A9.5 9.5 0 0 0 12 2.7Z" />
    </svg>
    <span data-shell>GitHub</span>
  </a>
  <a data-shell class="project-link" href="https://ko-fi.com/prowly" target="_blank" rel="noopener noreferrer">
    <svg data-shell viewBox="0 0 24 24" aria-hidden="true">
      <path data-shell d="M4 7 H17 V16 H6 L4 14 Z M17 9 H19 L21 11 V13 L19 15 H17 M7 10 L10 13 L14 9" />
    </svg>
    <span data-shell>Buy me a coffee</span>
  </a>
{/snippet}
{#snippet handoffPanel(rows: import('svelte').ComponentProps<typeof DesktopShowcase>['rows'])}
  <DesktopShowcase {rows} />
{/snippet}