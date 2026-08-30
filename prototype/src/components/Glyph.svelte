<script lang="ts">
  // One item-category mark. Stroke-only and `currentColor`, so it inherits the
  // row's colour in both modes without a second definition - no theme-specific
  // asset, no CSP entry, no bytes on the wire beyond this component.
  //
  // `aria-hidden` by default: the glyph repeats the row's own text, so
  // announcing it would only add noise for a screen reader. Pass a `label`
  // on the rare surface where the mark stands alone.
  import { GLYPH_PATHS, type GlyphName } from '../lib/glyphs';

  let {
    name = 'unknown' as GlyphName,
    size = 16,
    label = '',
  }: { name?: GlyphName; size?: number; label?: string } = $props();
</script>

<svg
  class="glyph"
  viewBox="0 0 24 24"
  width={size}
  height={size}
  fill="none"
  stroke="currentColor"
  stroke-width="1.5"
  stroke-linecap="square"
  stroke-linejoin="miter"
  role={label ? 'img' : 'presentation'}
  aria-hidden={label ? undefined : 'true'}
  aria-label={label || undefined}
>
  <path d={GLYPH_PATHS[name] ?? GLYPH_PATHS.unknown} />
</svg>

<style>
  .glyph {
    flex: none;
    /* Sits on the text baseline rather than the line box, so a glyph in a
       table cell lines up with the name beside it at any row height. */
    vertical-align: -0.15em;
    opacity: 0.85;
  }
</style>
