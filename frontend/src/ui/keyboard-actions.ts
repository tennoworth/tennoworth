// Small keyboard behaviours shared by many views.

const TEXT_ENTRY = 'input:not([type="checkbox"], [type="radio"], [type="button"], [type="submit"], [type="reset"], [type="range"], [type="color"], [type="file"]), textarea';

/** A control whose own action is running. `disabled` would drop keyboard
 *  focus to the page in Chromium (WebView2), so the user lost their place
 *  after every save; this keeps the control focusable, announces it as
 *  unavailable and swallows activation until the work finishes. */
export function busy(node: HTMLElement, on: boolean | undefined) {
  const blocked = () => node.getAttribute('aria-disabled') === 'true';
  const stop = (event: Event) => { if (blocked()) { event.preventDefault(); event.stopImmediatePropagation(); } };
  const keys = (event: KeyboardEvent) => {
    if (!blocked()) return;
    const typing = node.matches(TEXT_ENTRY);
    if (event.key === 'Enter' || event.key === ' ' || (typing && (event.key === 'ArrowUp' || event.key === 'ArrowDown'))) {
      event.preventDefault(); event.stopImmediatePropagation();
    }
  };
  const apply = (value: boolean | undefined) => {
    if (value) node.setAttribute('aria-disabled', 'true'); else node.removeAttribute('aria-disabled');
    if (node.matches(TEXT_ENTRY)) (node as HTMLInputElement).readOnly = !!value;
  };
  node.addEventListener('click', stop, true);
  node.addEventListener('keydown', keys, true);
  node.addEventListener('beforeinput', stop, true);
  apply(on);
  return {
    update: apply,
    destroy() {
      node.removeEventListener('click', stop, true);
      node.removeEventListener('keydown', keys, true);
      node.removeEventListener('beforeinput', stop, true);
    },
  };
}

/** One Tab stop for a row of buttons: arrows move between them, Enter or
 *  Space acts. Moving never activates, so a preset or a status is only
 *  chosen deliberately. The stop is the pressed or selected item. */
export function roving(node: HTMLElement, selector = ':scope > button, :scope > a[href]') {
  const items = () => [...node.querySelectorAll<HTMLElement>(selector)].filter((el) => el.getClientRects().length > 0);
  const current = (list: HTMLElement[]) =>
    list.find((el) => el === document.activeElement) ??
    list.find((el) => el.matches('[aria-pressed="true"], [aria-checked="true"], [aria-selected="true"], [aria-current="true"], [aria-current="page"]')) ??
    list[0];
  const sync = () => {
    const list = items();
    const stop = current(list);
    for (const el of list) el.tabIndex = el === stop ? 0 : -1;
  };
  const key = (event: KeyboardEvent) => {
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    const list = items();
    const at = list.indexOf(event.target as HTMLElement);
    if (at < 0) return;
    const next = event.key === 'ArrowRight' || event.key === 'ArrowDown' ? (at + 1) % list.length
      : event.key === 'ArrowLeft' || event.key === 'ArrowUp' ? (at - 1 + list.length) % list.length
      : event.key === 'Home' ? 0 : event.key === 'End' ? list.length - 1 : -1;
    if (next < 0) return;
    event.preventDefault();
    for (const el of list) el.tabIndex = -1;
    list[next].tabIndex = 0;
    list[next].focus();
  };
  const observer = new MutationObserver(sync);
  observer.observe(node, { subtree: true, childList: true, attributes: true, attributeFilter: ['aria-pressed', 'aria-checked', 'aria-selected', 'aria-current', 'hidden', 'class'] });
  node.addEventListener('keydown', key);
  node.addEventListener('focusin', sync);
  sync();
  return {
    destroy() {
      observer.disconnect();
      node.removeEventListener('keydown', key);
      node.removeEventListener('focusin', sync);
      for (const el of items()) el.removeAttribute('tabindex');
    },
  };
}

/** Closes a popover when focus leaves both it and its trigger, so a menu
 *  tabbed away from does not stay open and hold the shortcuts. */
export function closeOnFocusOut(node: HTMLElement, options: { trigger: () => HTMLElement | null | undefined; close: () => void }) {
  let current = options;
  const out = (event: FocusEvent) => {
    const to = event.relatedTarget as Node | null;
    // Focus moving to nothing (a click on plain page, a disabled control) is
    // handled by the popover's own outside-click rule.
    if (!to || node.contains(to) || current.trigger()?.contains(to)) return;
    current.close();
  };
  node.addEventListener('focusout', out);
  return {
    update(next: typeof options) { current = next; },
    destroy() { node.removeEventListener('focusout', out); },
  };
}

/** Focuses the first match once the DOM has caught up; for actions that
 *  remove the control the user just pressed. */
export function focusLater(find: () => HTMLElement | null | undefined): void {
  requestAnimationFrame(() => {
    const target = find();
    if (target && target.isConnected) target.focus({ preventScroll: false });
  });
}

/** Moves focus to the current view: its heading, or the first control in
 *  sight when a view kept its scroll and the heading is above the fold
 *  (focus there was invisible and the next Tab jumped back to the top). */
export function focusView(): void {
  const main = document.querySelector<HTMLElement>('main.workspace, main.landing');
  if (!main) return;
  const shown = (el: HTMLElement) => el.getClientRects().length > 0;
  const heading = [...main.querySelectorAll<HTMLElement>('h2')].find(shown);
  const frame = main.getBoundingClientRect();
  const inSight = (el: HTMLElement) => { const box = el.getBoundingClientRect(); return box.bottom > frame.top && box.top < frame.bottom; };
  if (heading && !inSight(heading)) {
    const control = [...main.querySelectorAll<HTMLElement>('button:not(:disabled), a[href], input:not(:disabled), [tabindex="0"]')].find((el) => shown(el) && inSight(el));
    if (control) { control.focus({ preventScroll: true }); return; }
  }
  const target = heading ?? main;
  target.tabIndex = -1;
  target.focus({ preventScroll: true });
}
