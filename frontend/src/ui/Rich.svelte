<!-- A translated message with <em>, <b> or <code> in it. The parts render as
     elements and text, never as HTML, so a catalog cannot inject markup.
     Elements come from this component, so a parent styles them with :global. -->
<script lang="ts">
  import { richParts } from '../i18n/message';

  let { text }: { text: string } = $props();
</script>

{#each richParts(text) as part, i (i)}{#if part.tag === 'em'}<em>{part.text}</em>{:else if part.tag === 'b'}<b>{part.text}</b>{:else if part.tag === 'code'}<code>{part.text}</code>{:else}{part.text}{/if}{/each}
