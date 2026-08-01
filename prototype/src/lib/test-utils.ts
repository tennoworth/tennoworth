// @ts-nocheck — test scaffolding; the modules under test carry the real types.
//
// Shared fixtures for the desktop-runtime tests. installTauri/removeTauri were
// written out in three files, and the desktop-update copy had already grown a
// `listen` parameter the other two lacked — the point where three copies stop
// being three copies.

/**
 * Fake the objects Tauri v2 injects into the webview.
 *
 * Both globals matter and they are not interchangeable: `isDesktopRuntime()`
 * keys off `__TAURI_INTERNALS__` (per the desktop spike — it is the one Tauri
 * always injects), while `invoke` and the event API are reached through
 * `__TAURI__`. Installing only one produces a runtime that reports desktop and
 * then cannot call anything.
 *
 * `listen` is optional so a test can exercise the absent-event-API path, which
 * is what `onUpdateAvailable`'s best-effort registration is written for.
 */
export function installTauri(invoke, listen) {
  globalThis.__TAURI_INTERNALS__ = { invoke };
  globalThis.__TAURI__ = { core: { invoke }, ...(listen ? { event: { listen } } : {}) };
}

/** Return the runtime to browser mode. Call from afterEach — the globals leak
 *  across files otherwise, and a stray one silently flips a browser-mode test
 *  into desktop mode. */
export function removeTauri() {
  delete globalThis.__TAURI_INTERNALS__;
  delete globalThis.__TAURI__;
}
