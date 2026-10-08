<script lang="ts">
  import PromptBanner from '../../ui/PromptBanner.svelte';
  import type { PromptSession } from '../../ui/prompt-session.svelte';
  import { SUPPORT_PROMPT } from '../prompt-policies';
  let { session, active }: { session: PromptSession; active: boolean } = $props();
</script>

{#if active}
  <PromptBanner {session} id={SUPPORT_PROMPT.id} title="Support TennoWorth">
    <p>TennoWorth is free and has no ads. If it has helped you sell, you can support its development on Ko-fi. Ignoring this changes nothing: every feature stays free.</p>
    {#snippet actions()}
      <a class="btn primary" href="https://ko-fi.com/prowly" target="_blank" rel="noopener noreferrer" onclick={() => { session.finish(SUPPORT_PROMPT.id); session.decline(); }}>Support on Ko-fi</a>
    {/snippet}
  </PromptBanner>
{/if}
