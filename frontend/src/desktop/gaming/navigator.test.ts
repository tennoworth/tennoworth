import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { apply, recoverFocus, type NavigatorHandlers } from './navigator';

function handlers(): NavigatorHandlers {
  return { back: vi.fn(), prev: vi.fn(), next: vi.fn(), menu: vi.fn(), scan: vi.fn(), secondary: vi.fn() };
}

// jsdom lays nothing out; give every element a box so it counts as shown.
beforeEach(() => {
  Element.prototype.scrollIntoView ??= () => {};
  vi.spyOn(Element.prototype, 'getClientRects').mockReturnValue([new DOMRect(0, 0, 10, 10)] as unknown as DOMRectList);
});
afterEach(() => { document.body.innerHTML = ''; vi.restoreAllMocks(); });

describe('pad actions', () => {
  it('accept clicks the focused control', () => {
    document.body.innerHTML = '<main><button id="a">A</button></main>';
    const root = document.querySelector('main')!;
    const button = document.getElementById('a')!;
    const click = vi.fn();
    button.addEventListener('click', click);
    button.focus();
    apply(root, 'accept', handlers());
    expect(click).toHaveBeenCalledOnce();
  });

  it('section buttons reach the shell when no dialog is open', () => {
    document.body.innerHTML = '<main><button>A</button></main>';
    const h = handlers();
    const root = document.querySelector('main')!;
    for (const action of ['back', 'prev', 'next', 'menu', 'scan', 'secondary'] as const) apply(root, action, h);
    for (const fn of Object.values(h)) expect(fn).toHaveBeenCalledOnce();
  });

  it('an open dialog takes the buttons: B closes it and the shell hears nothing', () => {
    document.body.innerHTML = '<main><button>A</button></main><div role="dialog" aria-modal="true"><button id="ok">OK</button></div>';
    const h = handlers();
    const root = document.querySelector('main')!;
    const dialog = document.querySelector('[role=dialog]')!;
    const escape = vi.fn();
    dialog.addEventListener('keydown', (e) => { if ((e as KeyboardEvent).key === 'Escape') escape(); });
    document.getElementById('ok')!.focus();
    for (const action of ['back', 'prev', 'next', 'secondary'] as const) apply(root, action, h);
    expect(escape).toHaveBeenCalledOnce();
    for (const fn of Object.values(h)) expect(fn).not.toHaveBeenCalled();
  });

  it('focus that fell to the page returns to the control nearest where it was', () => {
    document.body.innerHTML = '<main><button id="top">Top</button><button id="mid" data-pad-default>Mid</button><button id="low">Low</button></main>';
    const root = document.querySelector('main')!;
    const at = (id: string, top: number) => vi.spyOn(document.getElementById(id)!, 'getBoundingClientRect').mockReturnValue(new DOMRect(0, top, 100, 40));
    at('top', 0); at('mid', 100); at('low', 200);
    (document.activeElement as HTMLElement | null)?.blur();
    expect(recoverFocus(root, null)).toBe(true);
    expect(document.activeElement?.id).toBe('mid');
    (document.activeElement as HTMLElement).blur();
    expect(recoverFocus(root, new DOMRect(0, 190, 100, 40))).toBe(true);
    expect(document.activeElement?.id).toBe('low');
    expect(recoverFocus(root, null)).toBe(false);
    expect(document.activeElement?.id).toBe('low');
  });

  it('left and right step a focused number field; up and down never change it', () => {
    document.body.innerHTML = '<main><input id="n" type="number" value="5" min="1" max="9"></main>';
    const input = document.getElementById('n') as HTMLInputElement;
    const main = document.querySelector('main')!;
    input.focus();
    apply(main, 'down', handlers());
    apply(main, 'up', handlers());
    expect(input.value).toBe('5');
    apply(main, 'right', handlers());
    expect(input.value).toBe('6');
    apply(main, 'left', handlers());
    expect(input.value).toBe('5');
  });
});
