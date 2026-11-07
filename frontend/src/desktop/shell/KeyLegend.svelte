<script lang="ts">
  // The keys that work on the view on screen, each one also a button, so the
  // legend is the way in for anyone who cannot or does not use the keys. It
  // reads the user's own bindings, so a recorded or disabled shortcut never
  // shows here as something it is not.
  import { focusSearch, keyboard, requestKeyboard } from '../../ui/keyboard.svelte';
  import { t } from '../../i18n/i18n.svelte';

  let { search = false }: { search?: boolean } = $props();
  const goKey = $derived(`${keyboard.modifier}+${keyboard.navigation ?? keyboard.commandKey}`);
</script>

<footer class="key-legend" aria-label={t('keys.legend')}>
  {#if search}<button type="button" onclick={() => focusSearch()} aria-label={keyboard.enabled ? t('keys.searchKey', { key: keyboard.search }) : t('keys.search')}>{#if keyboard.enabled}<kbd>{keyboard.search}</kbd>{/if}{t('keys.search')}</button>{/if}
  <button type="button" onclick={() => requestKeyboard('navigate')} aria-label={t('keys.goToKeys', { keys: goKey })}><kbd>{goKey}</kbd>{t('keys.goTo')}</button>
  <span><kbd>F6</kbd>{t('keys.nextRegion')}</span>
  <button type="button" onclick={() => requestKeyboard('help')} aria-label={keyboard.enabled ? t('keys.shortcutsKey', { key: keyboard.help }) : t('keys.legend')}>{#if keyboard.enabled}<kbd>{keyboard.help}</kbd>{/if}{t('keys.shortcuts')}</button>
</footer>
