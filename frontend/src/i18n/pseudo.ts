// A fake language for layout checks before any real translation: accented
// letters show text that skipped the catalog, and the padding (about 40%,
// what German and Russian often need) shows where a layout cannot grow.
import { mapText } from './message';

const PLAIN = 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ';
const ACCENTED = 'áƀçďéƒĝĥíĵķĺɱñóþǫŕšţúṽŵẋýžÁƁÇĎÉƑĜĤÍĴĶĹṀÑÓÞǪŔŠŢÚṼŴẊÝŽ';
const swap = (text: string) => text.replace(/<[^>]*>|[A-Za-z]/g, part => (part.length > 1 ? part : [...ACCENTED][PLAIN.indexOf(part)]));

export function pseudoLocalize(source: string): string {
  const letters = source.replace(/\{[^{}]*\}/g, '').replace(/[^A-Za-z]/g, '').length;
  return `[${mapText(source, swap)}${'·'.repeat(Math.ceil(letters * 0.4))}]`;
}
