// Every table fits its pane in the same steps (design-system.md, "Fitting
// tables"). Step 0 is the table as designed. Step 1 narrows it: fixed minimum
// widths give way, cells tighten and headers and names wrap. Each later step
// hides one more rank of the columns whose header cell carries
// `data-drop="1|2|3"`, rank 1 first; unmarked columns never hide. Only the
// header is marked: the rank is copied to the body cells in that column. The
// scroll box carries the step as `data-fit` and names what is hidden in
// `data-fit-note`, which shared CSS draws under the table. Sideways scrolling is left only for content that cannot fit
// even then, such as enlarged text.
//
// The same header marks define columns as well as hiding them: `data-group`
// starts a group of columns (a faint line on its left), and `data-key` marks
// the column the table is about (a faint wash). Both are copied down like the
// rank. While the pointer rests on a cell, its column carries `data-hot`, so
// the reader can see which header a cell belongs to without counting across.
import { t } from '../i18n/i18n.svelte';

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
  return hidden.length ? t('ui.hiddenToFit', { count: hidden.length, columns: hidden.join(', ') }) : undefined;
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

/** The header marks copied down each column. */
const COLUMN_MARKS = ['drop', 'group', 'key'] as const;
type ColumnMarks = Partial<Record<(typeof COLUMN_MARKS)[number], string>>;

function setMarks(cell: HTMLElement, marks: ColumnMarks) {
  for (const name of COLUMN_MARKS) {
    const value = marks[name];
    if (value != null) cell.dataset[name] = value; else delete cell.dataset[name];
  }
}

/** The last header row, one entry per column it spans. */
function headerColumns(table: HTMLTableElement): ColumnMarks[] | null {
  const headRow = table.tHead?.rows[table.tHead.rows.length - 1];
  if (!headRow) return null;
  const columns: ColumnMarks[] = [];
  for (const th of headRow.cells) {
    const marks: ColumnMarks = {};
    for (const name of COLUMN_MARKS) if (th.dataset[name] != null) marks[name] = th.dataset[name];
    for (let i = 0; i < th.colSpan; i++) columns.push(marks);
  }
  return columns;
}

/** Copies each header's marks to the body cells beneath it, and its rank to
 *  its `<col>`, whose declared width would otherwise hold the column open.
 *  Rows that span columns (detail rows, empty states) are left whole. */
export function markColumns(table: HTMLTableElement) {
  const columns = headerColumns(table);
  if (!columns) return;
  const cols = [...table.querySelectorAll<HTMLTableColElement>(':scope > colgroup > col')];
  if (cols.length === columns.length) cols.forEach((col, i) => {
    if (columns[i]!.drop) col.dataset.drop = columns[i]!.drop; else delete col.dataset.drop;
  });
  for (const body of table.tBodies) {
    for (const row of body.rows) {
      if (row.cells.length !== columns.length) continue;
      [...row.cells].forEach((cell, i) => setMarks(cell, columns[i]!));
    }
  }
}

/** Marks the column under the pointer with `data-hot`, header included.
 *  `fitTable` installs it; a table that plans its own fit uses it directly. */
export function columnCursor(box: HTMLElement) {
  let hot: HTMLElement[] = [];
  const clear = () => { for (const cell of hot) delete cell.dataset.hot; hot = []; };
  const over = (event: Event) => {
    const cell = (event.target as Element | null)?.closest?.('td, th');
    const table = cell?.closest('table');
    if (!cell || !table || table.parentElement !== box) return clear();
    const width = headerColumns(table)?.length ?? 0;
    const row = cell.parentElement as HTMLTableRowElement;
    // A cell in a spanning row has no single column.
    if (row.cells.length !== width) return clear();
    const index = (cell as HTMLTableCellElement).cellIndex;
    if (hot[0] && hot.includes(cell as HTMLElement)) return;
    clear();
    for (const r of table.rows) {
      if (r.cells.length !== width) continue;
      const c = r.cells[index];
      if (c) { c.dataset.hot = ''; hot.push(c); }
    }
  };
  box.addEventListener('pointerover', over);
  box.addEventListener('pointerleave', clear);
  return { destroy() { box.removeEventListener('pointerover', over); box.removeEventListener('pointerleave', clear); clear(); } };
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
    markColumns(table);
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
  const cursor = columnCursor(box);
  fit();
  return {
    destroy() {
      resize?.disconnect(); content.disconnect(); cursor.destroy();
      if (frame) cancelAnimationFrame(frame);
      delete box.dataset.fit; delete box.dataset.fitNote;
    },
  };
}
