import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, render, screen } from '@testing-library/svelte';
import HostedShell from './HostedShell.svelte';
import { loadMarket } from '../adapters/market';
import bundledMarket from '../../public/market.json';
import type { Market } from '../contracts/data';

vi.mock('../adapters/market', () => ({ loadMarket: vi.fn() }));

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
