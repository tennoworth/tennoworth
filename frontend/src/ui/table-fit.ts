// Every table fits its pane in the same steps (design-system.md, "Fitting
// tables"). Step 0 is the table as designed. Step 1 narrows it: fixed minimum
// widths give way, cells tighten and headers and names wrap. Each later step
// hides one more rank of the columns whose header cell carries
// `data-drop="1|2|3"`, rank 1 first; unmarked columns never hide. Only the
// header is marked: the rank is copied to the body cells in that column. The
// scroll box carries the step as `data-fit` and names what is hidden in
// `data-fit-note`, which shared CSS draws under the table. Sideways scrolling is left only for content that cannot fit
// even then, such as enlarged text.

export const MAX_DROP_RANK = 3;

/** The first step at which `fits` holds, or the last step when none does. */
export function fitStep(fits: (step: number) => boolean, lastStep: number): number {
  for (let step = 0; step < lastStep; step++) if (fits(step)) return step;
  return lastStep;
}

/** Columns of rank `r` hide from step `r + 1`. */
export function hidesAt(rank: number, step: number): boolean {
  return rank >= 1 && step >= rank + 1;
}

export function fitNote(hidden: string[]): string | undefined {
  return hidden.length ? `Hidden to fit this window: ${hidden.join(', ')}. Widen the window to show ${hidden.length === 1 ? 'it' : 'them'}.` : undefined;
}

function columnLabel(cell: HTMLElement): string {
  return (cell.dataset.label ?? cell.textContent ?? '').replace(/\s+/g, ' ').trim();
}

/** Reports a box's width a frame after it changes. A table that plans its
 *  columns from the width changes its own layout in response; answering
 *  inside the observer callback (as `bind:clientWidth` does) loops. */
export function paneWidth(box: HTMLElement, onwidth: (width: number) => void) {
  let frame = 0;
  const report = () => { frame = 0; onwidth(box.clientWidth); };
  // Without ResizeObserver (jsdom) the box is measured once, like shimmer.ts.
  const resize = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(() => { if (!frame) frame = requestAnimationFrame(report); });
  resize?.observe(box);
  report();
  return { destroy() { resize?.disconnect(); if (frame) cancelAnimationFrame(frame); } };
}

/** Copies each marked header's rank to the body cells beneath it and to its
 *  `<col>`, whose declared width would otherwise hold the column open. Rows
 *  that span columns (detail rows, empty states) are left whole. */
function markColumns(table: HTMLTableElement, heads: HTMLElement[]) {
  const headRow = heads[0]?.parentElement;
  if (!headRow) return;
  const ranks: (string | undefined)[] = [];
  for (const th of headRow.children as HTMLCollectionOf<HTMLTableCellElement>) {
    for (let i = 0; i < th.colSpan; i++) ranks.push(th.dataset.drop);
  }
  const cols = [...table.querySelectorAll<HTMLTableColElement>(':scope > colgroup > col')];
  if (cols.length === ranks.length) cols.forEach((col, i) => {
    if (ranks[i]) col.dataset.drop = ranks[i]; else delete col.dataset.drop;
  });
  for (const body of table.tBodies) {
    for (const row of body.rows) {
      if (row.cells.length !== ranks.length) continue;
      [...row.cells].forEach((cell, i) => {
        if (ranks[i]) cell.dataset.drop = ranks[i]; else delete cell.dataset.drop;
      });
    }
  }
}

/** Fits the table directly inside `box` by measuring it, step by step. */
export function fitTable(box: HTMLElement) {
  let frame = 0;
  const apply = (step: number) => { box.dataset.fit = String(step); };
  const fit = () => {
    frame = 0;
    const table = box.querySelector<HTMLTableElement>(':scope > table');
    if (!table) { delete box.dataset.fit; delete box.dataset.fitNote; return; }
    const heads = [...table.querySelectorAll<HTMLElement>(':scope > thead [data-drop]')];
    markColumns(table, heads);
    const deepest = Math.min(MAX_DROP_RANK, Math.max(0, ...heads.map(th => Number(th.dataset.drop) || 0)));
    const step = fitStep(s => { apply(s); return box.scrollWidth <= box.clientWidth + 1; }, deepest + 1);
    apply(step);
    const note = fitNote(heads.filter(th => hidesAt(Number(th.dataset.drop), step)).map(columnLabel).filter(Boolean));
    if (note) box.dataset.fitNote = note; else delete box.dataset.fitNote;
  };
  const schedule = () => { if (!frame) frame = requestAnimationFrame(fit); };
  const resize = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(schedule);
  resize?.observe(box);
  // Rows arrive and change after mount; a wider row can need the next step.
  const content = new MutationObserver(schedule);
  content.observe(box, { childList: true, subtree: true, characterData: true });
  void document.fonts?.ready.then(schedule);
  fit();
  return {
    destroy() {
      resize?.disconnect(); content.disconnect();
      if (frame) cancelAnimationFrame(frame);
      delete box.dataset.fit; delete box.dataset.fitNote;
    },
  };
}
