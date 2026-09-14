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

const NOW = Date.parse('2026-09-14T12:00:00Z');

function renderMonthly(state: StateStore = store()) {
  render(RoutinesPanel, { props: { routine: new RoutineController(state, NOW), market: null, owned: new Map(), now: NOW } });
  return fireEvent.click(screen.getByRole('button', { name: 'Monthly' }));
}

async function addGoals(...texts: string[]) {
  const add = screen.getByLabelText('Add a monthly goal');
  for (const text of texts) {
    await fireEvent.input(add, { target: { value: text } });
    await fireEvent.click(screen.getByRole('button', { name: 'Add goal' }));
  }
}

function goalTitles(): (string | null)[] {
  return [...document.querySelectorAll('.checklist-items .task-copy strong')].map(node => node.textContent);
}

describe('RoutinesPanel', () => {
  it('shows selectable daily tasks by default and persists completion', async () => {
    const state = store();
    render(RoutinesPanel, { props: { routine: new RoutineController(state, NOW), market: null, owned: new Map(), now: NOW } });
    const tribute = screen.getByRole('checkbox', { name: /Claim the Daily Tribute/ }) as HTMLInputElement;
    expect(tribute.checked).toBe(false);
    expect(screen.getByText('0 of 5 complete')).toBeTruthy();
    await fireEvent.click(tribute);
    await waitFor(() => expect(screen.getByText('1 of 5 complete')).toBeTruthy());
    expect(state.getSetting('routine-checklist')).toContain('login-tribute');
  });

  it('offers personal monthly goals without inventing a game reset', async () => {
    await renderMonthly();
    expect(screen.getByText(/not a Warframe reset schedule/)).toBeTruthy();
    expect(screen.getByText(/Add a monthly goal to build/)).toBeTruthy();
    await addGoals('Prepare three ranked mods');
    expect(await screen.findByRole('checkbox', { name: /Prepare three ranked mods/ })).toBeTruthy();
    expect((screen.getByLabelText('Add a monthly goal') as HTMLInputElement).value).toBe('');
  });

  it('ticks goals independently and renames one without touching the other', async () => {
    await renderMonthly();
    await addGoals('List ranked mods', 'Prepare Prime sets');

    const first = screen.getByRole('checkbox', { name: /List ranked mods/ }) as HTMLInputElement;
    const second = screen.getByRole('checkbox', { name: /Prepare Prime sets/ }) as HTMLInputElement;
    await fireEvent.click(first);
    await waitFor(() => expect(screen.getByText('1 of 2 complete')).toBeTruthy());
    expect(second.checked).toBe(false);

    await fireEvent.click(screen.getByRole('button', { name: 'Edit List ranked mods' }));
    await fireEvent.input(screen.getByLabelText('Goal text'), { target: { value: 'List four ranked mods' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Save goal' }));
    await waitFor(() => expect(screen.getByRole('checkbox', { name: /List four ranked mods/ })).toBeTruthy());
    expect(screen.queryByRole('checkbox', { name: /List ranked mods/ })).toBeNull();
    expect(screen.getByText('0 of 2 complete')).toBeTruthy();
  });

  it('abandons an edit on cancel', async () => {
    await renderMonthly();
    await addGoals('List ranked mods');
    await fireEvent.click(screen.getByRole('button', { name: 'Edit List ranked mods' }));
    await fireEvent.input(screen.getByLabelText('Goal text'), { target: { value: 'Discarded' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.getByRole('checkbox', { name: /List ranked mods/ })).toBeTruthy();
    expect(screen.queryByText('Discarded')).toBeNull();
  });

  it('removes only the chosen goal and shows the empty state when the last one goes', async () => {
    const state = store();
    await renderMonthly(state);
    await addGoals('List ranked mods', 'Prepare Prime sets');

    await fireEvent.click(screen.getByRole('button', { name: 'Remove Prepare Prime sets' }));
    await waitFor(() => expect(screen.queryByRole('checkbox', { name: /Prepare Prime sets/ })).toBeNull());
    expect(screen.getByRole('checkbox', { name: /List ranked mods/ })).toBeTruthy();

    await fireEvent.click(screen.getByRole('button', { name: 'Remove List ranked mods' }));
    await waitFor(() => expect(screen.getByText(/Add a monthly goal to build/)).toBeTruthy());
    expect(JSON.parse(state.getSetting('routine-checklist')!).monthlyGoals).toEqual([]);
  });

  it('reorders goals with up and down controls and disables the ends', async () => {
    await renderMonthly();
    await addGoals('First goal', 'Second goal', 'Third goal');
    expect((screen.getByRole('button', { name: 'Move First goal up' }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: 'Move Third goal down' }) as HTMLButtonElement).disabled).toBe(true);

    await fireEvent.click(screen.getByRole('button', { name: 'Move Third goal up' }));
    await waitFor(() => expect(goalTitles()).toEqual(['First goal', 'Third goal', 'Second goal']));
    await fireEvent.click(screen.getByRole('button', { name: 'Move First goal down' }));
    await waitFor(() => expect(goalTitles()).toEqual(['Third goal', 'First goal', 'Second goal']));
  });

  it('keeps focus in the list after a remove', async () => {
    await renderMonthly();
    await addGoals('First goal', 'Second goal');

    await fireEvent.click(screen.getByRole('button', { name: 'Remove First goal' }));
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Remove Second goal' })));

    await fireEvent.click(screen.getByRole('button', { name: 'Remove Second goal' }));
    await waitFor(() => expect(document.activeElement).toBe(screen.getByLabelText('Add a monthly goal')));
  });

  it('returns focus to the row after an edit finishes', async () => {
    await renderMonthly();
    await addGoals('First goal', 'Second goal');

    await fireEvent.click(screen.getByRole('button', { name: 'Edit First goal' }));
    await fireEvent.input(screen.getByLabelText('Goal text'), { target: { value: 'Renamed goal' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Save goal' }));
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Edit Renamed goal' })));

    await fireEvent.click(screen.getByRole('button', { name: 'Edit Second goal' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Edit Second goal' })));
  });

  it('falls back to the other direction when a move disables the control it used', async () => {
    await renderMonthly();
    await addGoals('First goal', 'Second goal');

    await fireEvent.click(screen.getByRole('button', { name: 'Move Second goal up' }));
    await waitFor(() => expect(goalTitles()).toEqual(['Second goal', 'First goal']));
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Move Second goal down' })));
  });
});
