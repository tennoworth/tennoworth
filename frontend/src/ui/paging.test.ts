import { describe, expect, it } from 'vitest';
import { PAGE_SIZE, pageWindow } from './paging';

describe('pageWindow', () => {
  it('shows a short list whole on one page', () => {
    expect(pageWindow(42, 0)).toEqual({ page: 0, pages: 1, start: 0, end: 42 });
  });

  it('treats an empty list as one empty page', () => {
    expect(pageWindow(0, 3)).toEqual({ page: 0, pages: 1, start: 0, end: 0 });
  });

  it('splits at the page size and ends the last page at the total', () => {
    expect(PAGE_SIZE).toBe(100);
    expect(pageWindow(250, 1)).toEqual({ page: 1, pages: 3, start: 100, end: 200 });
    expect(pageWindow(250, 2)).toEqual({ page: 2, pages: 3, start: 200, end: 250 });
    expect(pageWindow(200, 1)).toEqual({ page: 1, pages: 2, start: 100, end: 200 });
  });

  it('clamps a page past the end to the last page instead of an empty one', () => {
    expect(pageWindow(150, 9)).toEqual({ page: 1, pages: 2, start: 100, end: 150 });
  });

  it('clamps negative and non-numeric pages to the first', () => {
    expect(pageWindow(150, -2).page).toBe(0);
    expect(pageWindow(150, Number.NaN).page).toBe(0);
  });
});
