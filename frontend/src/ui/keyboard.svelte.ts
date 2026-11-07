import type { SettingsStore } from '../contracts/state-store';

type CharacterAction = 'search' | 'help';
interface KeyboardMapSource extends EventTarget {
  getLayoutMap(): Promise<{ get(code: string): string | undefined }>;
}

export const keyboard = $state({ search: '/', help: '?', enabled: true, navigation: null as string | null, commandKey: 'K', modifier: 'Ctrl', layoutAvailable: false });

export function readKeyboardPreferences(store?: SettingsStore) {
  keyboard.search = '/'; keyboard.help = '?'; keyboard.enabled = true; keyboard.navigation = null;
  try {
    const saved = JSON.parse(store?.getSetting('keyboard') ?? 'null');
    if (saved && validCharacter(saved.search) && validCharacter(saved.help) && saved.search !== saved.help && typeof saved.enabled === 'boolean' && (saved.navigation === null || validCharacter(saved.navigation))) {
      keyboard.search = saved.search; keyboard.help = saved.help; keyboard.enabled = saved.enabled; keyboard.navigation = saved.navigation;
    }
  } catch { /* A damaged optional preference falls back to usable defaults. */ }
}

export function validCharacter(key: unknown): key is string {
  return typeof key === 'string' && [...key].length === 1 && !/\s|\p{C}/u.test(key);
}

export function saveKeyboardPreferences(store?: SettingsStore): Promise<void> {
  return store?.setSetting('keyboard', JSON.stringify({ search: keyboard.search, help: keyboard.help, enabled: keyboard.enabled, navigation: keyboard.navigation })) ?? Promise.resolve();
}

export function characterKey(event: KeyboardEvent): string | null {
  if (event.isComposing || event.repeat || event.metaKey || !validCharacter(event.key)) return null;
  const altGraph = event.getModifierState('AltGraph');
  if ((event.ctrlKey || event.altKey) && !altGraph) return null;
  return event.key;
}

// Checkboxes, radios and buttons take no characters, so a shortcut pressed
// on one is meant for the app. Most of Settings is checkboxes; treating them
// as text fields made F6 and Ctrl+K do nothing there.
const TEXT_ENTRY = 'input:not([type="checkbox"], [type="radio"], [type="button"], [type="submit"], [type="reset"], [type="range"], [type="color"], [type="file"]), textarea, select, [contenteditable]:not([contenteditable="false"]), [role="textbox"]';

/** Whether a key pressed here is text. A drawn select's type-ahead takes
 *  characters too, but not chords or F6 (`chord`). */
export function typingTarget(target: EventTarget | null, chord = false): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.closest(TEXT_ENTRY)) return true;
  return !chord && !!target.closest('[role="combobox"]');
}

export function visible(element: Element): boolean {
  return element.getClientRects().length > 0 && getComputedStyle(element).visibility !== 'hidden' && !element.closest('[inert]');
}

/** A modal always holds the keys. An open menu or popover holds them only
 *  while focus is in it or on its trigger: one left open behind the user
 *  silently turned every shortcut off. */
export function shortcutBlocked(event: KeyboardEvent, chord = false): boolean {
  if (event.defaultPrevented || event.isComposing || event.repeat || typingTarget(event.target, chord)) return true;
  if ([...document.querySelectorAll('dialog[open]:modal, [aria-modal="true"]')].some(visible)) return true;
  const focus = document.activeElement;
  const holds = (element: Element) => visible(element) && !!focus && element.contains(focus);
  if ([...document.querySelectorAll('[role="dialog"]:not(dialog), dialog[open]:not(:modal)')].some(holds)) return true;
  return [...document.querySelectorAll<HTMLElement>('[aria-expanded="true"][aria-controls]')].some((trigger) => {
    const popover = document.getElementById(trigger.getAttribute('aria-controls') ?? '');
    return !!popover && visible(popover) && (holds(popover) || trigger === focus);
  });
}

/** A place the view picker can go that is not a sidebar view, such as a Settings section. */
export interface KeyboardDestination { label: string; keywords?: string; run: () => void }

