<!-- A translated message with <em>, <b>, <code>, <kbd>, <good> or <bad> in it. The parts render as
     elements and text, never as HTML, so a catalog cannot inject markup.
     Elements come from this component, so a parent styles them with :global. -->
<script lang="ts">
  import { richParts } from '../i18n/message';

  let { text }: { text: string } = $props();
</script>

{#each richParts(text) as part, i (i)}{#if part.tag === 'em'}<em>{part.text}</em>{:else if part.tag === 'b'}<b>{part.text}</b>{:else if part.tag === 'code'}<code>{part.text}</code>{:else if part.tag === 'kbd'}<kbd>{part.text}</kbd>{:else if part.tag === 'good' || part.tag === 'bad'}<b class="tone" data-tone={part.tag}>{part.text}</b>{:else}{part.text}{/if}{/each}

<style>
  .tone[data-tone='good'] { color: var(--good); }
  .tone[data-tone='bad'] { color: var(--bad); }
</style>
