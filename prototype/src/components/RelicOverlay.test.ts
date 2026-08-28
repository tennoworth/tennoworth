import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/svelte';
import fixture from '../../../tests/fixtures/relic-ocr/result.json';
import { installTauri, removeTauri } from '../lib/test-utils.js';
import RelicOverlay from './RelicOverlay.svelte';

afterEach(() => {
  cleanup();
  removeTauri();
});

describe('RelicOverlay', () => {
  it('unregisters both overlay listeners on unmount', async () => {
    const unlistenUpdate = vi.fn();
    const unlistenHide = vi.fn();
    const listen = vi.fn()
      .mockResolvedValueOnce(unlistenUpdate)
      .mockResolvedValueOnce(unlistenHide);
    installTauri(vi.fn(async () => null), listen);
    const overlay = render(RelicOverlay);
    await waitFor(() => expect(listen).toHaveBeenCalledTimes(2));
    overlay.unmount();
    await waitFor(() => {
      expect(unlistenUpdate).toHaveBeenCalledTimes(1);
      expect(unlistenHide).toHaveBeenCalledTimes(1);
    });
  });

  it('renders the shared result contract and does not recommend an uncertain expensive match', async () => {
    const handlers: Record<string, (event: { payload: unknown }) => void> = {};
    installTauri(vi.fn(async () => null), vi.fn((name, handler) => {
      handlers[name] = handler;
      return Promise.resolve(() => {});
    }));
    render(RelicOverlay);

    handlers['relic-overlay:update']({ payload: fixture });

    expect(await screen.findByText('Wisp Prime Systems Blueprint')).toBeTruthy();
    expect(screen.getByText('42p')).toBeTruthy();
    expect(screen.getByText('own 0')).toBeTruthy();
    expect(screen.getAllByText('BEST PLAT')).toHaveLength(1);
    expect(screen.getByText(/check name · 87%/)).toBeTruthy();
    expect(screen.getByText('180p').closest('article')?.textContent).not.toContain('BEST PLAT');
  });

  it('clears cards when the Rust window emits hide', async () => {
    const handlers: Record<string, (event: { payload: unknown }) => void> = {};
    installTauri(vi.fn(async () => null), vi.fn((name, handler) => {
      handlers[name] = handler;
      return Promise.resolve(() => {});
    }));
    render(RelicOverlay);
    handlers['relic-overlay:update']({ payload: fixture });
    await screen.findByText('Paris Prime Blueprint');
    handlers['relic-overlay:hide']({ payload: null });
    await waitFor(() => expect(screen.queryByText('Paris Prime Blueprint')).toBeNull());
  });
});
