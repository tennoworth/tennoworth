// Applies controller actions to the page: focus moves by direction inside the
// open dialog (or the Gaming Mode view when none is open), A presses the
// focused control, B backs out of a dialog. Section switches and the other
// shell-wide buttons go to the handlers the layout passes in.
import { heldActions, keyAction, PadReader, type PadAction } from './pad';
import { nearestBox, type Direction } from './spatial';

export interface NavigatorHandlers {
  /** B with no dialog open. */
  back(): void;
  prev(): void;
  next(): void;
  menu(): void;
  scan(): void;
  /** X: the view's own second action, when it has one. */
  secondary(): void;
}

// tabindex="-1" opts a control out of pad focus (the section tabs: LB and RB
// switch sections, and A on a tab would leave the section the player is in).
const FOCUSABLE = 'button:not([disabled]):not([tabindex="-1"]), a[href]:not([tabindex="-1"]), input:not([disabled]):not([type="hidden"]), [tabindex]:not([tabindex="-1"]), [role="option"]';

function visible(element: Element): boolean {
  if (element.closest('[hidden], [inert]')) return false;
  const style = getComputedStyle(element);
  return style.visibility !== 'hidden' && element.getClientRects().length > 0;
}

/** The innermost surface that owns focus: an open dialog, else the layout. */
function scope(root: HTMLElement): HTMLElement {
  const dialogs = [...document.querySelectorAll<HTMLElement>('dialog[open], [role="dialog"][aria-modal="true"], [role="alertdialog"]')].filter(visible);
  return dialogs.at(-1) ?? root;
}

function focusables(within: HTMLElement): HTMLElement[] {
  return [...within.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(visible);
}

function isTextEntry(element: Element | null): element is HTMLInputElement {
  return element instanceof HTMLInputElement && !['checkbox', 'radio', 'button', 'range', 'number'].includes(element.type);
}

function focus(element: HTMLElement) {
  element.focus({ preventScroll: true });
  element.scrollIntoView({ block: 'nearest', inline: 'nearest' });
}

export function moveFocus(root: HTMLElement, direction: Direction): boolean {
  const within = scope(root);
  const candidates = focusables(within);
  if (!candidates.length) return false;
  const current = document.activeElement instanceof HTMLElement && within.contains(document.activeElement) ? document.activeElement : null;
  if (!current) {
    focus(candidates.find((element) => element.dataset.padDefault !== undefined) ?? candidates[0]);
    return true;
  }
  const others = candidates.filter((element) => element !== current);
  const index = nearestBox(current.getBoundingClientRect(), others.map((element) => element.getBoundingClientRect()), direction);
  if (index < 0) return false;
  focus(others[index]);
  return true;
}

/**
 * Puts focus back when it has fallen to the page: the focused control was
 * disabled (Scan while scanning), re-rendered (a toggled setting) or arrived
 * late (picks still calculating). Otherwise the next press acts from nowhere
 * and nothing on screen shows where the player is. Focus goes to the control
 * nearest where it last was, else to the surface's default.
 */
export function recoverFocus(root: HTMLElement, last: DOMRect | null): boolean {
  const active = document.activeElement;
  if (active && active !== document.body && active.isConnected) return false;
  const candidates = focusables(scope(root));
  if (!candidates.length) return false;
  let target = candidates.find((element) => element.dataset.padDefault !== undefined) ?? candidates[0];
  if (last) {
    const distance = (element: HTMLElement) => {
      const box = element.getBoundingClientRect();
      return Math.hypot(box.left + box.width / 2 - (last.left + last.width / 2), box.top + box.height / 2 - (last.top + last.height / 2));
    };
    target = candidates.reduce((best, element) => (distance(element) < distance(best) ? element : best));
  }
  focus(target);
  return true;
}

function step(input: HTMLInputElement, up: boolean) {
  if (up) input.stepUp(); else input.stepDown();
  input.dispatchEvent(new Event('input', { bubbles: true }));
  input.dispatchEvent(new Event('change', { bubbles: true }));
}

function closeDialog(within: HTMLElement) {
  const target = document.activeElement instanceof HTMLElement && within.contains(document.activeElement) ? document.activeElement : within;
  // The dialogs here close on Escape; send it where their handlers listen.
  target.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
  if (within instanceof HTMLDialogElement && within.open) {
    const cancel = new Event('cancel', { cancelable: true });
    if (within.dispatchEvent(cancel)) within.close();
  }
}

export function apply(root: HTMLElement, action: PadAction, handlers: NavigatorHandlers): void {
  const within = scope(root);
  const active = document.activeElement;
  const inDialog = within !== root;
  switch (action) {
    case 'up': case 'down':
      // Up and down always move on: a price must not change because the
      // player was passing through its field.
      if (!moveFocus(root, action) && !inDialog) root.querySelector<HTMLElement>('[data-pad-scroll]')?.scrollBy({ top: action === 'down' ? 120 : -120 });
      return;
    case 'left': case 'right':
      if (active instanceof HTMLInputElement && (active.type === 'number' || active.type === 'range') && within.contains(active)) { step(active, action === 'right'); return; }
      moveFocus(root, action);
      return;
    case 'accept':
      if (active instanceof HTMLElement && within.contains(active) && active !== within) active.click();
      else moveFocus(root, 'down');
      return;
    case 'back':
      if (inDialog) closeDialog(within); else handlers.back();
      return;
    case 'prev': if (!inDialog) handlers.prev(); return;
    case 'next': if (!inDialog) handlers.next(); return;
    case 'menu': if (!inDialog) handlers.menu(); return;
    case 'scan': if (!inDialog) handlers.scan(); return;
    case 'secondary': if (!inDialog) handlers.secondary(); return;
  }
}

/**
 * Reads every connected pad each frame while the window has focus, and the
 * keyboard equivalents, and applies them. Returns the stop function.
 */
export function startNavigator(root: HTMLElement, handlers: NavigatorHandlers, onpad?: () => void): () => void {
  const reader = new PadReader();
  let frame = 0;
  let last: DOMRect | null = null;
  const poll = () => {
    frame = requestAnimationFrame(poll);
    if (!document.hasFocus()) return;
    const active = document.activeElement;
    if (active instanceof HTMLElement && active !== document.body && active.isConnected) last = active.getBoundingClientRect();
    else recoverFocus(root, last);
    if (typeof navigator.getGamepads !== 'function') return;
    const held = new Set<PadAction>();
    for (const pad of navigator.getGamepads()) if (pad?.connected) for (const action of heldActions(pad)) held.add(action);
    const fired = reader.step(held, performance.now());
    if (fired.length) onpad?.();
    for (const action of fired) apply(root, action, handlers);
  };
  const onkey = (event: KeyboardEvent) => {
    if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey) return;
    const action = keyAction(event.key);
    if (!action) return;
    const active = document.activeElement;
    // Typing keeps its own keys; Escape and Enter still leave the field's dialog.
    if (isTextEntry(active) && action !== 'back' && action !== 'accept') return;
    if (action === 'accept' && (active instanceof HTMLButtonElement || active instanceof HTMLAnchorElement || isTextEntry(active))) return;
    if (action === 'back' && scope(root) !== root) return;
    if (event.key === 'Backspace' && isTextEntry(active)) return;
    event.preventDefault();
    apply(root, action, handlers);
  };
  frame = requestAnimationFrame(poll);
  window.addEventListener('keydown', onkey);
  return () => { cancelAnimationFrame(frame); window.removeEventListener('keydown', onkey); };
}
