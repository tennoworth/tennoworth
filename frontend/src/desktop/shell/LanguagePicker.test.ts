import { afterEach, beforeAll, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { i18n } from '../../i18n/i18n.svelte';
import LanguagePicker from './LanguagePicker.svelte';

beforeAll(() => {
  // jsdom has no modal dialogs.
  HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) { this.open = true; };
  HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) { this.open = false; };
});
afterEach(async () => { cleanup(); await i18n.use('en'); });

it('starts on the game’s language and shows the app in it', async () => {
  render(LanguagePicker, { props: { gameLanguage: 'de', save: vi.fn(async () => {}), ondone: vi.fn() } });
  const german = screen.getByRole('radio', { name: /Deutsch/ });
  expect(german.getAttribute('aria-checked')).toBe('true');
  expect(german.textContent).toContain('Your game');
  await waitFor(() => expect(i18n.language).toBe('de'));
});

it('offers every language but Thai, and both Portugueses', () => {
  render(LanguagePicker, { props: { gameLanguage: null, save: vi.fn(async () => {}), ondone: vi.fn() } });
  expect(screen.getAllByRole('radio')).toHaveLength(15);
  expect(screen.getByRole('radio', { name: /Português \(Portugal\)/ })).toBeTruthy();
  expect(screen.queryByRole('radio', { name: /ไทย/ })).toBeNull();
});

it('moves with the arrows and saves the language under the cursor', async () => {
  const save = vi.fn(async () => {});
  const ondone = vi.fn();
  render(LanguagePicker, { props: { gameLanguage: null, save, ondone } });
  const group = screen.getByRole('radiogroup');
  await fireEvent.keyDown(group, { key: 'ArrowRight' });
  expect(screen.getByRole('radio', { name: /Deutsch/ }).getAttribute('aria-checked')).toBe('true');
  await waitFor(() => expect(i18n.language).toBe('de'));
  await fireEvent.keyDown(group, { key: 'Enter' });
  await waitFor(() => expect(ondone).toHaveBeenCalled());
  expect(save).toHaveBeenCalledWith('de');
});

it('says when the choice could not be saved, then lets the player go on', async () => {
  const save = vi.fn(async () => { throw new Error('locked'); });
  const ondone = vi.fn();
  render(LanguagePicker, { props: { gameLanguage: null, save, ondone } });
  await fireEvent.click(screen.getByRole('button', { name: /Continue/ }));
  expect(await screen.findByRole('alert')).toBeTruthy();
  expect(ondone).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole('button', { name: /Continue/ }));
  await waitFor(() => expect(ondone).toHaveBeenCalled());
});
