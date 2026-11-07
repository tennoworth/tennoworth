// The desktop loads its language before mounting, so nothing renders in
// English first. The hosted site stays English.
import type { SettingsStore } from '../contracts/state-store';
import { i18n, previewLanguage, savedLanguage, SIDE_WINDOW_LANGUAGE } from './i18n.svelte';

export async function startLanguage(store: SettingsStore): Promise<void> {
  // Dev-only override for reviewing a language in the preview: `?lang=fr`, `?lang=pseudo`.
  const override = import.meta.env.DEV ? new URLSearchParams(location.search).get('lang') : null;
  const language = override === null ? savedLanguage(store.getSetting('app-language')) : previewLanguage(override);
  try {
    await i18n.use(language);
  } catch (error) {
    // A catalog that fails to load leaves the app in English rather than blank.
    console.error(error);
  }
  document.documentElement.lang = i18n.tag;
}

/** The overlay windows: the language the main window last loaded. */
export async function startSideWindowLanguage(): Promise<void> {
  let saved: string | null = null;
  try { saved = localStorage.getItem(SIDE_WINDOW_LANGUAGE); } catch { /* storage blocked */ }
  try {
    await i18n.use(savedLanguage(saved));
  } catch (error) {
    console.error(error);
  }
  document.documentElement.lang = i18n.tag;
}
