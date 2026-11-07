// The terminal decode the first-run language picker uses: characters settle
// left to right while the unsettled tail cycles through glyphs of the target
// text, so every script decodes in its own letters.

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
export function decodePage(root: Element, ms: number, limit = 400): () => void {
  if (ms <= 0) return () => {};
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
    acceptNode: (node) => (node.nodeValue?.trim() && !node.parentElement?.closest('[aria-hidden="true"], script, style, input, textarea') ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT),
  });
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
