import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, screen, waitFor } from '@testing-library/svelte';
import { renderDesktop as render } from '../../dev/render-desktop';
import fixture from '../../../../tests/fixtures/relic-ocr/result.json';
import rewardPalette from '../../../../tests/fixtures/reward-palette.json';
import { readFileSync } from 'node:fs';
import { installTauri, removeTauri } from '../../dev/test-utils.js';
import RelicOverlay from './RelicOverlay.svelte';
import { RELIC_OVERLAY_HIDE_EVENT, RELIC_OVERLAY_UPDATE_EVENT } from '../../contracts/events';

afterEach(() => {
  cleanup();
  removeTauri();
});

describe('RelicOverlay', () => {
  it.each(['hide event', 'direct hide', 'update event', 'direct update'])('ignores the startup reply after a newer %s', async action => {
    let resolve!: (value: typeof fixture) => void;
    const pending = new Promise<typeof fixture>(yes => { resolve = yes; });
    const handlers: Record<string, (event: { payload: unknown }) => void> = {};
    installTauri(vi.fn(() => pending), vi.fn((name, handler) => {
      handlers[name] = handler;
      return Promise.resolve(() => {});
    }));
    render(RelicOverlay);
    await waitFor(() => expect(Object.keys(handlers)).toHaveLength(2));
    const newer = { ...fixture, slots: fixture.slots.map(slot => ({ ...slot, name: 'Current reward', livePlatinum: slot.livePlatinum ?? undefined, owned: slot.owned ?? undefined })) };
    if (action === 'hide event') handlers[RELIC_OVERLAY_HIDE_EVENT]({ payload: null });
    else if (action === 'direct hide') window.__TENNOWORTH_RELIC_OVERLAY_HIDE__?.();
    else if (action === 'update event') handlers[RELIC_OVERLAY_UPDATE_EVENT]({ payload: newer });
    else window.__TENNOWORTH_RELIC_OVERLAY_UPDATE__?.(newer);
    resolve(fixture);
    await new Promise(done => setTimeout(done, 0));
    expect(screen.queryByText('Paris Prime Blueprint')).toBeNull();
    expect(screen.queryAllByText('Current reward')).toHaveLength(action.includes('update') ? fixture.slots.length : 0);
  });

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

  it('renders a missed reward placeholder without any recommendation', async () => {
    const handlers: Record<string, (event: { payload: unknown }) => void> = {};
    installTauri(vi.fn(async () => null), vi.fn((name, handler) => {
      handlers[name] = handler;
      return Promise.resolve(() => {});
    }));
    render(RelicOverlay);
    const partial = {
      ...fixture,
      slots: fixture.slots.map((slot, index) => ({
        ...slot,
        ...(index === 1 ? { name: undefined, rawText: 'Reward not recognized', confidence: 0 } : {}),
        bestPlatinum: false,
        bestDucats: false,
      })),
    };

    handlers['relic-overlay:update']({ payload: partial });

    expect(await screen.findByText('Reward not recognized')).toBeTruthy();
    expect(screen.queryByText('BEST PLAT')).toBeNull();
    expect(screen.queryByText('BEST DUCATS')).toBeNull();
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

// The Wayland card is drawn natively from the same palette; both sides read
// the fixture so a token change cannot leave the in-game card behind again.
it('the overlay surface tokens match the palette the native card draws', () => {
  const appCss = readFileSync('src/app.css', 'utf8');
  const block = /html\.relic-overlay-surface\s*\{([^}]*)\}/.exec(appCss)?.[1] ?? '';
  const tokens = Object.fromEntries(
    [...block.matchAll(/--reward-([\w-]+):\s*(#[\da-f]+)/gi)].map(([, name, value]) => [name, value.toLowerCase()]),
  );
  expect(tokens).toEqual(rewardPalette);
});
