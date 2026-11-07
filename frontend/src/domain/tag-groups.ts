// warframe.market's item tags, sorted the way the game thinks about items.
// Besides categories, the market tags a mod with whatever it belongs to (the
// Nyx augments carry `nyx`, Grakata's exclusive mods `grakata`, a companion's
// precepts its breed), so a large inventory carries some 150 tags, most of
// them names. Anything outside the vocabularies below is such a name.

export type TagGroup = 'type' | 'slot' | 'rarity' | 'era' | 'companion' | 'name';

const words = (text: string) => text.trim().split(/\s+/);

const VOCABULARY: Record<Exclude<TagGroup, 'name'>, string[]> = {
  type: words(`mod prime weapon component warframe blueprint relic set augment arcane_enhancement
    arcane_helmet archwing archgun archmelee necramech railjack k_drive parazon sentinel companion pet
    imprint syndicate stance aura riven riven_mod veiled_riven peculiar any pvp focus lens fish gem
    skin emote scene misc tome ayatan_sculpture ayatan_star operator damaged locator key void_key
    sparring fusion_core mech collectible unique beacon modular triangulator helminth_charger`),
  slot: words(`primary secondary melee rifle shotgun pistol sniper bow assault_rifle rifle_no_aoe
    pistol_no_aoe thrown thrown_melee claws daggers dual_daggers dark_dagger dual_swords glaives
    gunblade hammers heavy_blade heavy_scythe machetes nikanas two_handed_nikana polearms rapiers
    scythes staves swords sword_and_shield blade_and_whip tonfas warfans whips bayonet
    kubrow_claws kavat_claws helminth_claws`),
  rarity: words('common uncommon rare legendary'),
  era: words('lith meso neo axi requiem'),
  companion: words('kubrow kavat vulpaphyla predasite hound moa robotic beast'),
};

const GROUP = new Map<string, TagGroup>();
for (const [group, tags] of Object.entries(VOCABULARY)) for (const tag of tags) GROUP.set(tag, group as TagGroup);

/** The order the groups are offered in. */
export const TAG_GROUPS: readonly TagGroup[] = ['type', 'slot', 'rarity', 'era', 'companion', 'name'];

// Breeds fold into their species: a Huras Kubrow's precepts are kubrow mods.
const BREED = /^[a-z]+_(kubrow|kavat|vulpaphyla|predasite)$/;

/** The tag a filter works on. A few market tags use a space ("void key"). */
export function canonicalTag(tag: string): string {
  const snake = tag.trim().toLowerCase().replace(/\s+/g, '_');
  return snake.match(BREED)?.[1] ?? snake;
}

export function canonicalTags(tags: readonly string[] | undefined): string[] {
  return [...new Set((tags ?? []).map(canonicalTag))];
}

export function tagGroup(tag: string): TagGroup {
  return GROUP.get(canonicalTag(tag)) ?? 'name';
}

// "weapon" spans primary, secondary and melee, so it narrows less than they do.
const NOT_ON_TOP = new Set(['weapon']);

/** The tags worth a place on the Sell view's single line: the broadest item
 *  types and slots, never rarity (most mods are rare) and never names. Expects
 *  tags sorted by count, as computeAvailableTags returns them. */
export function topTags(available: ReadonlyArray<readonly [string, number]>, limit = 12): string[] {
  return available
    .filter(([tag, count]) => count > 0 && !NOT_ON_TOP.has(tag) && (tagGroup(tag) === 'type' || tagGroup(tag) === 'slot'))
    .slice(0, limit)
    .map(([tag]) => tag);
}
