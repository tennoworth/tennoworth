<script lang="ts">
  import { onMount, tick } from 'svelte';
  import type { SettingsStore } from '../contracts/state-store';
  import DialogHeader from './DialogHeader.svelte';
  import { characterKey, keyboard, matchesCharacter, readKeyboardPreferences, saveKeyboardPreferences, shortcutBlocked, visible, watchKeyboardLayout } from './keyboard.svelte';

  let { desktop = false, compact = false, store }: { desktop?: boolean; compact?: boolean; store?: SettingsStore } = $props();
  let dialog: HTMLDialogElement;
  let mode = $state<'help' | 'navigate'>('help');
  let query = $state('');
  let recording = $state<'search' | 'help' | 'navigation' | null>(null);
  let error = $state('');
  let saveFailed = $state(false);
  let destinations = $state<{ label: string; element: HTMLButtonElement }[]>([]);
  let selected = $state(0);
  let previous: HTMLElement | null = null;
  let filterInput = $state<HTMLInputElement>();
  const matches = $derived(destinations.filter(item => item.label.toLocaleLowerCase().includes(query.toLocaleLowerCase())));

  function focusView() {
    const main = document.querySelector<HTMLElement>('main.workspace, main.landing');
    if (!main) return;
    const heading = [...main.querySelectorAll<HTMLElement>('h2')].find(visible);
    const target = heading ?? main;
    target.tabIndex = -1;
    target.focus({ preventScroll: true });
  }
  async function open(next: 'help' | 'navigate') {
    if ([...document.querySelectorAll('dialog[open], [aria-modal="true"]')].some(visible)) return;
    previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    mode = next; query = ''; selected = 0; recording = null; error = '';
    destinations = [...document.querySelectorAll<HTMLButtonElement>('.sidebar .nav-item')].filter(visible)
      .map(element => ({ label: element.querySelector('span')?.textContent?.trim() ?? element.textContent?.trim() ?? '', element }));
    await tick(); dialog.showModal();
    if (next === 'navigate') filterInput?.focus();
  }
  async function showDestinations() {
    mode = 'navigate'; recording = null; query = ''; selected = 0;
    await tick(); filterInput?.focus();
  }
  function restoreFocus() {
    if (dialog.open) return;
    recording = null;
    if (previous?.isConnected && visible(previous)) previous.focus({ preventScroll: true });
    previous = null;
  }
  function close() { dialog.close(); restoreFocus(); }
  async function go(index: number) {
    const destination = matches[index];
    if (!destination) return;
    previous = null; close(); destination.element.click();
    await tick(); focusView();
  }
  async function save() {
    error = ''; saveFailed = false;
    try { await saveKeyboardPreferences(store); }
    catch { saveFailed = true; error = 'This choice works for this window, but could not be saved. Try again.'; }
  }
  async function reset() { recording = null; readKeyboardPreferences(); await save(); }
  function dialogKey(event: KeyboardEvent) {
    if (recording) {
      if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); recording = null; return; }
      if (event.key === 'Tab') { recording = null; return; }
      event.preventDefault(); event.stopPropagation();
      const key = characterKey(event);
      if (!key) { error = 'Type one character, using Shift or AltGr if needed.'; return; }
      if (recording === 'navigation' && event.getModifierState('AltGraph')) { error = 'Choose a character without AltGr. The command modifier is added automatically.'; return; }
      if (recording !== 'navigation' && key === keyboard[recording === 'search' ? 'help' : 'search']) { error = 'Search and help need different characters.'; return; }
      keyboard[recording] = key; recording = null; void save(); return;
    }
    if (mode === 'navigate' && event.target === filterInput) {
      if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key) && !event.isComposing) {
        // Home/End remain text-editing keys when a query is present.
        if (query && (event.key === 'Home' || event.key === 'End')) return;
        event.preventDefault();
        selected = !matches.length ? 0 : event.key === 'Home' ? 0 : event.key === 'End' ? matches.length - 1 : (selected + (event.key === 'ArrowDown' ? 1 : -1) + matches.length) % matches.length;
        document.getElementById(`keyboard-destination-${selected}`)?.scrollIntoView({ block: 'nearest' });
      }
      if (event.key === 'Enter' && !event.isComposing) { event.preventDefault(); void go(selected); }
    }
    if (event.key !== 'Tab') return;
    const controls = [...dialog.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), a[href]')].filter(el => visible(el) && el.tabIndex >= 0);
    const first = controls[0], last = controls.at(-1);
    if (!controls.includes(document.activeElement as HTMLElement) || (event.shiftKey ? document.activeElement === first : document.activeElement === last)) {
      event.preventDefault(); (event.shiftKey ? last : first)?.focus();
    }
  }
  function cycleRegion(backwards: boolean) {
    const regions = ['.statusbar', '.sidebar nav', 'main.workspace, main.landing'].map(selector => document.querySelector<HTMLElement>(selector)).filter((element): element is HTMLElement => !!element && visible(element));
    const current = regions.findIndex(region => region.contains(document.activeElement));
    const next = regions[current < 0 ? backwards ? regions.length - 1 : 0 : (current + (backwards ? -1 : 1) + regions.length) % regions.length];
    if (!next) return;
    if (next.tagName === 'MAIN') focusView();
    else {
      const control = [...next.querySelectorAll<HTMLElement>('button:not(:disabled), a[href]')].find(visible);
      if (control) control.focus();
      else { next.tabIndex = -1; next.focus(); }
    }
  }
  onMount(() => {
    readKeyboardPreferences(store);
    const stop = watchKeyboardLayout();
    const handler = (event: KeyboardEvent) => {
      if (shortcutBlocked(event)) return;
      if (matchesCharacter(event, 'help')) { event.preventDefault(); void open('help'); }
      else if (desktop && event.key === 'F6' && !event.altKey && !event.ctrlKey && !event.metaKey) { event.preventDefault(); cycleRegion(event.shiftKey); }
      else if (desktop && !event.altKey && (keyboard.navigation !== null || !event.shiftKey) && !event.getModifierState('AltGraph') && (keyboard.modifier === 'Command' ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey) && (keyboard.navigation !== null ? event.key.toLocaleLowerCase() === keyboard.navigation.toLocaleLowerCase() : keyboard.layoutAvailable ? event.code === 'KeyK' : event.key.toLowerCase() === 'k')) {
        event.preventDefault(); void open('navigate');
      }
    };
    const navigation = async (event: MouseEvent) => {
      if (event.detail !== 0 || !(event.target instanceof HTMLElement)) return;
      const target = event.target;
      const view = target.closest('.sidebar .nav-item');
      const disclosure = target.closest<HTMLElement>('[aria-expanded][aria-controls]');
      const anchor = target.closest<HTMLAnchorElement>('a[href^="#"]');
      await tick();
      if (view) focusView();
      else if (disclosure?.getAttribute('aria-expanded') === 'true') {
        const popover = document.getElementById(disclosure.getAttribute('aria-controls') ?? '');
        const controls = [...popover?.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled), a[href]') ?? []].filter(visible);
        const selectedControl = controls.find(control => control.matches('input:checked'));
        (selectedControl ?? controls[0])?.focus();
      } else if (anchor && !event.defaultPrevented) {
        const section = document.getElementById(anchor.hash.slice(1));
        if (section) { section.tabIndex = -1; section.focus({ preventScroll: true }); }
      }
    };
    document.addEventListener('keydown', handler);
    document.addEventListener('click', navigation);
    return () => { stop(); document.removeEventListener('keydown', handler); document.removeEventListener('click', navigation); };
  });
