import { afterEach, describe, expect, it } from 'vitest';
import { cleanup, fireEvent, screen, waitFor } from '@testing-library/svelte';
import { renderDesktop as render } from '../../dev/render-desktop';
import type { StateStore } from '../../contracts/state-store';
import RoutinesPanel from './RoutinesPanel.svelte';
import { RoutineController } from './controller.svelte';

afterEach(cleanup);

function store(): StateStore {
  const values = new Map<string, string>();
  return {
    mode: 'local', hydrate: async () => {},
    getSetting: key => values.get(key) ?? null,
    setSetting: async (key, value) => { values.set(key, value); },
    loadSnapshot: async () => null, saveSnapshot: async () => {}, clearSnapshot: async () => {},
  };
}

describe('RoutinesPanel', () => {
  it('shows selectable daily tasks by default and persists completion', async () => {
    const state = store();
    const now = Date.parse('2026-09-14T12:00:00Z');
    render(RoutinesPanel, { props: { routine: new RoutineController(state, now), market: null, owned: new Map(), now } });
    const tribute = screen.getByRole('checkbox', { name: /Claim the Daily Tribute/ }) as HTMLInputElement;
    expect(tribute.checked).toBe(false);
    expect(screen.getByText('0 of 5 complete')).toBeTruthy();
    await fireEvent.click(tribute);
    await waitFor(() => expect(screen.getByText('1 of 5 complete')).toBeTruthy());
    expect(state.getSetting('routine-checklist')).toContain('login-tribute');
  });

  it('offers a clearly personal monthly goal without inventing a game reset', async () => {
    const now = Date.parse('2026-09-14T12:00:00Z');
    render(RoutinesPanel, { props: { routine: new RoutineController(store(), now), market: null, owned: new Map(), now } });
    await fireEvent.click(screen.getByRole('button', { name: 'Monthly' }));
    expect(screen.getByText(/not a Warframe reset schedule/)).toBeTruthy();
    const goal = screen.getByLabelText('Personal monthly goal');
    await fireEvent.input(goal, { target: { value: 'Prepare three ranked mods' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Save goal' }));
    expect(await screen.findByRole('checkbox', { name: /Prepare three ranked mods/ })).toBeTruthy();
  });
});
