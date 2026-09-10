import type { Snapshot, SaveSnapshotInput } from '../domain/snapshot';


// The scalar settings the SPA persists. The value is always a short string; the
// caller owns parsing/validation (the parseInt guard, the VALID_VIEWS set), so
// the store stays a dumb, byte-faithful key/value - it never interprets a value.
export type SettingKey =
  | 'reserve-copies'
  | 'filters-open'
  | 'view'
  | 'score-explainer-dismissed'
  | 'keep-copies-nudge-dismissed'
  | 'tray-toast-seen'
  | 'sell-onboarding-dismissed'
  /** Desktop: EE.log sold-detection may adjust WFM listings ('on' | 'off'). The
   *  Rust tailer reads the same `setting` row (trades.rs SETTING_AUTO_CLOSE). */
  | 'auto-close-sold'
  /** Visual theme: `theme.mode` = 'system' | 'light' | 'dark'. Also read RAW
   *  from localStorage by public/theme-boot.js before first paint (the one
   *  sanctioned raw read) - keep the key name below in step with it.
   *  (`theme.look` retired 2026-08 with the four-look picker: yorha is the
   *  only look. Old `wfminv:theme-look-v1` values are simply never read.) */
  | 'theme.mode';


export interface StateStore {
  readonly mode: 'local' | 'tauri';

  /**
   * Load every scalar setting into an in-memory cache so `getSetting` can be
   * read synchronously at component-init time - no default-value flash for a
   * returning user. Called once at boot BEFORE the Svelte app mounts. Must never
   * reject: a backing-store failure leaves the cache empty and every
   * `getSetting` falls back to its caller default.
   */
  hydrate(): Promise<void>;

  /** Synchronous after `hydrate()`. `null` = unset (the caller applies its default). */
  getSetting(key: SettingKey): string | null;
  setSetting(key: SettingKey, value: string): Promise<void>;

  loadSnapshot(): Promise<Snapshot | null>;
  saveSnapshot(input: SaveSnapshotInput, timestamp?: number): Promise<void>;
  clearSnapshot(): Promise<void>;
}
