import type { SettingsStore } from '../contracts/state-store';

// A malformed value must not resurrect every prompt the user already declined
// with an exception, nor hide prompts they never saw: unreadable means "none".
export function dismissedPrompts(store: SettingsStore): Set<string> {
  try {
    const value: unknown = JSON.parse(store.getSetting('dismissed-prompts') ?? '[]');
    return new Set(Array.isArray(value) ? value.filter((id): id is string => typeof id === 'string') : []);
  } catch {
    return new Set();
  }
}

export async function dismissPrompt(store: SettingsStore, id: string): Promise<void> {
  const ids = dismissedPrompts(store);
  if (ids.has(id)) return;
  ids.add(id);
  await store.setSetting('dismissed-prompts', JSON.stringify([...ids].sort()));
}
