// The drawn dropdown must behave like the native select it replaces: the same
// name and value for assistive technology, the same keys, and nothing chosen
// until the user commits.
import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import Select from './Select.svelte';

afterEach(cleanup);

const options = [
  { value: 'sell', label: 'Lowest ask is ≤' },
  { value: 'buy', label: 'Highest bid is ≥' },
  { value: 'bulk', label: 'Bulk buyers' },
];

function setup(props: Partial<{ value: string; disabled: boolean }> = {}) {
  const onchange = vi.fn();
  const view = render(Select, { props: { label: 'Notify when', value: 'sell', options, onchange, ...props } });
  const box = screen.getByRole('combobox', { name: 'Notify when' });
  return { onchange, box, view };
}

const list = () => screen.queryByRole('listbox');

it('shows the chosen option and opens a list that marks it', async () => {
  const { box } = setup({ value: 'buy' });
  expect(box.textContent).toContain('Highest bid is ≥');
  expect(box.getAttribute('aria-expanded')).toBe('false');
  expect(list()).toBeNull();
  await fireEvent.click(box);
  expect(box.getAttribute('aria-expanded')).toBe('true');
  const chosen = screen.getByRole('option', { selected: true });
  expect(chosen.textContent).toBe('Highest bid is ≥');
  expect(box.getAttribute('aria-activedescendant')).toBe(chosen.id);
});

it('picks with a click and reports only a real change', async () => {
  const { box, onchange } = setup();
  await fireEvent.click(box);
  await fireEvent.click(screen.getByRole('option', { name: 'Lowest ask is ≤' }));
  expect(onchange).not.toHaveBeenCalled();
  expect(list()).toBeNull();
  await fireEvent.click(box);
  await fireEvent.click(screen.getByRole('option', { name: 'Bulk buyers' }));
  expect(onchange).toHaveBeenCalledExactlyOnceWith('bulk');
  expect(document.activeElement).toBe(box);
});

it('moves with the arrow keys and commits with Enter', async () => {
  const { box, onchange } = setup();
  box.focus();
  await fireEvent.keyDown(box, { key: 'ArrowDown' });
  expect(list()).not.toBeNull();
  await fireEvent.keyDown(box, { key: 'ArrowDown' });
  await fireEvent.keyDown(box, { key: 'ArrowDown' });
  await fireEvent.keyDown(box, { key: 'ArrowDown' });
  await fireEvent.keyDown(box, { key: 'Enter' });
  expect(onchange).toHaveBeenCalledExactlyOnceWith('bulk');
  expect(list()).toBeNull();
});

it('Escape closes without choosing, and Home and End jump to the ends', async () => {
  const { box, onchange } = setup({ value: 'buy' });
  await fireEvent.keyDown(box, { key: 'End' });
  expect(box.getAttribute('aria-activedescendant')).toBe(screen.getByRole('option', { name: 'Bulk buyers' }).id);
  await fireEvent.keyDown(box, { key: 'Home' });
  expect(box.getAttribute('aria-activedescendant')).toBe(screen.getByRole('option', { name: 'Lowest ask is ≤' }).id);
  await fireEvent.keyDown(box, { key: 'Escape' });
  expect(list()).toBeNull();
  expect(onchange).not.toHaveBeenCalled();
});

it('Tab commits the highlighted option and lets focus move on', async () => {
  const { box, onchange } = setup();
  await fireEvent.keyDown(box, { key: ' ' });
  await fireEvent.keyDown(box, { key: 'ArrowDown' });
  const tab = await fireEvent.keyDown(box, { key: 'Tab' });
  expect(tab).toBe(true);
  expect(onchange).toHaveBeenCalledExactlyOnceWith('buy');
});

it('jumps to an option by its first letters', async () => {
  const { box, onchange } = setup();
  await fireEvent.keyDown(box, { key: 'b' });
  expect(box.getAttribute('aria-activedescendant')).toBe(screen.getByRole('option', { name: 'Bulk buyers' }).id);
  await fireEvent.keyDown(box, { key: 'Enter' });
  expect(onchange).toHaveBeenCalledExactlyOnceWith('bulk');
});

it('closes on a click elsewhere without choosing', async () => {
  const { box, onchange } = setup();
  await fireEvent.click(box);
  await fireEvent.pointerDown(document.body);
  expect(list()).toBeNull();
  expect(onchange).not.toHaveBeenCalled();
});

it('stays shut while disabled', async () => {
  const { box } = setup({ disabled: true });
  expect((box as HTMLButtonElement).disabled).toBe(true);
  await fireEvent.click(box);
  await fireEvent.keyDown(box, { key: 'ArrowDown' });
  expect(list()).toBeNull();
});

it('follows the value it is given, so a refused save shows the old choice again', async () => {
  const { box, view } = setup();
  await view.rerender({ value: 'buy' });
  expect(box.textContent).toContain('Highest bid is ≥');
  await view.rerender({ value: 'sell' });
  expect(box.textContent).toContain('Lowest ask is ≤');
});