/** Asks the keyboard tools to open help or the view picker, from a click on
 *  the key legend. `count` changes on every request. */
export const keyboardRequest = $state({ mode: 'help' as 'help' | 'navigate', count: 0 });
export function requestKeyboard(mode: 'help' | 'navigate'): void {
  keyboardRequest.mode = mode; keyboardRequest.count++;
}

/** Focuses the view's primary search, as the search key does. */
export function focusSearch(): boolean {
  const first = [...document.querySelectorAll<HTMLInputElement>('[data-shortcut-search]')].find(el => visible(el) && !el.disabled);
  first?.focus();
  return !!first;
}

export function searchShortcut(node: HTMLInputElement, onjump?: () => void) {
  node.dataset.shortcutSearch = '';
  const handler = (event: KeyboardEvent) => {
    if (!keyboard.enabled || shortcutBlocked(event) || characterKey(event) !== keyboard.search || !visible(node) || node.disabled) return;
    // Several retained views can own searches; a page starts with its primary one.
    const first = [...document.querySelectorAll<HTMLInputElement>('[data-shortcut-search]')].find(el => visible(el) && !el.disabled);
    if (first !== node) return;
    event.preventDefault(); node.focus(); onjump?.();
  };
  // Escape empties a search; a second Escape on the empty field, or Escape
  // while a suggestion list is open, belongs to whatever owns the field.
  const clear = (event: KeyboardEvent) => {
    if (event.key !== 'Escape' || event.isComposing || !node.value || node.getAttribute('aria-expanded') === 'true') return;
    event.preventDefault(); event.stopPropagation();
    node.value = '';
    node.dispatchEvent(new Event('input', { bubbles: true }));
  };
  document.addEventListener('keydown', handler);
  node.addEventListener('keydown', clear);
  return { destroy() { document.removeEventListener('keydown', handler); node.removeEventListener('keydown', clear); delete node.dataset.shortcutSearch; } };
}

/** Layout data stays in this closure; only the command key's label reaches the UI. */
export function watchKeyboardLayout() {
  const source = (navigator as Navigator & { keyboard?: KeyboardMapSource }).keyboard;
  let alive = true;
  let epoch = 0;
  keyboard.modifier = /Mac|iPhone|iPad/.test(navigator.platform) ? 'Command' : 'Ctrl';
  const refresh = async () => {
    const current = ++epoch;
    try {
      const map = await source?.getLayoutMap();
      if (!alive || current !== epoch) return;
      const label = map?.get('KeyK');
      keyboard.layoutAvailable = validCharacter(label);
      keyboard.commandKey = label && validCharacter(label) ? label.toLocaleUpperCase() : 'K';
    } catch {
      if (alive && current === epoch) { keyboard.layoutAvailable = false; keyboard.commandKey = 'K'; }
    }
  };
  void refresh();
  source?.addEventListener?.('layoutchange', refresh);
  window.addEventListener('focus', refresh);
  return () => { alive = false; source?.removeEventListener?.('layoutchange', refresh); window.removeEventListener('focus', refresh); };
}

export function matchesCharacter(event: KeyboardEvent, action: CharacterAction): boolean {
  return keyboard.enabled && characterKey(event) === keyboard[action];
}

let tableId = 0;
export function tableNavigation(node: HTMLElement, label: string) {
  const id = `keyboard-table-end-${++tableId}`;
  node.tabIndex = 0; node.setAttribute('role', 'region'); node.setAttribute('aria-label', `${label}; scroll for all columns`);
  const skip = document.createElement('a');
  skip.className = 'skip-navigation'; skip.href = `#${id}`; skip.textContent = `Skip ${label.toLocaleLowerCase()}`;
  const end = document.createElement('span');
  end.id = id; end.tabIndex = -1; end.setAttribute('role', 'note'); end.setAttribute('aria-label', `End of ${label.toLocaleLowerCase()}`);
  skip.onclick = event => { event.preventDefault(); end.focus(); };
  node.before(skip); node.after(end);
  return { destroy() { skip.remove(); end.remove(); } };
}
