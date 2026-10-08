import { describe, expect, it } from 'vitest';
import type { SettingsStore } from '../contracts/state-store';
import { dismissPrompt, dismissedPrompts } from './prompts';

function memory(initial: string | null): SettingsStore & { value: string | null } {
  return {
    value: initial, mode: 'local', hydrate: async () => {},
    getSetting() { return this.value; },
    async setSetting(_key, value) { this.value = value; },
  };
}

describe('dismissed prompts', () => {
  it('remembers each declined prompt alongside earlier ones', async () => {
    const store = memory(null);
    await dismissPrompt(store, 'b');
    await dismissPrompt(store, 'a');
    await dismissPrompt(store, 'a');
    expect(store.value).toBe('["a","b"]');
    expect(dismissedPrompts(store)).toEqual(new Set(['a', 'b']));
  });

  it('treats an unreadable value as nothing declined', () => {
    expect(dismissedPrompts(memory('{not json'))).toEqual(new Set());
    expect(dismissedPrompts(memory('{"a":1}'))).toEqual(new Set());
    expect(dismissedPrompts(memory('["a",3,null]'))).toEqual(new Set(['a']));
  });
});
