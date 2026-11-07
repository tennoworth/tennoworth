// Translation catalogs: `--write` brings every locales/*.po in step with
// src/i18n/en.ts. Without --write it only checks: it fails on a catalog that is
// out of step or has a broken translation, and prints each language's progress.

import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { EN } from '../src/i18n/en';
import { TRANSLATED } from '../src/i18n/languages';
import { checkCatalogs, syncCatalog } from '../src/i18n/catalog';
import { formatPo, parsePo } from '../src/i18n/po';

const path = (language: string) => new URL(`../src/i18n/locales/${language}.po`, import.meta.url);
const read = (language: string) => (existsSync(path(language)) ? readFileSync(path(language), 'utf8') : undefined);

if (process.argv.includes('--write')) {
  for (const language of TRANSLATED) {
    const raw = read(language);
    writeFileSync(path(language), formatPo(syncCatalog(EN, language, raw === undefined ? null : parsePo(raw))));
  }
}

const files = Object.fromEntries(TRANSLATED.flatMap(language => {
  const raw = read(language);
  return raw === undefined ? [] : [[language, raw]];
}));
const { errors, warnings, coverage } = checkCatalogs(EN, files, TRANSLATED);
for (const [language, { total, translated, reviewed }] of Object.entries(coverage)) {
  console.log(`${language.padEnd(8)} ${translated}/${total} translated, ${reviewed} reviewed`);
}
for (const warning of warnings) console.warn(`warning: ${warning}`);
for (const error of errors) console.error(`error: ${error}`);
if (errors.length) process.exit(1);
