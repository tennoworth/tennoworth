// C5 desktop auto-update surface. Desktop-only by construction - every entry
// point invokes a Tauri command, so nothing here is reachable in the hosted
// build (the update banner renders only in desktop mode; the hosted SPA
// updates by redeploy). Deliberately NOT on the Transport seam: updates are a
// desktop-shell concern with no hosted analogue, like the tray.
//
// Contract with the Rust side (tennoworth-desktop/src/update.rs): the check
// never rejects - offline / malformed manifest / bad endpoint all read as
// `available: false`. Only `installUpdate` can reject (download failure, bad
// signature), and only after the user explicitly confirmed; the caller shows
// the message and the running app is untouched.

import { resolveInvoke } from './transport';

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

/** The last check's outcome - no network. Pull side of the mount handshake. */
export function updateStatus(): Promise<UpdateStatus> {
  return resolveInvoke()<UpdateStatus>('update_status');
}

/** Fetch the signed updater manifest now. Nothing is downloaded or installed. */
export function checkUpdate(): Promise<UpdateStatus> {
  return resolveInvoke()<UpdateStatus>('check_update');
}

/** Download + install the pending update. Explicit user confirmation only. */
export async function installUpdate(): Promise<void> {
  await resolveInvoke()('install_update');
}

/** Relaunch to switch to the installed version ("apply on restart"). */
export async function restartApp(): Promise<void> {
  await resolveInvoke()('restart_app');
}

/** Event name the Rust close-with-tray path emits so the SPA shows its once-ever tray banner. */
export const TRAY_HINT_EVENT = 'tray-hint';

/**
 * Push side: the Rust launch check emits `update-available` when it finds one.
 * Registration is best-effort (no-op when the event API is absent) because the
 * mount also pulls `updateStatus()` - an emit that beat the listener is never
 * lost, and a check that finishes after mount still lands here.
 */
export function onUpdateAvailable(cb: (s: UpdateStatus) => void): () => void {
  return listenForTauriEvent('update-available', cb);
}

/**
 * Register a Rust-emitted event listener. Best-effort: the hosted build has no
 * Tauri event API, so a missing `__TAURI__.event.listen` is a silent no-op and
 * a rejected registration is swallowed - both are the expected shape for the
 * "desktop enhancement in a browser app" split this app lives in. The returned
 * function is safe to call before asynchronous registration finishes.
 */
export function listenForTauriEvent<T>(event: string, cb: (payload: T) => void): () => void {
  const w = globalThis as unknown as {
    __TAURI__?: {
      event?: {
        listen?: (
          name: string,
          handler: (e: { payload: T }) => void,
        ) => Promise<unknown>;
      };
    };
  };
  const listen = w.__TAURI__?.event?.listen;
  if (!listen) return () => {};

  let disposed = false;
  let unlisten: (() => void) | undefined;
  void listen(event, (e) => {
    if (!disposed) cb(e.payload);
  })
    .then((registered) => {
      if (typeof registered !== 'function') return;
      if (disposed) {
        registered();
      } else {
        unlisten = registered as () => void;
      }
    })
    .catch(() => {});

  return () => {
    if (disposed) return;
    disposed = true;
    try {
      unlisten?.();
    } catch {
      // Teardown is best-effort for the same reason registration is.
    }
    unlisten = undefined;
  };
}
