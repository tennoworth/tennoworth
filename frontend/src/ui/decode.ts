// The terminal decode the first-run language picker uses: characters settle
// left to right while the unsettled tail cycles through glyphs of the target
// text, so every script decodes in its own letters.
import { tick } from 'svelte';

/** `text` with the first `progress` share settled and the rest scrambled. */
export function scramble(text: string, progress: number, frame: number): string {
  const chars = [...text];
  const pool = chars.filter((c) => c.trim());
  if (progress >= 1 || !pool.length) return text;
  const settled = Math.floor(chars.length * Math.max(0, progress));
  return chars.map((c, i) => (i < settled || !c.trim() ? c : pool[(i * 7 + frame) % pool.length])).join('');
}

/**
 * Decode the text already on screen under `root`, once. It writes only text
 * node values and hands each node back with the text it found, and it lets go
 * of a node the moment something else rewrites it (a clock, a status), so the
 * page's own updates always win. Returns a cancel function.
 */
export function decodePage(root: Element, ms: number, limit = 400, only: (node: Text) => boolean = () => true): () => void {
  if (ms <= 0) return () => {};
  const walker = textWalker(root, only);
  const nodes: { node: Text; text: string; written: string }[] = [];
  for (let node = walker.nextNode(); node && nodes.length < limit; node = walker.nextNode()) {
    nodes.push({ node: node as Text, text: node.nodeValue ?? '', written: node.nodeValue ?? '' });
  }
  const start = performance.now();
  let raf = 0;
  const step = (now: number) => {
    const progress = Math.min(1, (now - start) / ms);
    const frame = Math.floor(now / 40);
    for (const entry of nodes) {
      if (entry.node.nodeValue !== entry.written) continue;
      entry.written = scramble(entry.text, progress, frame);
      entry.node.nodeValue = entry.written;
    }
    if (progress < 1) raf = requestAnimationFrame(step);
  };
  raf = requestAnimationFrame(step);
  return () => {
    cancelAnimationFrame(raf);
    for (const entry of nodes) if (entry.node.nodeValue === entry.written) entry.node.nodeValue = entry.text;
  };
}

function textWalker(root: Element, only: (node: Text) => boolean = () => true): TreeWalker {
  return document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
    acceptNode: (node) => (node.nodeValue?.trim() && !node.parentElement?.closest('[aria-hidden="true"], [data-no-decode], script, style, input, textarea') && only(node as Text) ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT),
  });
}

let decoding: (() => void) | null = null;

/**
 * Run `change` (a language switch) and decode only the text it rewrote under
 * `root`; item names, numbers and anything else that stayed put do not move.
 * A newer change settles the running decode first, so a fast cursor never
 * leaves a word from the language before.
 */
export async function decodeChange(root: Element | null, ms: number, change: () => Promise<void>): Promise<void> {
  decoding?.();
  decoding = null;
  const before = new WeakMap<Text, string>();
  if (root && ms > 0) {
    const walker = textWalker(root);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) before.set(node as Text, node.nodeValue ?? '');
  }
  await change();
  await tick();
  if (root) decoding = decodePage(root, ms, 400, (node) => before.get(node) !== node.nodeValue);
}
