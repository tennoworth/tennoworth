// Keeps each language's catalog in step with the English source, and checks
// what a reviewer cannot see at a glance: arguments, plural syntax, tags and
// length. `bun scripts/i18n.ts` runs these over the files.
import { argumentNames, tagsIn, MessageError } from './message';
import { formatPo, parsePo, type PoEntry, type PoFile } from './po';

interface Source { text: string; note: string; max?: number }
type Sources = Record<string, Source>;

function notes(source: Source): string[] {
  return [source.note, ...(source.max ? [`Max ${source.max} characters.`] : [])];
}

/**
 * New keys arrive empty, removed keys go, and a changed English text turns
 * its translation back into a draft that remembers the English it came from.
 */
export function syncCatalog(sources: Sources, language: string, existing: PoFile | null): PoFile {
  const prior = new Map(existing?.entries.map(entry => [entry.key, entry]));
  const entries: PoEntry[] = Object.entries(sources).map(([key, source]) => {
    const old = prior.get(key);
    const entry: PoEntry = { key, english: source.text, translation: '', fuzzy: false, notes: notes(source) };
    if (!old) return entry;
    entry.translation = old.translation;
    entry.fuzzy = old.fuzzy;
    entry.previousEnglish = old.fuzzy ? old.previousEnglish : undefined;
    if (old.english !== source.text && old.translation) {
      entry.fuzzy = true;
      entry.previousEnglish = old.previousEnglish ?? old.english;
    }
    return entry;
  });
  return { language, entries };
}

export interface Coverage { total: number; translated: number; reviewed: number }

export interface CheckResult {
  errors: string[];
  warnings: string[];
  coverage: Record<string, Coverage>;
}

const visibleLength = (text: string) => [...text.replace(/\{[^{}]*\}/g, '').replace(/<\/?[a-z]+>/g, '')].length;

export function checkCatalogs(sources: Sources, files: Record<string, string>, languages: readonly string[]): CheckResult {
  const result: CheckResult = { errors: [], warnings: [], coverage: {} };
  const sourceArgs = new Map<string, string[]>();
  for (const [key, source] of Object.entries(sources)) {
    try { sourceArgs.set(key, argumentNames(source.text)); }
    catch (error) { result.errors.push(`en ${key}: ${(error as Error).message}`); }
  }
  for (const language of languages) {
    const raw = files[language];
    if (raw === undefined) { result.errors.push(`${language}: catalog missing; run bun scripts/i18n.ts --write`); continue; }
    let file: PoFile;
    try { file = parsePo(raw); }
    catch (error) { result.errors.push(`${language}: ${(error as Error).message}`); continue; }
    if (file.language !== language) result.errors.push(`${language}: header says Language: ${file.language}`);
    if (formatPo(syncCatalog(sources, language, file)) !== raw) {
      result.errors.push(`${language}: out of step with en.ts; run bun scripts/i18n.ts --write`);
    }
    const coverage: Coverage = { total: Object.keys(sources).length, translated: 0, reviewed: 0 };
    for (const entry of file.entries) {
      const source = sources[entry.key];
      if (!source || !entry.translation) continue;
      coverage.translated++;
      if (!entry.fuzzy) coverage.reviewed++;
      const where = `${language} ${entry.key}`;
      try {
        const args = argumentNames(entry.translation);
        const expected = sourceArgs.get(entry.key) ?? [];
        if (args.join() !== expected.join()) result.errors.push(`${where}: uses {${args.join(', ')}} but English uses {${expected.join(', ')}}`);
      } catch (error) {
        result.errors.push(`${where}: ${error instanceof MessageError ? error.message : String(error)}`);
      }
      if (tagsIn(entry.translation).join() !== tagsIn(source.text).join()) result.errors.push(`${where}: tags differ from English (${tagsIn(source.text).join(' ') || 'none'})`);
      if (source.max && visibleLength(entry.translation) > source.max) {
        result.warnings.push(`${where}: ${visibleLength(entry.translation)} characters, the space holds about ${source.max}`);
      }
    }
    result.coverage[language] = coverage;
  }
  return result;
}
