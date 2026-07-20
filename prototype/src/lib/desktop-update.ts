// C5 desktop auto-update surface. Desktop-only by construction — every entry
// point invokes a Tauri command, so nothing here is reachable in the hosted
// build (the update banner renders only in desktop mode; the hosted SPA
// updates by redeploy). Deliberately NOT on the Transport seam: updates are a
// desktop-shell concern with no loopback-companion analogue, like the tray.
//
// Contract with the Rust side (tennoworth-desktop/src/update.rs): the check
// never rejects — offline / malformed manifest / bad endpoint all read as
// `available: false`. Only `installUpdate` can reject (download failure, bad
// signature), and only after the user explicitly confirmed; the caller shows
// the message and the running app is untouched.

import { resolveInvoke } from './transport';

export interface UpdateStatus {
  /** False until the launch check (or a manual check) has completed. */
  checked: boolean;
  available: boolean;
  current_version: string;
  version: string | null;
  notes: string | null;
}

/** The last check's outcome — no network. Pull side of the mount handshake. */
export function updateStatus(): Promise<UpdateStatus> {
  return resolveInvoke()<UpdateStatus>('update_status');
}

/** Fresh check (network). Resolves `available: false` on any failure. */
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

/**
 * Push side: the Rust launch check emits `update-available` when it finds one.
 * Registration is best-effort (no-op when the event API is absent) because the
 * mount also pulls `updateStatus()` — an emit that beat the listener is never
 * lost, and a check that finishes after mount still lands here.
 */
export function onUpdateAvailable(cb: (s: UpdateStatus) => void): void {
  const w = globalThis as unknown as {
    __TAURI__?: {
      event?: {
        listen?: (name: string, handler: (e: { payload: UpdateStatus }) => void) => Promise<unknown>;
      };
    };
  };
  const listen = w.__TAURI__?.event?.listen;
  if (!listen) return;
  void listen('update-available', (e) => cb(e.payload)).catch(() => {});
}
