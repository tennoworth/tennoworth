<script lang="ts">
  // The first launch asks which language TennoWorth should speak, once. The
  // app behind switches as the cursor moves, so the choice is seen, not
  // imagined. The motion is a documented exception (design-system.md,
  // Language picker); reduced motion shows the same dialog without it.
  import { onMount, tick } from 'svelte';
  import { i18n, t } from '../../i18n/i18n.svelte';
  import { LANGUAGES, type LanguageCode } from '../../i18n/languages';
  import { decodeChange, decodePage, scramble } from '../../ui/decode';
  import Rich from '../../ui/Rich.svelte';

  type Choice = (typeof LANGUAGES)[number];
  let { gameLanguage, save, ondone }: {
    /** The game's language, if the log names one; the cursor starts there. */
    gameLanguage: string | null;
    /** Stores the choice. Rejects when the app language could not be saved. */
    save: (code: LanguageCode) => Promise<void>;
    ondone: () => void;
  } = $props();

  const CHOICES = LANGUAGES.filter((language) => language.offered);
  const reduced = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
  const ms = (n: number) => (reduced ? 0 : n);

  let dialog = $state<HTMLDialogElement>();
  let grid = $state<HTMLElement>();
  let selected = $state<Choice>(CHOICES.find((language) => language.code === i18n.language) ?? CHOICES[0]);
  let phase = $state<'entering' | 'open' | 'closing'>('entering');
  let failed = $state(false);
  let saving = false;
  let mark = $state({ x: 0, y: 0, w: 0, h: 0, ready: false });
  // Until the player acts, the title answers "Language" in every language.
  let cycling = $state(!reduced);
  let railIndex = $state(0);
  let railWord = $derived(cycling ? CHOICES[railIndex].word : selected.word);
  // One decode per change of language: 0 → 1 over a few hundred ms.
  let progress = $state(1);
  let frame = $state(0);

  function decodeText() {
    if (reduced) return;
    const start = performance.now();
    const step = (now: number) => {
      progress = Math.min(1, (now - start) / 320);
      frame = Math.floor(now / 40);
      if (progress < 1) requestAnimationFrame(step);
    };
    progress = 0;
    requestAnimationFrame(step);
  }

  function placeMark() {
    const row = grid?.querySelector<HTMLElement>(`[data-code="${selected.code}"]`);
    if (row) mark = { x: row.offsetLeft, y: row.offsetTop, w: row.offsetWidth, h: row.offsetHeight, ready: true };
  }

  async function choose(language: Choice, focus = false) {
    cycling = false;
    if (language.code === selected.code) return;
    selected = language;
    await tick();
    placeMark();
    if (focus) grid?.querySelector<HTMLElement>(`[data-code="${language.code}"]`)?.focus();
    try {
      await decodeChange(document.querySelector('#app'), ms(420), async () => {
        await i18n.use(language.code);
        document.documentElement.lang = i18n.tag;
      });
      decodeText();
    } catch (error) {
      // A catalog that fails to load keeps the last language on screen; the
      // choice still saves and loads on the next start.
      console.error(error);
    }
  }

  function columns(): number {
    const rows = [...(grid?.querySelectorAll<HTMLElement>('[role=radio]') ?? [])];
    return rows.filter((row) => row.offsetTop === rows[0]?.offsetTop).length || 1;
  }

  function onkey(event: KeyboardEvent) {
    const index = CHOICES.findIndex((language) => language.code === selected.code);
    const step: Record<string, number> = { ArrowRight: 1, ArrowLeft: -1, ArrowDown: columns(), ArrowUp: -columns() };
    if (event.key in step) {
      event.preventDefault();
      void choose(CHOICES[(index + step[event.key] + CHOICES.length) % CHOICES.length], true);
    } else if (event.key === 'Home' || event.key === 'End') {
      event.preventDefault();
      void choose(CHOICES[event.key === 'Home' ? 0 : CHOICES.length - 1], true);
    } else if (event.key === 'Enter') {
      event.preventDefault();
      void confirm();
    }
  }

  async function confirm() {
    if (saving || phase === 'closing') return;
    cycling = false;
    saving = true;
    try {
      await save(selected.code);
    } catch (error) {
      console.error(error);
      // Asked once: a second press goes on without it, and the next start asks again.
      if (!failed) { failed = true; saving = false; return; }
    }
    phase = 'closing';
    await new Promise((resolve) => setTimeout(resolve, ms(380)));
    dialog?.close();
    const workspace = document.querySelector('#app');
    if (workspace) decodePage(workspace, ms(420));
    ondone();
  }

  onMount(() => {
    dialog?.showModal();
    const preset = CHOICES.find((language) => language.code === gameLanguage);
    if (preset && preset.code !== selected.code) {
      selected = preset;
      void i18n.use(preset.code).then(() => { document.documentElement.lang = i18n.tag; }, (error) => console.error(error));
    }
    requestAnimationFrame(() => {
      placeMark();
      grid?.querySelector<HTMLElement>('[aria-checked=true]')?.focus({ preventScroll: true });
    });
    const opened = setTimeout(() => (phase = 'open'), ms(700));
    const timer = reduced ? undefined : setInterval(() => {
      if (cycling) railIndex = (railIndex + 1) % CHOICES.length;
    }, 1400);
    // Rows change height as fonts load and the text changes language.
    const rows = typeof ResizeObserver === 'function' ? new ResizeObserver(() => placeMark()) : null;
    for (const row of grid?.querySelectorAll('[role=radio]') ?? []) rows?.observe(row);
    return () => { clearTimeout(opened); clearInterval(timer); rows?.disconnect(); };
  });
