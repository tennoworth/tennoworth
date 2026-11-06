<script lang="ts" module>
  export interface SelectOption<V> { value: V; label: string }
</script>

<script lang="ts" generics="T extends string | number">
  // The app's single-choice dropdown. A native <select> hands its field and
  // its open list to the platform: WebKitGTK paints a white GTK box and menu
  // on the paper and bench themes alike, and neither can be restyled. This
  // draws both from the theme tokens and follows the WAI-ARIA select-only
  // combobox pattern, so the keyboard and screen readers get what a native
  // select would give them.
  import { tick } from 'svelte';

  let {
    value,
    options,
    onchange,
    disabled = false,
    id,
    labelledby,
    label,
    class: className = '',
  }: {
    value: T;
    options: readonly SelectOption<T>[];
    onchange: (value: T) => void;
    disabled?: boolean;
    id?: string;
    /** Id of the visible label. Preferred: a wrapping <label> would also read the chosen option into the name. */
    labelledby?: string;
    /** Accessible name when no visible label element exists. */
    label?: string;
    class?: string;
  } = $props();

  // Where the webview lacks the popover API (or a test DOM hides [popover]
  // without opening it), the list stays an ordinary fixed element.
  const topLayer = typeof HTMLElement !== 'undefined' && 'showPopover' in HTMLElement.prototype;
  const uid = $props.id();
  const listId = `${uid}-list`;
  const optionId = (index: number) => `${uid}-opt-${index}`;

  let open = $state(false);
  let active = $state(-1);
  let trigger = $state<HTMLButtonElement>();
  let list = $state<HTMLUListElement>();
  let place = $state({ left: 0, top: 0, width: 0, maxHeight: 0 });

  const selectedIndex = $derived(options.findIndex((option) => option.value === value));
  // A value the list no longer offers (a search narrowed it away) shows blank, as a native select does.
  const shown = $derived(selectedIndex >= 0 ? options[selectedIndex].label : '');

  let typed = '';
  let typedAt = 0;

  function show(start: number) {
    if (disabled || open) return;
    active = Math.max(0, Math.min(options.length - 1, start));
    open = true;
  }

  function close(focusTrigger = true) {
    open = false;
    if (focusTrigger) trigger?.focus();
  }

  function choose(index: number, focusTrigger = true) {
    const option = options[index];
    close(focusTrigger);
    if (option && option.value !== value) onchange(option.value);
  }

  function move(to: number) {
    if (!options.length) return;
    active = Math.max(0, Math.min(options.length - 1, to));
  }

  // Type to jump, as a native select does: letters within half a second build
  // one prefix; repeating a single letter cycles through the matches.
  function seek(char: string): number {
    const now = Date.now();
    typed = now - typedAt > 500 ? char : typed + char;
    typedAt = now;
    const query = typed.toLocaleLowerCase();
    const cycling = [...query].every((c) => c === query[0]);
    const from = open ? active : selectedIndex;
    const order = options.map((_, i) => (from + 1 + i) % options.length);
    const hit = (needle: string) => order.find((i) => options[i].label.toLocaleLowerCase().startsWith(needle));
    return hit(query) ?? (cycling ? hit(query[0]) : undefined) ?? -1;
  }

  function onkeydown(event: KeyboardEvent) {
    if (disabled) return;
    const { key, altKey } = event;
    const char = key.length === 1 && !event.ctrlKey && !event.metaKey && !altKey;
    // Space types a character while a prefix is being typed, and picks otherwise.
    const typing = typed !== '' && Date.now() - typedAt < 500;
    const pick = key === 'Enter' || (key === ' ' && !typing);
    if (!open) {
      if (key === 'ArrowDown' || key === 'ArrowUp' || pick) { event.preventDefault(); show(selectedIndex < 0 ? 0 : selectedIndex); }
      else if (key === 'Home') { event.preventDefault(); show(0); }
      else if (key === 'End') { event.preventDefault(); show(options.length - 1); }
      else if (char) { event.preventDefault(); const hit = seek(key); if (hit >= 0) show(hit); }
      return;
    }
    if (key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(); }
    else if (pick || (key === 'ArrowUp' && altKey)) { event.preventDefault(); choose(active); }
    else if (key === 'Tab') choose(active, false);
    else if (key === 'ArrowDown') { event.preventDefault(); move(active + 1); }
    else if (key === 'ArrowUp') { event.preventDefault(); move(active - 1); }
    else if (key === 'Home') { event.preventDefault(); move(0); }
    else if (key === 'End') { event.preventDefault(); move(options.length - 1); }
    else if (key === 'PageDown') { event.preventDefault(); move(active + 10); }
    else if (key === 'PageUp') { event.preventDefault(); move(active - 10); }
    else if (char) { event.preventDefault(); const hit = seek(key); if (hit >= 0) move(hit); }
  }

  // Space activates a button on keyup; the keydown above already acted on it.
  function onkeyup(event: KeyboardEvent) { if (event.key === ' ') event.preventDefault(); }

  // Below the field, or above it when that has more room; never past the
  // window, with the list scrolling inside whatever height is left.
  function measure() {
    if (!trigger || !list) return;
    const gutter = 8;
    const box = trigger.getBoundingClientRect();
    const below = innerHeight - box.bottom - gutter;
    const above = box.top - gutter;
    const natural = list.scrollHeight + 2;
    const up = natural > below && above > below;
    const room = Math.max(0, (up ? above : below) - 2);
    const width = Math.min(Math.max(box.width, list.scrollWidth), innerWidth - gutter * 2);
    const left = Math.max(gutter, Math.min(box.left, innerWidth - gutter - width));
    const height = Math.min(natural, room);
    place = { left, top: up ? box.top - 2 - height : box.bottom + 2, width: box.width, maxHeight: room };
  }

  $effect(() => {
    if (!open || !list) return;
    const node = list;
    // Top layer where the webview has it, so no panel, dialog or stacking
    // context can clip or cover the list.
    if (topLayer) { try { node.showPopover(); } catch { /* fixed positioning still applies */ } }
    measure();
    const outside = (event: PointerEvent) => {
      const target = event.target as Node | null;
      if (target && (node.contains(target) || trigger?.contains(target))) return;
      close(false);
    };
    const blur = () => close(false);
    const scrolled = (event: Event) => { if (event.target !== node) measure(); };
    document.addEventListener('pointerdown', outside, true);
    addEventListener('resize', measure);
    addEventListener('scroll', scrolled, true);
    addEventListener('blur', blur);
    return () => {
      document.removeEventListener('pointerdown', outside, true);
      removeEventListener('resize', measure);
      removeEventListener('scroll', scrolled, true);
      removeEventListener('blur', blur);
    };
  });

  $effect(() => {
    if (!open || active < 0 || !list) return;
    const index = active;
    void tick().then(() => document.getElementById(optionId(index))?.scrollIntoView?.({ block: 'nearest' }));
  });

  // Reads only `disabled`: a field disabled mid-save drops its open list.
  $effect(() => { if (disabled) open = false; });
