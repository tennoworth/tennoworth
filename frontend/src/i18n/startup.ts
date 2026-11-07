// The desktop loads its language before mounting, so nothing renders in
// English first. The hosted site stays English.
import type { SettingsStore } from '../contracts/state-store';
import { i18n, savedLanguage } from './i18n.svelte';

export async function startLanguage(store: SettingsStore): Promise<void> {
  // Dev-only override for reviewing a language in the preview: `?lang=fr`, `?lang=pseudo`.
  const override = import.meta.env.DEV ? new URLSearchParams(location.search).get('lang') : null;
  const language = savedLanguage(override ?? store.getSetting('app-language'));
  try {
    await i18n.use(language);
  } catch (error) {
    // A catalog that fails to load leaves the app in English rather than blank.
    console.error(error);
  }
  document.documentElement.lang = i18n.tag;
}
