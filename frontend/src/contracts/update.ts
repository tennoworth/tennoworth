

export const UPDATE_CHECK_INTERVAL_MS = 30 * 60 * 1000;


export const UPDATE_SUPPORT = [
  'supported',
  'appimage_required',
  'disabled_test_build',
] as const;

export type UpdateSupport = (typeof UPDATE_SUPPORT)[number];


export interface UpdateStatus {
  /** False until the launch check (or a manual check) has completed. */
  checked: boolean;
  available: boolean;
  support: UpdateSupport;
  current_version: string;
  version: string | null;
  notes: string | null;
}


/** Event name the Rust close-with-tray path emits so the SPA shows its once-ever tray banner. */
export const TRAY_HINT_EVENT = 'tray-hint';
