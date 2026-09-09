import { diagnosticError } from '../contracts/errors';
import type { UpdateStatus } from '../contracts/update';
// C5 desktop auto-update surface. Desktop-only by construction - every entry
// point invokes a Tauri command, so nothing here is reachable in the hosted
// build (the update banner renders only in desktop mode; the hosted SPA
// updates by redeploy). Deliberately NOT on the DesktopCapabilities seam: updates are a
// desktop-shell concern with no hosted analogue, like the tray.
//
// Contract with the Rust side (tennoworth-desktop/src/update.rs): the check
// never rejects - offline / malformed manifest / bad endpoint all read as
// `available: false`. Only `installUpdate` can reject (download failure, bad
// signature), and only after the user explicitly confirmed; the caller shows
// the message and the running app is untouched.

import { resolveInvoke } from './runtime';

let lastOperation: { operation: string; status: string; error: string | null } | null = null;
export function updateDiagnostics() { return lastOperation ? { ...lastOperation } : null; }
async function trackedUpdate<T>(operation: string, command: string): Promise<T> {
  lastOperation = { operation, status: 'running', error: null };
  try {
    const result = await resolveInvoke()<T>(command);
    lastOperation = { operation, status: 'completed', error: null };
    return result;
  } catch (error) {
    lastOperation = { operation, status: 'failed', error: diagnosticError(error) };
    throw error;
  }
}

/** The last check's outcome - no network. Pull side of the mount handshake. */
export function updateStatus(): Promise<UpdateStatus> {
  return resolveInvoke()<UpdateStatus>('update_status');
}

/** Fetch the signed updater manifest now. Nothing is downloaded or installed. */
export function checkUpdate(): Promise<UpdateStatus> {
  return trackedUpdate<UpdateStatus>('check', 'check_update');
}

/** Download + install the pending update. Explicit user confirmation only. */
export async function installUpdate(): Promise<void> {
  await trackedUpdate('install', 'install_update');
}

/** Relaunch to switch to the installed version ("apply on restart"). */
export async function restartApp(): Promise<void> {
  await trackedUpdate('restart', 'restart_app');
}

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
