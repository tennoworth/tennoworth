// A small ICU MessageFormat subset: {name}, {n, number},
// {n, plural, =0 {...} one {...} other {...}} with #, and
// {x, select, a {...} other {...}}. Apostrophes quote only before a brace,
// a # inside a plural, or another apostrophe, so "l'inventaire" stays as it is.

type Node =
  | string
  | { kind: 'arg'; name: string; number?: boolean }
  | { kind: 'pound' }
  | { kind: 'plural' | 'select'; name: string; cases: Record<string, Node[]> };

export type MessageArgs = Record<string, string | number>;

export class MessageError extends Error {}

export function parseMessage(source: string): Node[] {
  let at = 0;
  const fail = (why: string): never => { throw new MessageError(`${why} at ${at} in "${source}"`); };
  const space = () => { while (/\s/.test(source[at] ?? '')) at++; };
  const word = () => {
    space();
    const start = at;
    while (at < source.length && /[^\s,{}]/.test(source[at])) at++;
    if (start === at) fail('Expected a name');
    return source.slice(start, at);
  };
  const expect = (char: string) => { space(); if (source[at] !== char) fail(`Expected "${char}"`); at++; };

  function nodes(inPlural: boolean, nested: boolean): Node[] {
    const out: Node[] = [];
    let text = '';
    const flush = () => { if (text) out.push(text); text = ''; };
    while (at < source.length) {
      const char = source[at];
      if (char === "'") {
        const next = source[at + 1];
        if (next === "'") { text += "'"; at += 2; continue; }
        if (next === '{' || next === '}' || (inPlural && next === '#')) {
          const close = source.indexOf("'", at + 1);
          if (close < 0) fail('Unclosed quote');
          text += source.slice(at + 1, close);
          at = close + 1;
          continue;
        }
        text += char; at++; continue;
      }
      if (char === '}') { if (!nested) fail('Unexpected "}"'); break; }
      if (char === '#' && inPlural) { flush(); out.push({ kind: 'pound' }); at++; continue; }
      if (char === '{') { flush(); at++; out.push(argument()); continue; }
      text += char; at++;
    }
    flush();
    return out;
  }

  function argument(): Node {
    const name = word();
    space();
    if (source[at] === '}') { at++; return { kind: 'arg', name }; }
    expect(',');
    const type = word();
    space();
    if (type === 'number') { expect('}'); return { kind: 'arg', name, number: true }; }
    if (type !== 'plural' && type !== 'select') fail(`Unsupported argument type "${type}"`);
    expect(',');
    const cases: Record<string, Node[]> = {};
    space();
    while (source[at] !== '}') {
      const selector = word();
      expect('{');
      cases[selector] = nodes(type === 'plural', true);
      expect('}');
      space();
      if (at >= source.length) fail('Unclosed argument');
    }
    at++;
    if (!cases.other) fail(`"${name}" needs an "other" case`);
    return { kind: type as 'plural' | 'select', name, cases };
  }

  const out = nodes(false, false);
  if (at < source.length) fail('Unexpected "}"');
  return out;
}

/** Every argument name a message reads, for checking a translation against English. */
export function argumentNames(source: string): string[] {
  const names = new Set<string>();
  const walk = (list: Node[]) => {
    for (const node of list) {
      if (typeof node === 'string' || node.kind === 'pound') continue;
      names.add(node.name);
      if (node.kind === 'plural' || node.kind === 'select') Object.values(node.cases).forEach(walk);
    }
  };
  walk(parseMessage(source));
  return [...names].sort();
}

export function formatMessage(source: string | Node[], args: MessageArgs, locale: string): string {
  const list = typeof source === 'string' ? parseMessage(source) : source;
  const number = new Intl.NumberFormat(locale);
  const render = (nodes: Node[], count?: number): string => nodes.map(node => {
    if (typeof node === 'string') return node;
    if (node.kind === 'pound') return count === undefined ? '#' : number.format(count);
    const value = args[node.name];
    if (node.kind === 'arg') {
      if (value === undefined) return `{${node.name}}`;
      return node.number || typeof value === 'number' ? number.format(Number(value)) : String(value);
    }
    if (node.kind === 'select') return render(node.cases[String(value)] ?? node.cases.other);
    const n = Number(value);
    const exact = node.cases[`=${n}`];
    if (exact) return render(exact, n);
    return render(node.cases[new Intl.PluralRules(locale).select(n)] ?? node.cases.other, n);
  }).join('');
  return render(list);
}

/** Inline tags a message may carry, such as <em>; a translation must keep the same ones. */
export function tagsIn(source: string): string[] {
  return (source.match(/<\/?[a-z]+>/g) ?? []).sort();
}

/** Split formatted text on <em> so a template can render the emphasis. */
export function richParts(text: string): { text: string; em: boolean }[] {
  return text.split(/(<em>.*?<\/em>)/).filter(Boolean).map(part => {
    const em = /^<em>(.*)<\/em>$/.exec(part);
    return em ? { text: em[1], em: true } : { text: part, em: false };
  });
}

/** Rewrite a message's literal text, keeping its arguments and cases intact. */
export function mapText(source: string, change: (text: string) => string): string {
  const escape = (text: string, inPlural: boolean) =>
    text.replace(/'/g, "''").replace(inPlural ? /[{}#]+/g : /[{}]+/g, run => `'${run}'`);
  const write = (nodes: Node[], inPlural: boolean): string => nodes.map(node => {
    if (typeof node === 'string') return escape(change(node), inPlural);
    if (node.kind === 'pound') return '#';
    if (node.kind === 'arg') return node.number ? `{${node.name}, number}` : `{${node.name}}`;
    const cases = Object.entries(node.cases).map(([selector, list]) => `${selector} {${write(list, node.kind === 'plural')}}`).join(' ');
    return `{${node.name}, ${node.kind}, ${cases}}`;
  }).join('');
  return write(parseMessage(source), false);
}
