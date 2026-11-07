import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { argumentNames, formatMessage, mapText, parseMessage, richParts } from './message';
import { formatPo, parsePo } from './po';
import { checkCatalogs, syncCatalog } from './catalog';
import { pseudoLocalize } from './pseudo';
import { i18n, previewLanguage, savedLanguage } from './i18n.svelte';
import { EN } from './en';
import { TRANSLATED } from './languages';

const COUNT = '{count, plural, =0 {nothing} one {# item} other {# items}} left';

describe('messages', () => {
  it('formats arguments, exact plurals and locale plural rules', () => {
    expect(formatMessage(COUNT, { count: 0 }, 'en')).toBe('nothing left');
    expect(formatMessage(COUNT, { count: 1 }, 'en')).toBe('1 item left');
    expect(formatMessage(COUNT, { count: 1200 }, 'en')).toBe('1,200 items left');
    const ru = '{n, plural, one {# предмет} few {# предмета} many {# предметов} other {# предмета}}';
    expect([1, 3, 5, 21].map(n => formatMessage(ru, { n }, 'ru'))).toEqual(['1 предмет', '3 предмета', '5 предметов', '21 предмет']);
    expect(formatMessage('{who, select, me {You} other {{who}}} sold it', { who: 'Ash' }, 'en')).toBe('Ash sold it');
  });

  it('keeps ordinary apostrophes and quotes only around syntax', () => {
    expect(formatMessage("l'inventaire de {name}", { name: 'Ash' }, 'fr')).toBe("l'inventaire de Ash");
    expect(formatMessage("Type '{name}' literally", {}, 'en')).toBe('Type {name} literally');
  });

  it('rejects broken syntax and reports argument names', () => {
    expect(() => parseMessage('{count, plural, one {#}}')).toThrow(/other/);
    expect(() => parseMessage('unclosed {name')).toThrow();
    expect(argumentNames(COUNT + ' {summary}')).toEqual(['count', 'summary']);
  });

  it('rewrites only literal text and splits emphasis', () => {
    expect(mapText(COUNT, text => text.toUpperCase())).toBe('{count, plural, =0 {NOTHING} one {# ITEM} other {# ITEMS}} LEFT');
    expect(richParts('ranks <em>your</em> stuff')).toEqual([{ text: 'ranks ', tag: null }, { text: 'your', tag: 'em' }, { text: ' stuff', tag: null }]);
    expect(richParts('<b>3</b> new · <code>a<b</code>')).toEqual([{ text: '3', tag: 'b' }, { text: ' new · ', tag: null }, { text: 'a<b', tag: 'code' }]);
    expect(richParts('<good>+5p</good> or <bad>−2p</bad>')).toEqual([{ text: '+5p', tag: 'good' }, { text: ' or ', tag: null }, { text: '−2p', tag: 'bad' }]);
    // A tag the app does not render stays text, so it cannot become markup.
    expect(richParts('<script>x</script>')).toEqual([{ text: '<script>x</script>', tag: null }]);
    const pseudo = pseudoLocalize('Hi {name}, <em>you</em>');
    expect(pseudo).toMatch(/^\[Ĥí \{name\}, <em>ýóú<\/em>·+\]$/);
    expect(formatMessage(pseudoLocalize(COUNT), { count: 2 }, 'en')).toContain('2 íţéɱš');
  });
});

