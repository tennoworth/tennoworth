/**
 * Item pictures for the Visual list display.
 *
 * The pictures come from the Warframe wiki, downloaded and re-served by our
 * host (`tennoworth-pipeline art`), never loaded from the wiki directly.
 * - Hosted site and browser preview: `/item-art.json` maps slug → a
 *   same-origin file under `/item-art/`.
 * - Desktop: the native `art` protocol answers by slug and caches each picture
 *   locally, so the webview's policy only admits that local scheme.
 * Without either, every lookup is null and lists draw without pictures.
 */
type TauriInternals = { convertFileSrc?: (path: string, protocol?: string) => string };

let index: Promise<Record<string, string>> | null = null;

function loadIndex(): Promise<Record<string, string>> {
  index ??= fetch('/item-art.json')
    .then((r) => (r.ok ? r.json() : null))
    .then((body: unknown) => {
      const items = (body as { items?: unknown } | null)?.items;
      return items && typeof items === 'object' ? (items as Record<string, string>) : {};
    })
    .catch(() => ({}));
  return index;
}

function desktopArt(): TauriInternals['convertFileSrc'] | null {
  const internals = (globalThis as { __TAURI_INTERNALS__?: TauriInternals }).__TAURI_INTERNALS__;
  return typeof internals?.convertFileSrc === 'function' ? internals.convertFileSrc : null;
}

export async function itemArtUrl(slug: string): Promise<string | null> {
  if (!slug) return null;
  const convert = desktopArt();
  if (convert) return convert(slug, 'art');
  const path = (await loadIndex())[slug];
  return typeof path === 'string' && /^item-art\/[\w.-]+$/.test(path) ? `/${path}` : null;
}
