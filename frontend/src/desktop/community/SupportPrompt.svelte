<script lang="ts">
  import PromptBanner from '../prompts/PromptBanner.svelte';
  import type { PromptSession } from '../prompts/prompt-session.svelte';
  import { SUPPORT_PROMPT } from '../prompts/policies';
  import { t } from '../../i18n/i18n.svelte';
  let { session, active }: { session: PromptSession; active: boolean } = $props();
</script>

{#if active}
  <PromptBanner {session} id={SUPPORT_PROMPT.id} title={t('support.title')}>
    <p>{t('support.body')}</p>
    {#snippet actions()}
      <a class="btn primary" href="https://ko-fi.com/prowly" target="_blank" rel="noopener noreferrer" onclick={() => { session.finish(SUPPORT_PROMPT.id); session.decline(); }}>{t('support.kofi')}</a>
    {/snippet}
  </PromptBanner>
{/if}
