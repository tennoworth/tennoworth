import { describe, expect, it } from 'vitest';
import { fitNote, fitStep, fitTable, hidesAt, markColumns, MAX_DROP_RANK } from './table-fit';

function table(html: string): HTMLTableElement {
  const box = document.createElement('div');
  box.innerHTML = html;
  document.body.append(box);
  return box.querySelector('table')!;
}

const GROUPED = `<table>
  <thead><tr><th>Item</th><th data-group data-drop="1">Own</th><th data-group data-key>Value</th></tr></thead>
  <tbody>
    <tr><td>Ash</td><td>2</td><td>40</td></tr>
    <tr><td colspan="3">detail</td></tr>
  </tbody>
</table>`;

const marks = (cell: Element) => ({ ...(cell as HTMLElement).dataset });

describe('table fit steps', () => {
  it('takes the first step that fits, and the last when none does', () => {
    expect(fitStep(() => true, MAX_DROP_RANK + 1)).toBe(0);
    expect(fitStep((step) => step >= 2, MAX_DROP_RANK + 1)).toBe(2);
    expect(fitStep(() => false, MAX_DROP_RANK + 1)).toBe(MAX_DROP_RANK + 1);
  });

  it('hides nothing before step 2, then one more rank per step', () => {
    expect([0, 1, 2, 3, 4].map((step) => hidesAt(1, step))).toEqual([false, false, true, true, true]);
    expect([0, 1, 2, 3, 4].map((step) => hidesAt(3, step))).toEqual([false, false, false, false, true]);
  });

  it('never hides an unranked column', () => {
    expect(hidesAt(0, MAX_DROP_RANK + 1)).toBe(false);
    expect(hidesAt(Number.NaN, MAX_DROP_RANK + 1)).toBe(false);
  });

  it('copies the group, key and rank of each header down its column', () => {
    const t = table(GROUPED);
    markColumns(t);
    const cells = [...t.tBodies[0]!.rows[0]!.cells];
    expect(cells.map(marks)).toEqual([{}, { group: '', drop: '1' }, { group: '', key: '' }]);
    // A row that spans the table belongs to no single column.
    expect(marks(t.tBodies[0]!.rows[1]!.cells[0]!)).toEqual({});
  });

  it('clears marks a header no longer carries', () => {
    const t = table(GROUPED);
    markColumns(t);
    t.tHead!.rows[0]!.cells[2]!.removeAttribute('data-key');
    markColumns(t);
    expect(marks(t.tBodies[0]!.rows[0]!.cells[2]!)).toEqual({ group: '' });
  });

  it('marks the column under the pointer, header included, and clears it on leaving', () => {
    const t = table(GROUPED);
    const box = t.parentElement!;
    const action = fitTable(box);
    t.tBodies[0]!.rows[0]!.cells[1]!.dispatchEvent(new Event('pointerover', { bubbles: true }));
    const hot = [...t.querySelectorAll('[data-hot]')].map((c) => c.textContent);
    expect(hot).toEqual(['Own', '2']);
    box.dispatchEvent(new Event('pointerleave'));
    expect(t.querySelectorAll('[data-hot]')).toHaveLength(0);
    action.destroy();
  });

  it('names what is hidden, and says nothing when nothing is', () => {
    expect(fitNote([])).toBeUndefined();
    expect(fitNote(['Trend'])).toBe('Hidden to fit this window: Trend. Widen the window to show it.');
    expect(fitNote(['Trend', 'Δ 90d'])).toBe('Hidden to fit this window: Trend, Δ 90d. Widen the window to show them.');
  });
});
