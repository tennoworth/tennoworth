<script lang="ts">
  // A 7px square beside the status word. Shape carries the state as well as
  // hue: filled in game, outlined online, faint outline invisible, amber when
  // the user has something to fix. Decorative; the word next to it is the meaning.
  import type { PresenceChoice } from '../../contracts/desktop';

  let { kind }: { kind: PresenceChoice | 'warn' } = $props();
</script>

<span class="mark {kind}" aria-hidden="true"></span>

<style>
  .mark { display: inline-block; flex: none; width: 7px; height: 7px; background: var(--muted); }
  .ingame { background: var(--good); }
  .online { background: transparent; box-shadow: inset 0 0 0 1.5px var(--good); }
  .invisible { background: transparent; box-shadow: inset 0 0 0 1px var(--muted); }
  .warn { background: var(--warn); }
</style>