describe('catalogs', () => {
  const sources = { a: { text: 'Hello {name}', note: 'Greeting.', max: 20 }, b: { text: 'Bye', note: 'Farewell.' } };

  it('round-trips PO text, including drafts and escapes', () => {
    const file = { language: 'fr', entries: [{ key: 'a', english: 'Say "hi"\nnow', translation: 'Dis « salut »', fuzzy: true, previousEnglish: 'Say hi', notes: ['Note.'] }] };
    expect(parsePo(formatPo(file))).toEqual(file);
  });

  it('adds new keys, drops removed ones and turns changed English into a draft', () => {
    const old = { language: 'fr', entries: [
      { key: 'a', english: 'Hi {name}', translation: 'Salut {name}', fuzzy: false, notes: [] },
      { key: 'gone', english: 'Old', translation: 'Vieux', fuzzy: false, notes: [] },
    ] };
    const synced = syncCatalog(sources, 'fr', old);
    expect(synced.entries.map(entry => entry.key)).toEqual(['a', 'b']);
    expect(synced.entries[0]).toMatchObject({ translation: 'Salut {name}', fuzzy: true, previousEnglish: 'Hi {name}', notes: ['Greeting.', 'Max 20 characters.'] });
    expect(synced.entries[1]).toMatchObject({ translation: '', fuzzy: false });
  });

  it('flags wrong arguments, broken plurals, lost tags, length and stale files', () => {
    const tagged = { ...sources, c: { text: 'Your <em>own</em>', note: 'Tagged.' } };
    const file = formatPo({ language: 'fr', entries: syncCatalog(tagged, 'fr', null).entries.map(entry => ({
      ...entry,
      translation: { a: 'Bonjour {nom} et une très longue phrase', b: '{n, plural, one {x}}', c: 'Votre propre' }[entry.key] ?? '',
    })) });
    const { errors, warnings, coverage } = checkCatalogs(tagged, { fr: file }, ['fr', 'de']);
    expect(errors.join('\n')).toMatch(/fr a: uses \{nom\} but English uses \{name\}/);
    expect(errors.join('\n')).toMatch(/fr b: .*other/);
    expect(errors.join('\n')).toMatch(/fr c: tags differ/);
    expect(errors.join('\n')).toMatch(/de: catalog missing/);
    expect(warnings).toEqual([expect.stringMatching(/fr a: \d+ characters/)]);
    expect(coverage.fr).toEqual({ total: 3, translated: 3, reviewed: 3 });
    expect(checkCatalogs(tagged, { fr: file.replace('msgctxt "b"', 'msgctxt "b2"') }, ['fr']).errors.join()).toMatch(/out of step/);
  });

  it('the shipped catalogs are in step with en.ts and valid', () => {
    const files = Object.fromEntries(TRANSLATED.map(language => [language, readFileSync(`src/i18n/locales/${language}.po`, 'utf8')]));
    expect(checkCatalogs(EN, files, TRANSLATED).errors).toEqual([]);
  });
});

describe('runtime', () => {
  it('translates from a catalog and falls back to English per key', async () => {
    await i18n.use('fr');
    const french = parsePo(readFileSync('src/i18n/locales/fr.po', 'utf8'));
    const translated = french.entries.find(entry => entry.translation);
    if (translated) expect(i18n.t(translated.key as keyof typeof EN, { count: 2, summary: 'x', age: '1 h', state: 'x' })).not.toBe('');
    const missing = french.entries.find(entry => !entry.translation);
    if (missing) expect(i18n.t(missing.key as keyof typeof EN, { count: 1, summary: 'x', age: '1 h', state: 'x' })).toBe(formatMessage(EN[missing.key as keyof typeof EN].text, { count: 1, summary: 'x', age: '1 h', state: 'x' }, 'en'));
    await i18n.use('pseudo');
    expect(i18n.t('firstRun.scan')).toMatch(/^\[Šçáñ íñṽéñţóŕý·+\]$/);
    await i18n.use('en');
    expect(i18n.t('strip.unmatched', { count: 1, summary: 'quests' })).toMatch(/^1 item couldn't be price-matched \(quests\)/);
    expect(i18n.tag).toBe('en');
  });

  it('starts only in a language a player can choose, but previews any catalog', () => {
    expect(savedLanguage('fr')).toBe('fr');
    expect(savedLanguage('zh-hant')).toBe('zh-hant');
    expect(savedLanguage('th')).toBe('en');
    expect(savedLanguage('pseudo')).toBe('en');
    expect(savedLanguage('klingon')).toBe('en');
    expect(savedLanguage(null)).toBe('en');
    expect(previewLanguage('th')).toBe('th');
    expect(previewLanguage('pseudo')).toBe('pseudo');
    expect(previewLanguage('klingon')).toBe('en');
  });
});
