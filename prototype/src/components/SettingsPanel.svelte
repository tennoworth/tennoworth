<script lang="ts">
  // The Settings view. Home for preferences that are not part of doing the
  // work — starting with Appearance, which is where the theme control lives
  // now that it is no longer chrome on the landing header and the sidebar.
  //
  // Structure: one `.wrap.tw` panel per section, each with a `.rail` title and
  // a `.sbody`. Adding a section = adding another panel; nothing here is
  // special-cased to Appearance.
  import ThemeSwitcher from './ThemeSwitcher.svelte';
  import type { ThemeController } from '../lib/theme';

  interface Props {
    /** The boot-time controller from src/lib/theme.ts. */
    theme: ThemeController;
  }
  let { theme }: Props = $props();
</script>

<section class="view-header">
  <h2>Settings</h2>
  <span
    class="lede-dot"
    role="img"
    aria-label="About this view"
    title="Preferences for this install. They persist on this machine and are never uploaded."
  >ⓘ</span>
</section>

<div class="settings">
  <section class="wrap tw" aria-labelledby="set-appearance">
    <div class="rail"><h3 id="set-appearance">Appearance</h3></div>
    <div class="sbody">
      <div class="field">
        <span class="k">Colour mode</span>
        <ThemeSwitcher {theme} label="Colour mode" />
      </div>
      <p class="exp">
        System follows your operating system's light/dark setting and changes
        with it; Light and Dark pin the app regardless.
      </p>
    </div>
  </section>
</div>

<style>
  /* The view header + its info dot, as SellPane has them: both components
     render the shared markup, but the rules are Svelte-scoped per component,
     so each self-contained view carries its own copy. */
  .view-header {
    display: flex;
    align-items: center;
    gap: var(--s2);
    min-height: var(--rail);
    flex-wrap: wrap;
  }
  .view-header h2 {
    font-size: 20px;
    font-weight: 600;
    text-transform: none;
    letter-spacing: -0.01em;
    color: var(--fg);
    margin: 0;
    line-height: 1.5rem;
  }
  .lede-dot {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    font-size: 11px;
    line-height: 1;
    color: var(--muted);
    border: 1px var(--rule) var(--hairline);
    cursor: help;
  }
  .lede-dot:hover, .lede-dot:focus-visible { color: var(--accent); border-color: var(--accent); }

  .settings { display: flex; flex-direction: column; gap: var(--stack); max-width: 44rem; margin-top: var(--stack); }
  .sbody {
    display: flex;
    flex-direction: column;
    gap: var(--s2);
    padding: var(--s3) var(--inset) var(--s4);
  }
  .field { display: flex; align-items: center; gap: var(--s2) var(--s4); flex-wrap: wrap; }
  .field .k {
    width: 7rem;
    flex: 0 0 auto;
    font-family: var(--font-ui);
    font-size: 10px;
    line-height: 1rem;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    font-weight: 600;
    color: var(--muted);
  }
  /* Helper copy is real information, so --muted (the readable floor), never
     --faint, which is decorative-only. */
  .sbody .exp { margin: 0; font-size: 12px; line-height: 1rem; color: var(--muted); max-width: 60ch; white-space: normal; }
</style>
