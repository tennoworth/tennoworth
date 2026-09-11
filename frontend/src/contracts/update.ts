

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

export interface UpdateChange {
  id: string;
  kind: 'improved' | 'fixed' | 'action';
  title: string;
  body: string;
  platforms: Array<'windows' | 'linux'>;
  supersedes: string[];
}
export interface UpdateRelease { version: string; date: string; changes: UpdateChange[] }
export interface UpdateNotesStatus {
  current_version: string;
  previous_version: string | null;
  auto_show: boolean;
  earlier_version_unknown: boolean;
  partial_history: boolean;
  releases: UpdateRelease[];
  changes: UpdateChange[];
}
export interface UpdateNotesServices {
  updateNotes(): Promise<UpdateNotesStatus>;
  acknowledgeUpdateNotes(version: string): Promise<void>;
  updateNotesCanPresent(): Promise<boolean>;
}
