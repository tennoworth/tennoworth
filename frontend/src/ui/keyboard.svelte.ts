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

export function typingTarget(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && !!target.closest('input, textarea, select, [contenteditable]:not([contenteditable="false"]), [role="textbox"], [role="combobox"]');
}

export function visible(element: Element): boolean {
  return element.getClientRects().length > 0 && getComputedStyle(element).visibility !== 'hidden' && !element.closest('[inert]');
}

export function shortcutBlocked(event: KeyboardEvent): boolean {
  return event.defaultPrevented || event.isComposing || event.repeat || typingTarget(event.target) ||
    [...document.querySelectorAll('dialog[open], [role="dialog"], [aria-modal="true"], [aria-expanded="true"][aria-controls]')].some(visible);
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
  document.addEventListener('keydown', handler);
  return { destroy() { document.removeEventListener('keydown', handler); delete node.dataset.shortcutSearch; } };
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
