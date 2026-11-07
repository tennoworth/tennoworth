// The sidebar's width, chosen by dragging its edge. Dragging it narrow snaps it
// to the icon rail; the rail's width itself lives in CSS (--sidebar-rail).
import type { SettingsStore } from '../../contracts/state-store';

export const SIDEBAR_MIN = 176;
export const SIDEBAR_MAX = 320;
export const SIDEBAR_DEFAULT = 216;
// Below this a drag means "collapse" rather than "a very narrow sidebar".
const SNAP_TO_RAIL = 128;
const STEP = 16;

export class SidebarLayout {
  width = $state(SIDEBAR_DEFAULT);
  collapsed = $state(false);

  constructor(private store: SettingsStore) {
    try {
      const saved = JSON.parse(store.getSetting('sidebar') ?? 'null') as { width?: unknown; collapsed?: unknown } | null;
      if (saved && typeof saved.width === 'number') this.width = clamp(saved.width);
      if (saved && typeof saved.collapsed === 'boolean') this.collapsed = saved.collapsed;
    } catch { /* A malformed value keeps the defaults. */ }
  }

  /** A drag position, in px from the shell's left edge. */
  dragTo(x: number): void {
    if (x < SNAP_TO_RAIL) { this.collapsed = true; return; }
    this.collapsed = false;
    this.width = clamp(x);
  }

  toggle(): void { this.collapsed = !this.collapsed; this.save(); }

  /** Keyboard resizing on the separator; returns whether the key was used. */
  key(key: string): boolean {
    // Enter only, as a window splitter does: Space on the way past was a
    // surprise collapse.
    if (key === 'Enter') { this.toggle(); return true; }
    if (key === 'ArrowLeft') {
      if (this.collapsed) return true;
      if (this.width <= SIDEBAR_MIN) this.collapsed = true; else this.width = clamp(this.width - STEP);
    } else if (key === 'ArrowRight') {
      if (this.collapsed) { this.collapsed = false; this.width = SIDEBAR_MIN; } else this.width = clamp(this.width + STEP);
    } else if (key === 'Home') { this.collapsed = false; this.width = SIDEBAR_MIN; }
    else if (key === 'End') { this.collapsed = false; this.width = SIDEBAR_MAX; }
    else return false;
    this.save();
    return true;
  }

  save(): void {
    void this.store.setSetting('sidebar', JSON.stringify({ width: this.width, collapsed: this.collapsed })).catch(() => {});
  }
}

function clamp(px: number): number {
  return Math.round(Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, px)));
}
