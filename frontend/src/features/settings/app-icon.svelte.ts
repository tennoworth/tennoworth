/**
 * The desktop App icon setting, seen from the webview.
 *
 * The user's choice is a webview setting because 'match' depends on the
 * resolved colour mode, which only the webview knows. Native receives the
 * resolved colour, applies it to the window and tray, and keeps it for the next
 * launch. The shell owns the instance so the panel and the theme follower share
 * one source of truth - the same arrangement as the automatic-scan controller.
 */
import type { SettingsStore } from '../../contracts/state-store';
import type { AppIconCapability, AppIconColour } from '../../contracts/desktop';
import type { Mode, ThemeController } from '../../ui/theme';
import { humanError } from '../../contracts/errors';

export type AppIconChoice = 'blue' | 'match' | 'ink' | 'rag';
export const APP_ICON_CHOICES: readonly AppIconChoice[] = ['blue', 'match', 'ink', 'rag'];

export function readAppIconChoice(store: SettingsStore): AppIconChoice {
  const value = store.getSetting('app-icon');
  return APP_ICON_CHOICES.find((choice) => choice === value) ?? 'blue';
}

/** Match colour mode draws the theme's own ink: ink on Light, rag on Dark. */
export function resolveAppIcon(choice: AppIconChoice, mode: Mode): AppIconColour {
  if (choice !== 'match') return choice;
  return mode === 'dark' ? 'rag' : 'ink';
}

export interface AppIconDeps {
  store: SettingsStore;
  theme: Pick<ThemeController, 'mode' | 'subscribe'>;
  native: AppIconCapability;
}

export class AppIconController {
  choice = $state<AppIconChoice>('blue');
  /** False where the desktop session cannot show a per-window icon. */
  windowIcon = $state(true);
  error = $state('');

  #applied: AppIconColour | null = null;
  #request = 0;

  constructor(private deps: AppIconDeps) {
    this.choice = readAppIconChoice(deps.store);
  }

  /** Applies the stored choice, then follows the colour mode. Returns the stop. */
  start(): () => void {
    void this.#apply();
    return this.deps.theme.subscribe(() => void this.#apply());
  }

  async select(next: AppIconChoice): Promise<void> {
    if (next === this.choice) return;
    this.choice = next;
    void this.deps.store.setSetting('app-icon', next);
    await this.#apply();
  }

  async #apply(): Promise<void> {
    const colour = resolveAppIcon(this.choice, this.deps.theme.mode);
    if (colour === this.#applied) return;
    // A slower earlier request must not overwrite the outcome of a later one.
    const request = ++this.#request;
    try {
      const outcome = await this.deps.native.setAppIcon(colour);
      if (request !== this.#request) return;
      this.#applied = colour;
      this.windowIcon = outcome.windowIcon;
      this.error = '';
    } catch (e) {
      if (request === this.#request) this.error = humanError(e);
    }
  }
}
