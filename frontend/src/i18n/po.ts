// The subset of gettext PO the catalogs use: one entry per message key, the
// key in msgctxt, the English text in msgid, the translation in msgstr. A
// `#, fuzzy` flag marks a draft nobody has reviewed yet; `#| msgid` keeps the
// English a draft was made from after the English changed.

export interface PoEntry {
  key: string;
  english: string;
  translation: string;
  fuzzy: boolean;
  notes: string[];
  previousEnglish?: string;
}

export interface PoFile {
  language: string;
  entries: PoEntry[];
}

function unquote(line: string, at: number): string {
  const raw = line.slice(line.indexOf('"', at) + 1, line.lastIndexOf('"'));
  return raw.replace(/\\(.)/g, (_, c: string) => (c === 'n' ? '\n' : c === 't' ? '\t' : c));
}

function quote(text: string): string {
  return `"${text.replace(/\\/g, '\\\\').replace(/"/g, '\\"').replace(/\n/g, '\\n').replace(/\t/g, '\\t')}"`;
}

export function parsePo(source: string): PoFile {
  const entries: PoEntry[] = [];
  let language = '';
  for (const block of source.replace(/\r\n/g, '\n').split(/\n\s*\n/)) {
    const lines = block.split('\n').filter(line => line.trim());
    if (!lines.length) continue;
    const entry: PoEntry = { key: '', english: '', translation: '', fuzzy: false, notes: [] };
    let field: 'key' | 'english' | 'translation' | 'previousEnglish' | null = null;
    for (const line of lines) {
      if (line.startsWith('#.')) entry.notes.push(line.slice(2).trim());
      else if (line.startsWith('#,')) entry.fuzzy ||= line.slice(2).split(',').some(flag => flag.trim() === 'fuzzy');
      else if (line.startsWith('#| msgid ')) entry.previousEnglish = unquote(line, 8);
      else if (line.startsWith('#')) continue;
      else if (line.startsWith('msgctxt ')) { field = 'key'; entry.key = unquote(line, 7); }
      else if (line.startsWith('msgid ')) { field = 'english'; entry.english = unquote(line, 5); }
      else if (line.startsWith('msgstr ')) { field = 'translation'; entry.translation = unquote(line, 6); }
      else if (line.startsWith('"') && field) entry[field] = (entry[field] ?? '') + unquote(line, 0);
      else throw new Error(`Unreadable PO line: ${line}`);
    }
    if (!entry.key && !entry.english) {
      language = /^Language: *(\S+)$/m.exec(entry.translation)?.[1] ?? language;
      continue;
    }
    if (!entry.key) throw new Error(`PO entry without msgctxt: ${entry.english}`);
    entries.push(entry);
  }
  return { language, entries };
}

export function formatPo(file: PoFile): string {
  const header = [
    'msgid ""',
    'msgstr ""',
    quote(`Language: ${file.language}\n`),
    quote('MIME-Version: 1.0\n'),
    quote('Content-Type: text/plain; charset=UTF-8\n'),
    quote('Content-Transfer-Encoding: 8bit\n'),
  ].join('\n');
  const blocks = file.entries.map(entry => [
    ...entry.notes.map(note => `#. ${note}`),
    ...(entry.fuzzy ? ['#, fuzzy'] : []),
    ...(entry.fuzzy && entry.previousEnglish !== undefined ? [`#| msgid ${quote(entry.previousEnglish)}`] : []),
    `msgctxt ${quote(entry.key)}`,
    `msgid ${quote(entry.english)}`,
    `msgstr ${quote(entry.translation)}`,
  ].join('\n'));
  return [header, ...blocks].join('\n\n') + '\n';
}
