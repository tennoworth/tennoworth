import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import HostedShell from './HostedShell.svelte';
import { loadMarket, revalidateMarket, startHostedMarketRevalidation } from '../adapters/market';
import bundledMarket from '../../public/market.json';
import type { Market } from '../contracts/data';

vi.mock('../adapters/market', () => ({ loadMarket: vi.fn(), revalidateMarket: vi.fn(), startHostedMarketRevalidation: vi.fn(() => () => {}) }));

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

it('omits a relative age when the hosted snapshot timestamp is malformed', async () => {
  vi.stubGlobal('__APP_COMMIT__', 'fixture');
  vi.stubGlobal('matchMedia', vi.fn(() => ({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() })));
  vi.mocked(loadMarket).mockResolvedValue({ ...bundledMarket, updated_at: 'not-a-date' } as unknown as Market);
  const { container } = render(HostedShell, { theme: { modePref: 'dark', mode: 'dark', subscribe: () => () => {}, setModePref: vi.fn(), destroy: vi.fn() } });
  await screen.findByText(/^Snapshot/);
  expect(container.querySelector('.statusbar')?.textContent).not.toContain('NaN');
  expect(container.textContent).not.toContain('NaN d ago');
});

it('swaps in a newer snapshot from revalidation and keeps the search', async () => {
  vi.stubGlobal('__APP_COMMIT__', 'fixture');
  vi.stubGlobal('matchMedia', vi.fn(() => ({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() })));
  vi.mocked(loadMarket).mockResolvedValue({ ...bundledMarket, updated_at: '2026-10-13T10:00:00Z' } as unknown as Market);
  render(HostedShell, { theme: { modePref: 'dark', mode: 'dark', subscribe: () => () => {}, setModePref: vi.fn(), destroy: vi.fn() } });
  await screen.findByText('Snapshot 2026-10-13 10:00 UTC');
  const search = screen.getByLabelText('Search items') as HTMLInputElement;
  await fireEvent.input(search, { target: { value: 'primed' } });
  const revalidate = vi.mocked(startHostedMarketRevalidation).mock.calls.at(-1)?.[0];
  expect(revalidate).toBeTypeOf('function');
  vi.mocked(revalidateMarket).mockResolvedValue({ ...bundledMarket, updated_at: '2026-10-13T12:00:00Z' } as unknown as Market);
  await revalidate!();
  await screen.findByText('Snapshot 2026-10-13 12:00 UTC');
  expect(search.value).toBe('primed');
  expect(screen.getByLabelText('Search items')).toBe(search);
});
