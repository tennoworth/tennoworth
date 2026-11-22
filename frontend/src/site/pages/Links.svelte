<script lang="ts">import { pages } from '../../features/market/pages';</script>
<nav class="ui-toolbar" aria-label="Selling guides">
  {#each pages as page}<a href={page.path}>{page.kind === 'desktop' ? 'Windows, Linux + Steam Deck app' : page.heading}</a>{/each}
</nav>
