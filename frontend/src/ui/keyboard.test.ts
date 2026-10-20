import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { characterKey, keyboard, readKeyboardPreferences, saveKeyboardPreferences, shortcutBlocked, watchKeyboardLayout } from './keyboard.svelte';
import type { SettingsStore } from '../contracts/state-store';

beforeEach(() => { readKeyboardPreferences(); document.body.innerHTML = ''; });
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it('uses typed characters across Shift and AltGr layouts without taking modifier commands or composition', () => {
  expect(characterKey(new KeyboardEvent('keydown', { key: '/', code: 'Digit7', shiftKey: true }))).toBe('/');
  const altGraph = new KeyboardEvent('keydown', { key: '/', ctrlKey: true, altKey: true });
  vi.spyOn(altGraph, 'getModifierState').mockImplementation(modifier => modifier === 'AltGraph');
  expect(characterKey(altGraph)).toBe('/');
  for (const options of [{ key: '/', ctrlKey: true }, { key: '/', altKey: true }, { key: '/', metaKey: true }, { key: '/', isComposing: true }, { key: 'Dead' }, { key: '/', repeat: true }]) {
    expect(characterKey(new KeyboardEvent('keydown', options))).toBeNull();
  }
});

it('validates stored choices and saves only intentional preferences', async () => {
  const write = vi.fn();
  const store = { getSetting: () => '{"search":"é","help":"¿","enabled":false,"navigation":"ж"}', setSetting: write } as unknown as SettingsStore;
  readKeyboardPreferences(store);
  expect([keyboard.search, keyboard.help, keyboard.enabled]).toEqual(['é', '¿', false]);
  keyboard.commandKey = 'Л'; keyboard.layoutAvailable = true;
  await saveKeyboardPreferences(store);
  expect(write).toHaveBeenCalledWith('keyboard', '{"search":"é","help":"¿","enabled":false,"navigation":"ж"}');
  for (const saved of ['{', 'null', '{"search":"x","help":"x","enabled":true}', '{"search":"Dead","help":"?","enabled":true}']) {
    readKeyboardPreferences({ getSetting: () => saved } as unknown as SettingsStore);
    expect(keyboard.search).toBe('/');
  }
});

it('leaves text entry, handled events and visible dialogs alone', () => {
  for (const markup of ['<input>', '<textarea></textarea>', '<select><option>A</option></select>', '<div contenteditable="true"><span>edit</span></div>']) {
    document.body.innerHTML = markup;
    const element = document.querySelector('span') ?? document.body.firstElementChild!;
    element.addEventListener('keydown', event => expect(shortcutBlocked(event as KeyboardEvent)).toBe(true));
    element.dispatchEvent(new KeyboardEvent('keydown', { key: '/' }));
  }
  const handled = new KeyboardEvent('keydown', { cancelable: true }); handled.preventDefault();
  expect(shortcutBlocked(handled)).toBe(true);
  document.body.innerHTML = '<div role="dialog"></div>';
  vi.spyOn(document.body.firstElementChild!, 'getClientRects').mockReturnValue([{}] as unknown as DOMRectList);
  expect(shortcutBlocked(new KeyboardEvent('keydown'))).toBe(true);
});

describe('optional local layout discovery', () => {
  it('updates the command label on layout changes and discards late results after disposal', async () => {
    const source = new EventTarget() as EventTarget & { getLayoutMap: () => Promise<Map<string, string>> };
    source.getLayoutMap = vi.fn(async () => new Map([['KeyK', 'л']]));
    vi.stubGlobal('navigator', { platform: 'Linux', keyboard: source });
    const stop = watchKeyboardLayout();
    await Promise.resolve();
    expect(keyboard.commandKey).toBe('Л');
    source.getLayoutMap = async () => new Map([['KeyK', 'k']]);
    source.dispatchEvent(new Event('layoutchange'));
    await Promise.resolve();
    await vi.waitFor(() => expect(keyboard.commandKey).toBe('K'));
    source.getLayoutMap = async () => new Map([['KeyK', 'x']]);
    source.dispatchEvent(new Event('layoutchange')); stop();
    await Promise.resolve();
    expect(keyboard.commandKey).toBe('K');
  });
  it('works when the API is missing, denied, or has no event-listener methods', async () => {
    for (const source of [undefined, { getLayoutMap: async () => { throw new Error('Denied'); } }, { getLayoutMap: async () => new Map([['KeyK', 'k']]) }]) {
      vi.stubGlobal('navigator', { platform: 'Linux', keyboard: source });
      const stop = watchKeyboardLayout(); await Promise.resolve(); stop();
      expect(keyboard.commandKey).toBe('K');
    }
  });
});
