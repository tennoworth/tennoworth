import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import Pager from './Pager.svelte';
import { pageWindow } from './paging';

afterEach(cleanup);

it('stays out of the way when everything fits on one page', () => {
  const { container } = render(Pager, { props: { window: pageWindow(100, 0), total: 100, label: 'Trade history pages', onpage: () => {} } });
  expect(container.querySelector('nav')).toBeNull();
});

it('names the range on screen and moves one page at a time', async () => {
  const onpage = vi.fn();
  render(Pager, { props: { window: pageWindow(250, 0), total: 250, label: 'Trade history pages', onpage } });
  expect(screen.getByRole('navigation', { name: 'Trade history pages' }).textContent).toContain('Showing 1–100 of 250');
  expect(screen.getByText('Page 1 of 3')).toBeTruthy();
  expect((screen.getByRole('button', { name: '‹ Prev' }) as HTMLButtonElement).disabled).toBe(true);
  await fireEvent.click(screen.getByRole('button', { name: 'Next ›' }));
  expect(onpage).toHaveBeenCalledWith(1);
});

it('disables Next on the last page', () => {
  render(Pager, { props: { window: pageWindow(250, 2), total: 250, label: 'Riven pages', onpage: () => {} } });
  expect(screen.getByRole('navigation').textContent).toContain('Showing 201–250 of 250');
  expect((screen.getByRole('button', { name: 'Next ›' }) as HTMLButtonElement).disabled).toBe(true);
});
