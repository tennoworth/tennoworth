// "Show in game" is offered only where Rust allows it, and copying a whisper
// only makes sense once some category shows in game.
import { describe, it, expect, vi, afterEach } from 'vitest';
import { screen, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
import { renderDesktop as render } from '../../dev/preview/render-desktop';
import NotificationSettings from './NotificationSettings.svelte';
import { installTauri, removeTauri } from '../../dev/testing/test-utils';
import { IN_GAME_CATEGORIES, NOTIFICATION_CATEGORIES, type NotificationPreferences } from '../../contracts/desktop';

afterEach(() => { cleanup(); removeTauri(); vi.restoreAllMocks(); });

function preferences(): NotificationPreferences {
  return { popups: true, copy_whisper: false, categories: Object.fromEntries(NOTIFICATION_CATEGORIES.map(k => [k, { enabled: true, native: true, in_game: false }])) as NotificationPreferences['categories'] };
}

describe('NotificationSettings in game', () => {
  it('offers In game only for the categories Rust allows, and saves the choice', async () => {
    let stored = preferences();
    const saved: NotificationPreferences[] = [];
    installTauri(vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === 'get_notification_preferences') return structuredClone(stored);
      if (cmd === 'set_notification_preferences') { stored = JSON.parse(JSON.stringify(args!.preferences)) as NotificationPreferences; saved.push(stored); return structuredClone(stored); }
      throw new Error(`unexpected ${cmd}`);
    }));
    render(NotificationSettings, { props: {} });
    const boxes = await screen.findAllByRole('checkbox', { name: / in game$/ });
    expect(boxes).toHaveLength(IN_GAME_CATEGORIES.length);
    const copy = screen.getByRole('checkbox', { name: /^Copy the whisper in game/ });
    expect((copy as HTMLInputElement).disabled).toBe(true);
    await fireEvent.click(screen.getByRole('checkbox', { name: 'Price watches in game' }));
    await waitFor(() => expect(saved.at(-1)?.categories.watches.in_game).toBe(true));
    await waitFor(() => expect((copy as HTMLInputElement).disabled).toBe(false));
    await fireEvent.click(copy);
    await waitFor(() => expect(saved.at(-1)?.copy_whisper).toBe(true));
  });

  it('pausing popups also pauses the in-game card', async () => {
    const paused = { ...preferences(), popups: false };
    installTauri(vi.fn(async (cmd: string) => {
      if (cmd === 'get_notification_preferences') return paused;
      throw new Error(`unexpected ${cmd}`);
    }));
    render(NotificationSettings, { props: {} });
    const box = await screen.findByRole('checkbox', { name: 'Price watches in game' });
    expect((box as HTMLInputElement).disabled).toBe(true);
  });
});
