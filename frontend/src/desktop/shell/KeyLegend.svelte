<script lang="ts">
  // A quiet reminder of the keys that work on the view on screen. It reads the
  // user's own bindings, so a recorded or disabled shortcut never shows here
  // as something it is not. Decorative to assistive technology: Keyboard
  // shortcuts in the sidebar foot is the accessible reference.
  import { keyboard } from '../../ui/keyboard.svelte';

  let { search = false }: { search?: boolean } = $props();
</script>

<footer class="key-legend" aria-hidden="true">
  {#if search && keyboard.enabled}<span><kbd>{keyboard.search}</kbd>Search</span>{/if}
  <span><kbd>{keyboard.modifier}+{keyboard.navigation ?? keyboard.commandKey}</kbd>Go to</span>
  <span><kbd>F6</kbd>Next region</span>
  {#if keyboard.enabled}<span><kbd>{keyboard.help}</kbd>Shortcuts</span>{/if}
</footer>