</script>

<!-- The choice is the point of the dialog, so Escape does not dismiss it; Enter continues with the cursor's language. -->
<dialog bind:this={dialog} data-no-decode class="language-picker" class:entering={phase === 'entering'} class:closing={phase === 'closing'}
  aria-labelledby="language-title" lang={i18n.tag} oncancel={(event) => event.preventDefault()}>
  <header class="rail">
    <span class="sq" aria-hidden="true"></span>
    <h2 id="language-title" lang={cycling ? CHOICES[railIndex].tag : selected.tag}>{railWord}</h2>
  </header>
  <div class="body">
    <p class="lead"><span class="visually-hidden">{t('lang.lead')}</span><span aria-hidden="true">{scramble(t('lang.lead'), progress, frame)}</span></p>
    <p class="sub"><span class="visually-hidden">{t('lang.sub')}</span><span aria-hidden="true">{scramble(t('lang.sub'), progress, frame)}</span></p>
    <div class="grid" role="radiogroup" aria-labelledby="language-title" bind:this={grid} onkeydown={onkey} tabindex="-1">
      <span class="mark" class:ready={mark.ready} aria-hidden="true"
        style:transform="translate({mark.x}px, {mark.y}px)" style:width="{mark.w}px" style:height="{mark.h}px"></span>
      {#each CHOICES as language (language.code)}
        {@const on = language.code === selected.code}
        <button type="button" role="radio" aria-checked={on} tabindex={on ? 0 : -1} data-code={language.code}
          class:on onclick={() => void choose(language)} ondblclick={() => void confirm()}>
          <span class="native" lang={language.tag}>{language.native}</span>
          <span class="english" lang="en">{language.english}</span>
          {#if language.code === gameLanguage}<span class="game">{t('lang.yourGame')}</span>{/if}
        </button>
      {/each}
    </div>
    {#if failed}<p class="ui-notice" data-tone="bad" role="alert">{t('lang.saveFailed')}</p>{/if}
  </div>
  <footer class="foot">
    <span class="hint"><Rich text={t('lang.keys')} /></span>
    <button type="button" class="cta" onclick={() => void confirm()}><span>{scramble(t('lang.continue'), progress, frame)}</span><span class="arrow" aria-hidden="true">→</span></button>
  </footer>
  <span class="scan" aria-hidden="true"></span>
</dialog>

<style>
  .language-picker {
    position: fixed; width: min(46rem, calc(100vw - 2 * var(--s4))); max-height: calc(100vh - 2 * var(--s4));
    margin: auto; padding: 0; overflow: auto;
    background: var(--panel); color: var(--fg); border: 1px solid var(--border);
    box-shadow: 6px 6px 0 var(--shadow-offset);
  }
  .language-picker::backdrop { background: var(--scrim); }

  .rail {
    display: flex; align-items: center; gap: var(--s3);
    min-height: var(--rail); padding: 0 var(--inset);
    background: var(--rail-bg); color: var(--rail-fg);
    border-bottom: 2px solid var(--rail-edge);
  }
  .rail h2 { margin: 0; font: 600 var(--text-control)/1 var(--font-ui); letter-spacing: 0.14em; text-transform: uppercase; }
  .sq { width: 8px; height: 8px; background: var(--rail-fg); flex: none; }

  .body { padding: var(--s5) var(--inset) var(--s4); display: grid; gap: var(--s2); }
  .lead { margin: 0; font: 500 var(--text-statement)/1.3 var(--font-body); }
  .sub { margin: 0 0 var(--s3); color: var(--muted); font: var(--text-body)/var(--leading-body) var(--font-body); }

  .grid { position: relative; overflow: hidden; display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); border-top: 1px var(--rule) var(--hairline); outline: none; }
  @media (max-width: 40rem) {
    .grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  }
  .mark {
    position: absolute; left: 0; top: 0; background: var(--ink-bar); pointer-events: none; opacity: 0;
    transition: transform 0.17s cubic-bezier(0.2, 0.7, 0.2, 1), width 0.17s, height 0.17s;
  }
  .mark.ready { opacity: 1; }
  .grid button {
    position: relative; z-index: 1; display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: baseline; column-gap: var(--s2);
    padding: var(--s2) var(--s3) var(--s2) calc(var(--s3) + var(--s2)); min-height: 3.25rem;
    background: transparent; border: 0; border-bottom: 1px var(--rule) var(--hairline); color: var(--fg);
    text-align: left; cursor: pointer;
  }
  /* The row cursor's square: hover, focus and the chosen language. */
  .grid button::before { content: ''; position: absolute; left: var(--s2); top: 50%; width: 6px; height: 6px; margin-top: -3px; background: currentColor; opacity: 0; }
  .grid button:hover::before, .grid button:focus-visible::before, .grid button.on::before { opacity: 1; }
  .grid button:hover:not(.on) { background: var(--hover); }
  .grid button:focus-visible { outline: 2px solid var(--fg); outline-offset: -4px; }
  .grid button.on { color: var(--on-ink); }
  .grid button.on:focus-visible { outline-color: var(--on-ink); }
  .native { font: 600 var(--text-section)/1.2 var(--font-body); overflow-wrap: break-word; }
  .english { grid-column: 1; font: var(--text-caption)/1.3 var(--font-ui); color: var(--muted); }
  .on .english { color: var(--on-ink-muted); }
  .game { grid-row: 1; grid-column: 2; font: 600 var(--text-caption)/1 var(--font-ui); letter-spacing: 0.08em; text-transform: uppercase; color: var(--muted); }
  .on .game { color: var(--on-ink-muted); }
  /* Two narrow columns leave no room beside the name: the caption goes under it. */
  @media (max-width: 40rem) { .game { grid-row: auto; grid-column: 1 / -1; margin-top: var(--s1); } }

  .foot { display: flex; flex-wrap: wrap; align-items: center; gap: var(--s3) var(--s4); padding: var(--s3) var(--inset) var(--inset); }
  .hint { color: var(--muted); font: var(--text-caption) var(--font-ui); }
  .hint :global(kbd) { font: var(--text-caption) var(--font-mono); border: 1px solid var(--border); padding: 0 var(--s1); margin-right: 2px; }
  .cta {
    margin-left: auto; display: inline-flex; align-items: center; justify-content: space-between; gap: var(--s3);
    min-height: var(--ctl-lg); min-width: 12rem; padding: 0 var(--s4);
    background: var(--ink-bar); color: var(--on-ink); border: 0; font: 500 var(--text-control) var(--font-body); cursor: pointer;
  }
  .cta:focus-visible { outline: 2px solid var(--fg); outline-offset: 2px; }
  .arrow { transition: transform 0.12s; }
  .cta:hover .arrow { transform: translateX(3px); }

  /* Boot: the rail draws across, then the body scans in under a line. */
  .entering .rail { animation: rail 0.26s cubic-bezier(0.3, 0.7, 0.2, 1) both; }
  .entering .body, .entering .foot { animation: wipe 0.42s 0.24s steps(14, end) both; }
  .scan { position: absolute; left: 0; right: 0; top: 0; height: 2px; background: var(--fg); opacity: 0; pointer-events: none; }
  .entering .scan { animation: scanline 0.42s 0.24s steps(14, end) both; }
  /* Continue collapses it like a CRT switching off. */
  .closing { animation: crt 0.36s cubic-bezier(0.6, 0, 0.8, 0.4) forwards; overflow: hidden; }
  .closing::backdrop { opacity: 0; transition: opacity 0.2s 0.16s ease-in; }

  @keyframes rail { from { clip-path: inset(0 100% 0 0); } to { clip-path: inset(0 0 0 0); } }
  @keyframes wipe { from { clip-path: inset(0 0 100% 0); } to { clip-path: inset(0 0 0 0); } }
  @keyframes scanline { 0% { opacity: 1; top: var(--rail); } 95% { opacity: 1; top: calc(100% - 2px); } 100% { opacity: 0; top: calc(100% - 2px); } }
  @keyframes crt { 0% { transform: scale(1, 1); } 55% { transform: scale(1, 0.006); } 100% { transform: scale(0, 0.006); } }
  @media (prefers-reduced-motion: reduce) {
    .language-picker, .language-picker *, .mark, .scan { animation: none !important; transition: none !important; }
  }
</style>
