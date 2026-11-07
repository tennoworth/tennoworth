<script lang="ts">
  // The keys that work on the view on screen, each one also a button, so the
  // legend is the way in for anyone who cannot or does not use the keys. It
  // reads the user's own bindings, so a recorded or disabled shortcut never
  // shows here as something it is not.
  import { focusSearch, keyboard, requestKeyboard } from '../../ui/keyboard.svelte';

  let { search = false }: { search?: boolean } = $props();
  const goKey = $derived(`${keyboard.modifier}+${keyboard.navigation ?? keyboard.commandKey}`);
</script>

<footer class="key-legend" aria-label="Keyboard shortcuts">
  {#if search}<button type="button" onclick={() => focusSearch()} aria-label={keyboard.enabled ? `Search, key ${keyboard.search}` : 'Search'}>{#if keyboard.enabled}<kbd>{keyboard.search}</kbd>{/if}Search</button>{/if}
  <button type="button" onclick={() => requestKeyboard('navigate')} aria-label="Go to a view, keys {goKey}"><kbd>{goKey}</kbd>Go to</button>
  <span><kbd>F6</kbd>Next region</span>
  <button type="button" onclick={() => requestKeyboard('help')} aria-label={keyboard.enabled ? `Keyboard shortcuts, key ${keyboard.help}` : 'Keyboard shortcuts'}>{#if keyboard.enabled}<kbd>{keyboard.help}</kbd>{/if}Shortcuts</button>
</footer>