</script>

<div class="keyboard-tools" class:compact>
  <button type="button" class="btn ghost xs" onclick={() => open('help')}>Keyboard shortcuts{#if keyboard.enabled} <kbd>{keyboard.help}</kbd>{/if}</button>
</div>
<dialog bind:this={dialog} class="cryptobox keyboard-dialog" aria-labelledby="keyboard-title" onclose={restoreFocus} oncancel={(event) => { event.preventDefault(); close(); }} onkeydown={dialogKey}>
  <DialogHeader titleId="keyboard-title" title={mode === 'help' ? 'Keyboard shortcuts' : 'Go to a view'} onclose={close} />
  <div class="keyboard-body ui-stack">
    {#if mode === 'navigate'}
      <label class="ui-field"><span>Find a view</span><input class="ui-input" bind:this={filterInput} bind:value={query} oninput={() => selected = 0} role="combobox" aria-autocomplete="list" aria-expanded="true" aria-controls="keyboard-destinations" aria-activedescendant={matches.length ? `keyboard-destination-${selected}` : undefined} /></label>
      <div id="keyboard-destinations" role="listbox" aria-label="Views" class="destinations">
        {#each matches as destination, index}
          <button type="button" id="keyboard-destination-{index}" role="option" tabindex="-1" aria-selected={selected === index} class="destination" onclick={() => go(index)}>{destination.label}</button>
        {:else}<p role="status">No matching views.</p>{/each}
      </div>
      <p class="muted">↑ / ↓ choose · Enter opens · Escape closes</p>
    {:else}
      {#if desktop}<button class="btn" onclick={showDestinations}>Go to a view <kbd>{keyboard.modifier}+{keyboard.navigation ?? keyboard.commandKey}</kbd></button>{/if}
      <p>Shortcuts work in this window when you are not typing or using a dialog. Search and help follow the character you type on your current keyboard, including Shift or AltGr.</p>
      <div class="shortcut-row"><span>Focus this view’s search</span><kbd>{keyboard.search}</kbd><button class="btn" onclick={() => { recording = 'search'; error = ''; saveFailed = false; }}>{recording === 'search' ? 'Type a character…' : 'Change search key'}</button></div>
      <div class="shortcut-row"><span>Open keyboard help</span><kbd>{keyboard.help}</kbd><button class="btn" onclick={() => { recording = 'help'; error = ''; saveFailed = false; }}>{recording === 'help' ? 'Type a character…' : 'Change help key'}</button></div>
      {#if desktop}
        <div class="shortcut-row"><span>Find a view</span><kbd>{keyboard.modifier}+{keyboard.navigation ?? keyboard.commandKey}</kbd><button class="btn" onclick={() => { recording = 'navigation'; error = ''; saveFailed = false; }}>{recording === 'navigation' ? 'Type a character…' : 'Change view key'}</button></div>
        <div class="shortcut-row"><span>Next / previous region</span><span><kbd>F6</kbd> / <kbd>Shift+F6</kbd></span></div>
      {/if}
      <div class="shortcut-row"><span>Next / previous control</span><span><kbd>Tab</kbd> / <kbd>Shift+Tab</kbd></span></div>
      <div class="shortcut-row"><span>Close a dialog or menu</span><kbd>Escape</kbd></div>
      <p class="muted">{keyboard.layoutAvailable ? 'Key labels adapt locally when your keyboard layout changes.' : 'Automatic key labels are unavailable in this browser. You can record search and help characters above.'} Keyboard layout information is never saved or sent. Only your shortcut choices are saved on this device.</p>
      <label class="enable-shortcuts"><input type="checkbox" bind:checked={keyboard.enabled} onchange={() => save()} /> Enable single-character shortcuts</label>
      <button class="btn" onclick={reset}>Reset shortcut choices</button>
      {#if recording}<p role="status">Type the character to use{recording === 'navigation' ? `; ${keyboard.modifier} is added automatically` : ''}. Escape cancels; Tab moves on.</p>{/if}
      {#if error}<div class="ui-notice" data-tone="warn" role="alert">{error} {#if saveFailed}<button class="btn" onclick={save}>Retry saving</button>{/if}</div>{/if}
    {/if}
  </div>
</dialog>

<style>
  .keyboard-tools { display: flex; flex-wrap: wrap; gap: var(--s1); }
  .keyboard-tools kbd { margin-inline-start: var(--s1); }
  .keyboard-tools.compact { width: 100%; }
  .compact > button { display: flex; align-items: center; justify-content: space-between; width: 100%; min-height: var(--ctl); padding: 0 var(--s1); border: 0; background: transparent; color: var(--muted); font: inherit; text-transform: none; letter-spacing: normal; }
  .compact > button:hover { color: var(--fg); text-decoration: underline dotted; }
  dialog.keyboard-dialog { max-width: 40rem; }
  :global(html[data-look="yorha"]) dialog.keyboard-dialog :global(header) { margin: 0; padding: var(--s2) var(--s3); flex-direction: row; background: var(--rail-bg); color: var(--rail-fg); }
  dialog.keyboard-dialog :global(header h2), dialog.keyboard-dialog :global(header button) { color: var(--rail-fg); }
  dialog.keyboard-dialog :global(header button) { border-color: var(--rail-edge); }
  .keyboard-body { padding: var(--inset); }
  .keyboard-body p { margin: 0; }
  .shortcut-row { display: flex; align-items: center; flex-wrap: wrap; gap: var(--s2); padding-block: var(--s1); border-bottom: 1px dotted var(--rule); }
  .shortcut-row > :first-child { flex: 1 1 12rem; }
  .enable-shortcuts { display: flex; flex-direction: row; align-items: center; gap: var(--s2); }
  .destinations { max-height: 45dvh; overflow-y: auto; }
  .destination { display: block; width: 100%; min-height: var(--ctl-lg); text-align: start; padding: var(--s2); background: var(--panel); color: var(--fg); border: 0; border-bottom: 1px dotted var(--rule); font: inherit; cursor: pointer; }
  .destination[aria-selected="true"] { background: var(--ink-bar); color: var(--on-ink); }
</style>
