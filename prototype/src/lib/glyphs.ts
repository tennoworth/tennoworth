// Item glyphs, drawn by us.
//
// DE publishes an icon for nearly every item in the game and we are not using
// them: the Fan Content Policy allows game assets only in non-commercial work,
// which would make the art the first thing to rip out if this project ever
// changed footing, and a mirrored PNG per item is real disk on the box for a
// row that is 20px tall. So these are original marks — one per category, not
// one per item.
//
// House style, matching the app's angular theme: 24×24 box, stroke-only,
// `currentColor`, 1.5 stroke, mitred joins, square caps, **no curves and no
// rounded corners**. They read at 16px, which is the only size that matters.
//
// A glyph identifies a KIND of thing. It is never the only carrier of meaning
// — every row that shows one also shows its name — so a reader who cannot
// distinguish two marks loses nothing.

export type GlyphName =
  | 'warframe'
  | 'primary'
  | 'secondary'
  | 'melee'
  | 'sentinel'
  | 'archwing'
  | 'mod'
  | 'riven'
  | 'relic'
  | 'arcane'
  | 'set'
  | 'resource'
  | 'ducat'
  | 'credit'
  | 'plat'
  | 'unknown';

/** Path data for each glyph, in a 24×24 box. */
export const GLYPH_PATHS: Record<GlyphName, string> = {
  // A helmet: tapered crown over a visor band.
  warframe: 'M6 9 L12 3 L18 9 L18 16 L12 21 L6 16 Z M6 12 H18',
  // A rifle: long barrel, fore grip, angled stock.
  primary: 'M3 9 H21 V12 H14 L12 15 H9 L7 12 H3 Z M17 12 V15',
  // A pistol: short slide over a canted grip.
  secondary: 'M5 8 H17 V12 H12 L10 18 H6 L8 12 H5 Z',
  // A blade: edge, guard, hilt.
  melee: 'M5 19 L15 5 L18 8 L8 19 Z M4 21 L8 19 M13 7 L17 3',
  // A drone: body diamond with two stub wings.
  sentinel: 'M12 6 L16 12 L12 18 L8 12 Z M4 12 H8 M16 12 H20',
  // Wings: two swept panels from a central spine.
  archwing: 'M12 4 V20 M12 8 L3 12 L12 14 M12 8 L21 12 L12 14',
  // A mod card: rectangle with the polarity notch cut into the top edge.
  mod: 'M6 4 H10 L12 6 L14 4 H18 V20 H6 Z M6 9 H18',
  // A mod card with the roll slashed across it.
  riven: 'M6 4 H10 L12 6 L14 4 H18 V20 H6 Z M8 18 L16 7',
  // A void relic: faceted capsule.
  relic: 'M12 2 L19 7 V17 L12 22 L5 17 V7 Z M5 7 L12 12 L19 7 M12 12 V22',
  // An arcane: hexagon with an orbiting mark.
  arcane: 'M12 4 L18 8 V16 L12 20 L6 16 V8 Z M12 9 L15 12 L12 15 L9 12 Z',
  // A set: three stacked plates, the front one complete.
  set: 'M4 8 H14 V18 H4 Z M7 5 H17 V15 M10 2 H20 V12',
  // A resource: an isometric cube.
  resource: 'M12 3 L21 8 V16 L12 21 L3 16 V8 Z M3 8 L12 13 L21 8 M12 13 V21',
  // A ducat: octagonal coin with a struck centre.
  ducat: 'M9 3 H15 L21 9 V15 L15 21 H9 L3 15 V9 Z M10 10 H14 V14 H10 Z',
  // Credits: a stamped bar with two ledger rules.
  credit: 'M3 6 H21 V18 H3 Z M7 10 H17 M7 14 H13',
  // Platinum: the trade currency — a faceted lozenge.
  plat: 'M12 3 L20 12 L12 21 L4 12 Z M8 12 H16',
  // Unknown: a plain plate, so a missing mapping still lines up in the column.
  unknown: 'M5 5 H19 V19 H5 Z',
};

/**
 * Map a snapshot category or tag to a glyph.
 *
 * Input is whatever the row carries — `path_to_info.category` ("Warframes",
 * "Melee") or a WFM tag ("mod", "relic", "prime"). Case and plural are
 * normalised because the two sources disagree on both. Anything unrecognised
 * gets `unknown` rather than a guess: an item wearing the wrong mark is worse
 * than one wearing a blank plate.
 */
export function glyphFor(category: string | null | undefined): GlyphName {
  if (!category) return 'unknown';
  const key = category.trim().toLowerCase().replace(/s$/, '');
  switch (key) {
    case 'warframe':
    case 'powersuit':
      return 'warframe';
    case 'primary':
    case 'rifle':
    case 'shotgun':
    case 'bow':
      return 'primary';
    case 'secondary':
    case 'pistol':
      return 'secondary';
    case 'melee':
    case 'zaw':
      return 'melee';
    case 'sentinel':
    case 'companion':
    case 'pet':
      return 'sentinel';
    case 'archwing':
    case 'arch-gun':
    case 'arch-melee':
      return 'archwing';
    case 'mod':
      return 'mod';
    case 'riven':
      return 'riven';
    case 'relic':
      return 'relic';
    case 'arcane':
      return 'arcane';
    case 'set':
      return 'set';
    case 'resource':
    case 'component':
    case 'misc':
      return 'resource';
    default:
      return 'unknown';
  }
}

/**
 * Pick a glyph from a WFM tag list.
 *
 * Tags are unordered and an item usually carries several ("prime", "set",
 * "warframe"), so the specific ones are tested before the generic — a prime
 * set should read as a set, not as a warframe.
 */
export function glyphForTags(tags: string[] | undefined): GlyphName {
  if (!tags?.length) return 'unknown';
  const has = (t: string) => tags.some((x) => x.toLowerCase() === t);
  if (has('riven')) return 'riven';
  if (has('relic')) return 'relic';
  if (has('arcane')) return 'arcane';
  if (has('set')) return 'set';
  if (has('mod')) return 'mod';
  for (const tag of tags) {
    const g = glyphFor(tag);
    if (g !== 'unknown') return g;
  }
  return 'unknown';
}
