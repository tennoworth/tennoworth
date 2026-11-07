import { afterEach, expect, it, vi } from 'vitest';
import { busy, closeOnFocusOut, roving } from './keyboard-actions';

afterEach(() => { document.body.innerHTML = ''; });

function shown(...elements: Element[]) {
  for (const el of elements) vi.spyOn(el, 'getClientRects').mockReturnValue([{}] as unknown as DOMRectList);
}

it('busy keeps a control focusable but ignores clicks, Enter and Space until it is done', () => {
  document.body.innerHTML = '<button>Save</button>';
  const button = document.querySelector('button')!;
  const clicked = vi.fn();
  button.addEventListener('click', clicked);
  const action = busy(button, true);
  button.focus();

  button.click();
  const enter = new KeyboardEvent('keydown', { key: 'Enter', cancelable: true });
  button.dispatchEvent(enter);
  expect(clicked).not.toHaveBeenCalled();
  expect(enter.defaultPrevented).toBe(true);
  expect(button.disabled).toBe(false);
  expect(button.getAttribute('aria-disabled')).toBe('true');
  expect(document.activeElement).toBe(button);

  action.update(false);
  button.click();
  expect(clicked).toHaveBeenCalledOnce();
  expect(button.hasAttribute('aria-disabled')).toBe(false);
});

it('busy makes a text field read-only rather than disabled', () => {
  document.body.innerHTML = '<input type="number">';
  const input = document.querySelector('input')!;
  const action = busy(input, true);
  expect(input.readOnly).toBe(true);
  expect(input.disabled).toBe(false);
  action.update(false);
  expect(input.readOnly).toBe(false);
});

it('roving gives a row one Tab stop and moves focus with arrows without pressing', () => {
  document.body.innerHTML = '<div><button>A</button><button aria-pressed="true">B</button><button>C</button></div>';
  const group = document.querySelector('div')!;
  const [a, b, c] = [...document.querySelectorAll('button')];
  shown(a, b, c);
  const pressed = vi.fn();
  for (const button of [a, b, c]) button.addEventListener('click', pressed);
  roving(group);
  expect([a.tabIndex, b.tabIndex, c.tabIndex]).toEqual([-1, 0, -1]);

  b.focus();
  b.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true, cancelable: true }));
  expect(document.activeElement).toBe(c);
  c.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true, cancelable: true }));
  expect(document.activeElement).toBe(a);
  a.dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true, cancelable: true }));
  expect(document.activeElement).toBe(c);
  expect([a.tabIndex, b.tabIndex, c.tabIndex]).toEqual([-1, -1, 0]);
  expect(pressed).not.toHaveBeenCalled();
});

it('closeOnFocusOut closes when focus leaves the popover and its trigger only', () => {
  document.body.innerHTML = '<button id="t">Menu</button><div id="pop"><button>Inside</button></div><button id="out">Elsewhere</button>';
  const trigger = document.getElementById('t')!;
  const pop = document.getElementById('pop')!;
  const close = vi.fn();
  closeOnFocusOut(pop, { trigger: () => trigger, close });
  const inside = pop.querySelector('button')!;
  inside.focus();
  trigger.focus();
  expect(close).not.toHaveBeenCalled();
  inside.focus();
  document.getElementById('out')!.focus();
  expect(close).toHaveBeenCalledOnce();
});
