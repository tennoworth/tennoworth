<script lang="ts">
  // Decorative: the item name always sits beside it, so it carries no alt text.
  // A missing or failed picture keeps the slot, so names stay aligned.
  // Not loading="lazy": WebKitGTK never starts a lazy image inside the app's
  // scrolling main pane, so every row past the first screen stayed blank.
  // Lists page at 100 rows at most.
  import { itemArtUrl } from './item-art';

  let { slug }: { slug: string } = $props();
  let src = $state<string | null>(null);
  let failed = $state(false);

  $effect(() => {
    const current = slug;
    let live = true;
    failed = false;
    void itemArtUrl(current).then((url) => {
      if (live) src = url;
    });
    return () => {
      live = false;
    };
  });
</script>

<span class="item-art" aria-hidden="true">
  {#if src && !failed}<img {src} alt="" decoding="async" onerror={() => (failed = true)} />{/if}
</span>
