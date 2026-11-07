// How a warframe.market tag reads on the Sell view. Category tags read as
// words; tags naming a Warframe, weapon or companion read as the game writes
// the name. The tags are the market's English vocabulary in every language.
import { tagGroup } from '../../domain/tag-groups';

const SPELLED: Record<string, string> = {
  arcane_enhancement: 'arcane',
  ax_52: 'Ax-52',
  efv_5_jupiter: 'EFV-5 Jupiter',
  efv_8_mars: 'EFV-8 Mars',
  ack_and_brunt: 'Ack & Brunt',
  silva_and_aegis: 'Silva & Aegis',
  vesper_77: 'Vesper 77',
};

export function tagLabel(tag: string): string {
  if (SPELLED[tag]) return SPELLED[tag];
  const words = tag.split('_');
  if (tagGroup(tag) !== 'name') return words.join(' ');
  return words.map((word) => word.charAt(0).toUpperCase() + word.slice(1)).join(' ');
}

/** Whether `query` (as typed: spaces, hyphens, any case) finds the tag. */
export function tagMatches(tag: string, query: string): boolean {
  const flat = (text: string) => text.toLowerCase().replace(/[\s_-]+/g, '');
  const q = flat(query);
  return !q || flat(tag).includes(q) || flat(tagLabel(tag)).includes(q);
}
