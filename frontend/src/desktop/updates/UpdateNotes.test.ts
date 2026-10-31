import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import UpdateNotes from './UpdateNotes.svelte';
import fixture from '../../../../tests/fixtures/update-notes/status.json';
import type { UpdateNotesServices, UpdateNotesStatus } from '../../contracts/update';
function services(overrides: Partial<UpdateNotesServices> = {}): UpdateNotesServices {
  return { updateNotes: vi.fn(async () => structuredClone(fixture) as UpdateNotesStatus), acknowledgeUpdateNotes: vi.fn(async () => {}), updateNotesCanPresent: vi.fn(async () => true), ...overrides };
}
beforeEach(() => {
  vi.spyOn(document, 'hasFocus').mockReturnValue(true);
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value() { this.setAttribute('open', ''); } });
  Object.defineProperty(HTMLDialogElement.prototype, 'close', { configurable: true, value() { this.removeAttribute('open'); } });
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); });
describe('installed update notes', () => {
  it('shows grouped notes and all intermediate releases without acknowledging on read', async () => {
    const api = services(); render(UpdateNotes, { services: api, ready: true });
    await screen.findByRole('heading', { name: 'What’s new' });
    await waitFor(() => expect(document.querySelector('dialog')?.open).toBe(true));
    expect(api.acknowledgeUpdateNotes).not.toHaveBeenCalled();
    expect(screen.getByText('3 releases included')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Show all changes (7)' }));
    expect(screen.getByRole('heading', { name: /Links open your browser again/ })).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Got it' }));
    expect(api.acknowledgeUpdateNotes).toHaveBeenCalledWith('0.7.103');
    expect(document.querySelector('dialog')?.open).toBe(false);
  });
  it('tells the prompts when automatic notes are waiting, absent or dismissed', async () => {
    const onautomatic = vi.fn();
    render(UpdateNotes, { services: services(), ready: true, onautomatic });
    await waitFor(() => expect(document.querySelector('dialog')?.open).toBe(true));
    expect(onautomatic.mock.calls).toEqual([['pending']]);
    await fireEvent.click(screen.getByRole('button', { name: 'Got it' }));
    expect(onautomatic.mock.calls).toEqual([['pending'], ['dismissed']]);
    cleanup();
    const quiet = vi.fn();
    render(UpdateNotes, { services: services({ updateNotes: vi.fn(async () => ({ ...structuredClone(fixture), auto_show: false }) as UpdateNotesStatus) }), ready: true, onautomatic: quiet });
    await waitFor(() => expect(quiet).toHaveBeenCalledWith('none'));
    cleanup();
    const failed = vi.fn();
    render(UpdateNotes, { services: services({ updateNotes: vi.fn(async () => { throw new Error(); }) }), ready: true, onautomatic: failed });
    await waitFor(() => expect(failed).toHaveBeenCalledWith('none'));
  });
  it('defers when the app is not ready, has recovery work, or its native window is unfocused', async () => {
    const api=services({ updateNotesCanPresent: vi.fn(async () => false) });
    const { rerender }=render(UpdateNotes, { services: api, ready: false });
    await waitFor(() => expect(api.updateNotes).toHaveBeenCalled());
    expect(document.querySelector('dialog')?.open).toBe(false);
    await rerender({ services: api, ready:true, blocked:true });
    expect(document.querySelector('dialog')?.open).toBe(false);
    await rerender({ services:api, ready:true, blocked:false });
    await fireEvent(window,new Event('focus'));
    expect(document.querySelector('dialog')?.open).toBe(false);
  });
  it('opens an explicit Settings request when window focus reporting lags', async () => {
    vi.mocked(document.hasFocus).mockReturnValue(false);
    const canPresent=vi.fn(async () => false);
    const api=services({ updateNotesCanPresent: canPresent });
    const { component }=render(UpdateNotes,{services:api,ready:false});
    await waitFor(() => expect(api.updateNotes).toHaveBeenCalled());
    expect(document.querySelector('dialog')?.open).toBe(false);
    await component.open();
    await waitFor(() => expect(document.querySelector('dialog')?.open).toBe(true));
    expect(canPresent).not.toHaveBeenCalled();
  });
  it('closes on persistence failure and offers a retry without reopening automatically', async () => {
    const save=vi.fn().mockRejectedValueOnce(new Error('disk full')).mockResolvedValue(undefined);
    render(UpdateNotes,{ services:services({ acknowledgeUpdateNotes:save }),ready:true });
    await waitFor(() => expect(document.querySelector('dialog')?.open).toBe(true));
    await fireEvent.click(screen.getByRole('button',{name:'Got it'}));
    await screen.findByText(/Could not remember/);
    expect(document.querySelector('dialog')?.open).toBe(false);
    await fireEvent.click(screen.getByRole('button',{name:'Retry saving'}));
    await waitFor(() => expect(screen.queryByText(/Could not remember/)).toBeNull());
  });
  it('keeps action notices visible even beyond the ordinary highlight limit', async () => {
    const data=structuredClone(fixture) as UpdateNotesStatus;
    data.changes.push({id:'important',kind:'action',title:'Review your saved plan',body:'Open the plan before your next sale.',platforms:['linux','windows'],supersedes:[]});
    render(UpdateNotes,{services:services({updateNotes:async()=>data}),ready:true});
    await screen.findByRole('heading',{name:'Review your saved plan'});
    expect(screen.getByRole('heading',{name:'Action needed'})).toBeTruthy();
  });
  it('ignores an old startup read after manually opening and acknowledging the notes', async () => {
    let resolveInitial!: (value: UpdateNotesStatus) => void;
    const read = vi.fn().mockImplementationOnce(() => new Promise<UpdateNotesStatus>(resolve => { resolveInitial = resolve; }))
      .mockResolvedValue(structuredClone(fixture));
    const { component } = render(UpdateNotes, { services: services({ updateNotes: read }), ready: true });
    await component.open();
    await waitFor(() => expect(document.querySelector('dialog')?.open).toBe(true));
    await fireEvent.click(screen.getByRole('button', { name: 'Got it' }));
    resolveInitial(structuredClone(fixture) as UpdateNotesStatus);
    await Promise.resolve();
    await fireEvent(window, new Event('focus'));
    expect(document.querySelector('dialog')?.open).toBe(false);
  });

});


it('does not keep checking dialogs after automatic notes have been dismissed', async () => {
  vi.useFakeTimers();
  try {
    const query = vi.spyOn(document, 'querySelectorAll');
    const api = services({ updateNotes: vi.fn(async () => ({ ...structuredClone(fixture), auto_show: false }) as UpdateNotesStatus) });
    render(UpdateNotes, { services: api, ready: true });
    await Promise.resolve();
    await vi.advanceTimersByTimeAsync(3000);
    expect(query.mock.calls.filter(([selector]) => selector === 'dialog[open], [role="dialog"], [aria-modal="true"]')).toHaveLength(0);
  } finally { cleanup(); vi.useRealTimers(); }
});

it('retries an explicit notes request after another dialog closes', async () => {
  vi.useFakeTimers();
  const blocker = document.createElement('div');
  blocker.setAttribute('role', 'dialog');
  blocker.getClientRects = () => [{ width: 10 }] as unknown as DOMRectList;
  document.body.append(blocker);
  try {
    const api = services({ updateNotes: async () => ({ ...structuredClone(fixture), auto_show: false }) as UpdateNotesStatus });
    const { component } = render(UpdateNotes, { services: api, ready: true });
    await component.open();
    expect(document.querySelector('dialog')?.open).toBe(false);
    blocker.remove();
    await vi.advanceTimersByTimeAsync(800);
    expect(document.querySelector('dialog')?.open).toBe(true);
    await fireEvent.click(screen.getByRole('button', { name: 'Got it' }));
    const query = vi.spyOn(document, 'querySelectorAll');
    await vi.advanceTimersByTimeAsync(3000);
    expect(query.mock.calls.filter(([selector]) => selector === 'dialog[open], [role="dialog"], [aria-modal="true"]')).toHaveLength(0);
  } finally { blocker.remove(); cleanup(); vi.useRealTimers(); }
});
