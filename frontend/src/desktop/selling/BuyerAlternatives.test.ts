import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { cleanup, render, screen } from '@testing-library/svelte';
import { DESKTOP_CONTEXT } from '../../contracts/services';
import BuyerAlternatives from './BuyerAlternatives.svelte';

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(Date.parse('2026-10-14T12:00:00Z')); });
afterEach(() => { cleanup(); vi.useRealTimers(); });

const quote = (observedAt: string) => ({
  slug: 'part', rank: 0, subtype: null, error: null,
  buyer_book: {
    observed_at: observedAt,
    own_orders_excluded: true,
    orders: [{ id: 'o1', user_id: 'u1', status: 'ingame', quantity: 3, per_trade: 1, platinum: 9 }],
  },
});

it('turns a buyer check stale at the 60s mark without ticking every second', async () => {
  const desktopLiveTopPrices = vi.fn().mockResolvedValue([quote(new Date().toISOString())]);
  render(BuyerAlternatives, {
    props: { slug: 'part', name: 'Part', quantity: 2, ask: 10, onclose: () => {} },
    context: new Map([[DESKTOP_CONTEXT, { desktopLiveTopPrices }]]),
  });
  await vi.advanceTimersByTimeAsync(0);
  expect(screen.getByText(/2 of 2 units covered/)).toBeTruthy();
  // Only the freshness deadline is scheduled; nothing repeats.
  expect(vi.getTimerCount()).toBe(1);
  await vi.advanceTimersByTimeAsync(59_999);
  expect(screen.queryByText(/stale/)).toBeNull();
  await vi.advanceTimersByTimeAsync(1);
  expect(screen.getByText(/This buyer check is stale/)).toBeTruthy();
  expect(vi.getTimerCount()).toBe(0);
});
