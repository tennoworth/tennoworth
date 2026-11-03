/**
 * Item pictures for the Visual list display.
 *
 * Local builds only: the slug → thumbnail map is an untracked same-origin file
 * (`public/item-thumbs.local`), and the pictures load from warframe.market's
 * image host, which the local CSP admits in img-src. A shipped build needs the
 * thumbnails baked same-origin instead; without the map every lookup is null
 * and lists draw without pictures.
 */
const IMAGE_HOST = 'https://warframe.market/static/assets/';

let thumbs: Promise<Record<string, string>> | null = null;

function loadThumbs(): Promise<Record<string, string>> {
  thumbs ??= fetch('/item-thumbs.local')
    .then((r) => (r.ok ? r.json() : {}))
    .then((map: unknown) => (map && typeof map === 'object' ? (map as Record<string, string>) : {}))
    .catch(() => ({}));
  return thumbs;
}

export async function itemArtUrl(slug: string): Promise<string | null> {
  const path = (await loadThumbs())[slug];
  return typeof path === 'string' && path ? IMAGE_HOST + path : null;
}
