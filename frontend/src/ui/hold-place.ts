// Keeps the row of filter controls the player just used where it was.
// Content above the controls (Top picks) changes height with every filter, so
// without this the next tag sits somewhere else by the time the pointer gets
// there. Growth above is absorbed by scrolling; a shrink the scroll position
// cannot absorb is held as a gap, which later growth fills first.

const SETTLE_MS = 1000;

function scroller(node: HTMLElement): HTMLElement {
  for (let el = node.parentElement; el; el = el.parentElement) {
    const { overflowY } = getComputedStyle(el);
    if ((overflowY === 'auto' || overflowY === 'scroll') && el.scrollHeight > el.clientHeight) return el;
  }
  return (document.scrollingElement as HTMLElement | null) ?? document.documentElement;
}

/** `controls` selects the rows inside `node` that hold their place when used;
 *  `node` is the block that scrolls or gives way as a whole. */
export function holdPlace(node: HTMLElement, controls = '.bar') {
  let row: Element | null = null;
  let anchor = 0;
  let until = 0;
  let hold = 0;

  const setHold = (px: number) => {
    hold = Math.max(0, px);
    node.style.marginTop = hold > 0.5 ? `${hold}px` : '';
  };

  const mark = (event: Event) => {
    const target = event.target as Element | null;
    row = target?.closest?.(controls) ?? null;
    if (!row) return;
    anchor = row.getBoundingClientRect().top;
    until = performance.now() + SETTLE_MS;
  };
  const release = () => { row = null; };

  const settle = () => {
    if (!row) return;
    if (performance.now() > until || !row.isConnected) { row = null; return; }
    let delta = row.getBoundingClientRect().top - anchor;
    if (Math.abs(delta) < 0.5) return;
    if (delta > 0 && hold > 0) {
      const take = Math.min(hold, delta);
      setHold(hold - take);
      delta -= take;
    }
    const box = scroller(node);
    const before = box.scrollTop;
    box.scrollTop = before + delta;
    const short = delta - (box.scrollTop - before);
    if (short < -0.5) setHold(hold - short);
  };

  // Without ResizeObserver (jsdom) there is no layout to hold.
  const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(settle);
  if (node.parentElement) observer?.observe(node.parentElement);
  node.addEventListener('pointerdown', mark, true);
  node.addEventListener('keydown', mark, true);
  node.addEventListener('input', mark, true);
  window.addEventListener('wheel', release, { passive: true });
  window.addEventListener('touchmove', release, { passive: true });

  return {
    destroy() {
      observer?.disconnect();
      node.removeEventListener('pointerdown', mark, true);
      node.removeEventListener('keydown', mark, true);
      node.removeEventListener('input', mark, true);
      window.removeEventListener('wheel', release);
      window.removeEventListener('touchmove', release);
    },
  };
}
