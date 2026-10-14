/** Rows per page for long history lists (ledger, rivens, notifications). */
export const PAGE_SIZE = 100;

export interface PageWindow {
  /** The requested page clamped to the pages that exist, zero-based. */
  page: number;
  pages: number;
  start: number;
  end: number;
}

/**
 * The slice of `total` rows that page `page` shows. A page beyond the end
 * (the list shrank, or a filter narrowed it) clamps to the last page rather
 * than showing an empty one.
 */
export function pageWindow(total: number, page: number, size = PAGE_SIZE): PageWindow {
  const count = Math.max(0, Math.floor(total));
  const pages = Math.max(1, Math.ceil(count / size));
  const current = Math.min(Math.max(0, Math.floor(page) || 0), pages - 1);
  const start = current * size;
  return { page: current, pages, start, end: Math.min(start + size, count) };
}