</script>

<button
  type="button"
  class="ui-select {className}"
  {id}
  bind:this={trigger}
  role="combobox"
  aria-haspopup="listbox"
  aria-expanded={open}
  aria-controls={listId}
  aria-labelledby={labelledby}
  aria-label={labelledby ? undefined : label}
  aria-activedescendant={open && active >= 0 ? optionId(active) : undefined}
  data-value={String(value)}
  {disabled}
  onclick={() => (open ? close() : show(selectedIndex < 0 ? 0 : selectedIndex))}
  {onkeydown}
  {onkeyup}
>
  <span class="ui-select-value">
    <span class="ui-select-current">{shown}</span>
    {#each options as option (option.value)}<span class="ui-select-sizer" aria-hidden="true">{option.label}</span>{/each}
  </span>
  <span class="ui-select-caret" aria-hidden="true">▾</span>
</button>
{#if open}
  <ul
    id={listId}
    class="ui-select-list"
    role="listbox"
    popover={topLayer ? 'manual' : undefined}
    aria-labelledby={labelledby}
    aria-label={labelledby ? undefined : label}
    bind:this={list}
    onmousedown={(event) => event.preventDefault()}
    style:left="{place.left}px"
    style:top="{place.top}px"
    style:min-width="{place.width}px"
    style:max-height="{place.maxHeight}px"
  >
    {#each options as option, index (option.value)}
      <!-- Pointer only: the keyboard stays on the combobox and moves aria-activedescendant. -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
      <li
        id={optionId(index)}
        role="option"
        aria-selected={index === selectedIndex}
        data-active={index === active || undefined}
        data-value={String(option.value)}
        onpointermove={() => (active = index)}
        onclick={() => choose(index)}
      >{option.label}</li>
    {/each}
  </ul>
{/if}
