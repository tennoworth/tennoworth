// The app's language at runtime. English is built in; another language's
// catalog loads before the app mounts, so t() stays synchronous. A key a
// catalog lacks, or has not translated, falls back to English.
import { EN, type MessageKey } from './en';
import { formatMessage, parseMessage, type MessageArgs } from './message';
import { parsePo } from './po';
import { isLanguageCode, isOfferedLanguage, languageTag, type LanguageCode } from './languages';
import { pseudoLocalize } from './pseudo';

export type AppLanguage = LanguageCode | 'pseudo';

const catalogs = import.meta.glob<string>('./locales/*.po', { query: '?raw', import: 'default' });

class I18n {
  language = $state<AppLanguage>('en');
  private messages = $state.raw<Partial<Record<MessageKey, string>>>({});
  private parsed = new Map<string, ReturnType<typeof parseMessage>>();

  get tag() { return languageTag(this.language); }

  async use(language: AppLanguage): Promise<void> {
    let messages: Partial<Record<MessageKey, string>> = {};
    if (language === 'pseudo') {
      messages = Object.fromEntries(Object.entries(EN).map(([key, source]) => [key, pseudoLocalize(source.text)]));
    } else if (language !== 'en') {
      const load = catalogs[`./locales/${language}.po`];
      if (!load) throw new Error(`No catalog for ${language}`);
      for (const entry of parsePo(await load()).entries) {
        if (entry.key in EN && entry.translation) messages[entry.key as MessageKey] = entry.translation;
      }
    }
    this.parsed.clear();
    this.messages = messages;
    this.language = language;
  }

  t = (key: MessageKey, args: MessageArgs = {}): string => {
    const own = this.messages[key];
    const source = own ?? EN[key].text;
    let nodes = this.parsed.get(source);
    if (!nodes) {
      try { nodes = parseMessage(source); } catch { nodes = parseMessage(EN[key].text); }
      this.parsed.set(source, nodes);
    }
    return formatMessage(nodes, args, own ? this.tag : 'en');
  };
}

export const i18n = new I18n();
export const t = i18n.t;

/** The locale for dates and numbers: the system's own while the app is in
 *  English, so an English app on a German system keeps German dates. */
export function formatLocale(): string | undefined {
  return i18n.language === 'en' ? undefined : i18n.tag;
}

/** Read the saved language; one that is unknown or not offered means English. */
export function savedLanguage(value: string | null | undefined): AppLanguage {
  return isOfferedLanguage(value) ? value : 'en';
}

/** A language to preview: any catalog, offered or not, or the pseudo-locale. */
export function previewLanguage(value: string | null | undefined): AppLanguage {
  return value === 'pseudo' || isLanguageCode(value) ? value : 'en';
}
