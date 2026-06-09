// Resolves /Lotus/... internal paths to a warframe.market slug.
//
// path -> display name comes from wfstat-catalog.json, baked at build
// time by csv_to_market_json.py and served same-origin. It used to be a
// direct warframestat.us fetch, but upstream dropped its CORS headers
// (2026-06-09) — and the direct fetch also varied on Accept-Language,
// so non-English browsers got localized names that matched nothing on
// WFM. name -> WFM slug comes from the catalog baked into market.json.

import { readCached, writeCached, type SlimCatalog } from './catalog-cache';
import type { Market, ResolvedItem, SlimItemInfo } from './types';

const WFSTAT_CATALOG_URL = '/wfstat-catalog.json';

export interface Catalogs {
  uniqueToInfo: Map<string, SlimItemInfo>;
}

export async function loadCatalogs(): Promise<Catalogs> {
  // Cheap path: IndexedDB cache, 24 h TTL. The slim form holds only the
  // (uniqueName, name, category) triples (~17k entries) we actually need —
  // keeps the stored payload to a fraction of warframestat.us's ~5 MB raw.
  const cached = await readCached();
  if (cached && Array.isArray(cached)) {
    return { uniqueToInfo: new Map(cached) };
  }

  const r = await fetch(WFSTAT_CATALOG_URL);
  if (!r.ok) throw new Error(`wfstat-catalog.json responded ${r.status} — rebuild the snapshot (csv_to_market_json.py)`);
  // Already in slim [uniqueName, {name, category}] form — baked that way.
  const slim = (await r.json()) as SlimCatalog;
  if (!Array.isArray(slim)) throw new Error('wfstat-catalog.json is not an array');

  // Fire-and-forget; we don't want to block first paint on the write.
  void writeCached(slim);
  return { uniqueToInfo: new Map(slim) };
}

function slugGuess(name: string): string {
  return name
    .replace(/[^a-zA-Z0-9 ]/g, '')
    .trim()
    .toLowerCase()
    .replace(/\s+/g, '_');
}

// Refinement levels in warframestat.us relic names. All four share a WFM
// slug (axi_k2_relic) but WFM lists them as distinct subtypes when
// creating an order. We preserve the refinement so each variant is a
// separate inventory row and the right subtype reaches the companion at
// listing time.
const REFINEMENTS = new Set(['Intact', 'Exceptional', 'Flawless', 'Radiant']);

function resolveRelic(name: string, market: Market | null | undefined): ResolvedItem | null {
  const parts = name.split(' ');
  if (parts.length < 2) return null;
  const last = parts[parts.length - 1];
  if (!REFINEMENTS.has(last)) return null;
  const base = parts.slice(0, -1).join(' ');
  const lookup = `${base.toLowerCase()} relic`;
  const slug = market?.catalog?.[lookup];
  if (!slug) return null;
  return {
    name: `${base} Relic (${last})`,
    slug,
    category: 'Relics',
    subtype: last.toLowerCase(),
  };
}

export function resolvePath(
  path: string,
  catalogs: Catalogs,
  market: Market | null | undefined,
): ResolvedItem {
  // Prime-part components (chassis / systems / weapon barrels / …) live
  // ONLY nested under their parent items in warframestat — the bulk
  // /items/ endpoint omits them. The scraper pre-walks parent categories
  // and bakes a path → {name, slug, category} map into market.json so
  // these resolve directly. Check it first; if found, short-circuit.
  const direct = market?.path_to_info?.[path];
  if (direct) {
    return { name: direct.name, slug: direct.slug, category: direct.category, subtype: null };
  }

  let info = catalogs.uniqueToInfo.get(path);
  if (!info) {
    for (const suffix of ['Component', 'Blueprint']) {
      if (path.endsWith(suffix)) {
        const trimmed = path.slice(0, -suffix.length);
        const candidate = catalogs.uniqueToInfo.get(trimmed);
        if (candidate) {
          info = candidate;
          break;
        }
      }
    }
  }
  if (!info) return { name: null, slug: null, category: null, subtype: null };
  const { name, category } = info;

  // Relics carry a refinement (Intact / Exceptional / Flawless / Radiant)
  // that's lost if we collapse on slug alone — radiant relics sell for
  // multiples of intact, and WFM rejects listings missing the subtype.
  const relic = resolveRelic(name, market);
  if (relic) return relic;

  const slug =
    market?.catalog?.[name.toLowerCase()] ?? slugGuess(name);
  return { name, slug, category, subtype: null };
}
