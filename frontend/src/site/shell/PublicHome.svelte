<script lang="ts">
  import Introduction from '../pages/Introduction.svelte';
  import Links from '../pages/Links.svelte';
  import Faq from '../../features/faq/Faq.svelte';
</script>
<main data-shell class="landing public-page">
  <h1>TennoWorth</h1><Introduction />
  <p>Browse public Warframe market prices, movers and vaulted items. Enable JavaScript to search the market; the selling guides below work without it.</p>
  <Links />
  <section class="ui-panel public-prose" id="desktop">
    <h2>Your inventory, ranked locally</h2>
    <p>The free Windows and Linux desktop app compares your owned items, sets, parts and ducat value. No Overwolf. Your inventory stays on your machine.</p>
    <a href="/desktop/">Explore the desktop app and downloads</a>
  </section>
  <Faq />
</main>
