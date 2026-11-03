import { afterEach, describe, expect, it, vi } from 'vitest';

async function load() {
  vi.resetModules();
  return (await import('./item-art')).itemArtUrl;
}

afterEach(() => {
  vi.unstubAllGlobals();
  delete (globalThis as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
});

describe('itemArtUrl', () => {
  it('serves the hosted picture from the same origin, reading the index once', async () => {
    const fetch = vi.fn(async () => new Response(JSON.stringify({ items: { abating_link: 'item-art/dae3b093983f1152.png' } })));
    vi.stubGlobal('fetch', fetch);
    const itemArtUrl = await load();
    expect(await itemArtUrl('abating_link')).toBe('/item-art/dae3b093983f1152.png');
    expect(await itemArtUrl('no_picture')).toBeNull();
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(fetch).toHaveBeenCalledWith('/item-art.json');
  });

  it('refuses an index entry that is not a local picture path', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ items: { a: 'https://example.com/x.png', b: 'item-art/../x' } }))));
    const itemArtUrl = await load();
    expect(await itemArtUrl('a')).toBeNull();
    expect(await itemArtUrl('b')).toBeNull();
  });

  it('draws nothing when the index is missing or malformed', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('nope', { status: 404 })));
    expect(await (await load())('abating_link')).toBeNull();
    vi.stubGlobal('fetch', vi.fn(async () => new Response('[1,2]')));
    expect(await (await load())('abating_link')).toBeNull();
    vi.stubGlobal('fetch', vi.fn(async () => { throw new Error('offline'); }));
    expect(await (await load())('abating_link')).toBeNull();
  });

  it('asks the desktop art protocol by slug and never fetches the index', async () => {
    const fetch = vi.fn();
    vi.stubGlobal('fetch', fetch);
    (globalThis as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {
      convertFileSrc: (path: string, protocol: string) => `${protocol}://localhost/${encodeURIComponent(path)}`,
    };
    const itemArtUrl = await load();
    expect(await itemArtUrl('abating_link')).toBe('art://localhost/abating_link');
    expect(fetch).not.toHaveBeenCalled();
  });
});
