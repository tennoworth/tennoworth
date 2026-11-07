// The status menu changes what buyers see on warframe.market, so moving
// through its choices must never send one; only a deliberate key or click does.
import { expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/svelte';
import PresenceMenu from './PresenceMenu.svelte';
import { PresenceController } from './presence.svelte';
import type { PresenceStatus } from '../../contracts/desktop';

const BASE: PresenceStatus = {
  signedIn: true, connected: true, status: 'online', statusUntil: null, statusSetAt: '2026-10-09T12:02:00Z',
  managed: false, following: false, followPaused: false, gameRunning: true, problem: null, detail: null,
  settings: { followGame: false, whenClosed: 'invisible', keepForMinutes: null },
};

it('arrows move between statuses without sending one; Enter sends the focused one', async () => {
  const setPresence = vi.fn(async (status: string) => ({ ...BASE, status }) as PresenceStatus);
  const presence = new PresenceController({
    native: { presenceStatus: vi.fn(async () => BASE), setPresence, updatePresenceSettings: vi.fn(), followGameNow: vi.fn() } as never,
    listen: (() => () => {}) as never,
  });
  presence.start();
  render(PresenceMenu, { props: { presence } });
  await fireEvent.click(await screen.findByRole('button', { expanded: false }));
  const radios = screen.getAllByRole('radio') as HTMLInputElement[];
  const at = radios.findIndex((radio) => radio.checked);
  radios[at].focus();

  await fireEvent.keyDown(radios[at], { key: 'ArrowDown' });
  const next = radios[(at + 1) % radios.length];
  expect(document.activeElement).toBe(next);
  await fireEvent.keyDown(next, { key: 'ArrowDown' });
  expect(setPresence).not.toHaveBeenCalled();

  const focused = document.activeElement as HTMLInputElement;
  await fireEvent.keyDown(focused, { key: 'Enter' });
  await vi.waitFor(() => expect(setPresence).toHaveBeenCalledWith(focused.value));
});
