// warframe.market's item tags, grouped the way the game thinks about items.
// The vocabularies and the canonical form are native (market_domain::tags);
// this file keeps the order the groups are offered in.

import type { AvailableTag, TagGroup } from '../contracts/generated/web';
import { call, jsNumber } from './native';

export type { TagGroup };

/** The order the groups are offered in. */
export const TAG_GROUPS: readonly TagGroup[] = ['type', 'slot', 'rarity', 'era', 'companion', 'name'];

// A tag's group never changes, and the chip row asks for each tag on every
// render; the chip counts' answer fills this in one call per list.
const groups = new Map<string, TagGroup>();

export function rememberTagGroups(tags: readonly AvailableTag[]): void {
  for (const { tag, group } of tags) groups.set(tag, group);
}

export function tagGroup(tag: string): TagGroup {
  let group = groups.get(tag);
  if (group === undefined) {
    group = call({ op: 'tag_groups', input: [tag] })[0]!;
    groups.set(tag, group);
  }
  return group;
}

/** The tags worth a place on the Sell view's single line: the broadest item
 *  types and slots, never rarity (most mods are rare) and never names. Expects
 *  tags sorted by count, as computeAvailableTags returns them. */
export function topTags(available: ReadonlyArray<readonly [string, number]>, limit?: number): string[] {
  if (available.length === 0) return [];
  return call({ op: 'top_tags', input: {
    available: available.map(([tag, count]): [string, number] => [tag, count]),
    ...(limit === undefined ? {} : { limit: jsNumber(limit) }),
  } });
}
