import { DesktopCmdError } from '../contracts/errors';
import type { TauriInvoke } from '../contracts/desktop';

/** Rethrow an invoke rejection as its typed form. Rust CmdError arrives as a
 *  plain `{code, message}` object; other commands reject with strings. */
export function rethrowInvoke(e: unknown): never {
  if (e && typeof e === 'object') {
    const o = e as { code?: unknown; message?: unknown };
    if (typeof o.code === 'string' && typeof o.message === 'string') {
      throw new DesktopCmdError(o.code, o.message);
    }
  }
  if (e instanceof Error) throw e;
  throw new Error(String(e));
}

export function resolveInvoke(): TauriInvoke {
  const w = globalThis as unknown as {
    __TAURI__?: { core?: { invoke?: TauriInvoke } };
    __TAURI_INTERNALS__?: { invoke?: TauriInvoke };
  };
  const invoke = w.__TAURI__?.core?.invoke ?? w.__TAURI_INTERNALS__?.invoke;
  if (!invoke) throw new Error('Tauri IPC unavailable (no invoke on window).');
  return invoke;
}

/** Bridge target=_blank links out of Tauri's single webview and into the
 * system browser. The Rust command applies the final scheme/host allowlist. */
export async function desktopOpenExternalUrl(url: string): Promise<boolean> {
  return await resolveInvoke()<boolean>('open_external_url', { url });
}

export function installDesktopExternalLinkHandler(root: Document = document): () => void {
  if (!isDesktopRuntime()) return () => { };
  const onClick = (event: MouseEvent): void => {
    if (event.defaultPrevented || event.button !== 0) return;
    const element = event.target instanceof Element ? event.target : null;
    const anchor = element?.closest<HTMLAnchorElement>('a[target="_blank"]');
    if (!anchor) return;
    const url = new URL(anchor.href, location.href);
    if (url.protocol !== 'https:') return;
    event.preventDefault();
    void desktopOpenExternalUrl(url.href);
  };
  root.addEventListener('click', onClick);
  return () => root.removeEventListener('click', onClick);
}

/**
 * True inside the Tauri desktop webview. Keyed off `__TAURI_INTERNALS__` (the
 * runtime object Tauri v2 always injects), per the desktop spike - this is a
 * boot-time constant, not a per-call check.
 */
export function isDesktopRuntime(): boolean {
  return (
    typeof globalThis !== 'undefined' &&
    typeof (globalThis as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ !== 'undefined'
  );
}
