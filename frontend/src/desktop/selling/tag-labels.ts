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

const words = (text: string) => text.toLowerCase().split(/[\s_-]+/).filter(Boolean);

/** Whether `query` (as typed: spaces, hyphens, any case) finds the tag: its
 *  letters run on inside the name ("jatk"), or each typed word starts a
 *  different word of the name, in any order ("kit jat", "j k"). */
export function tagMatches(tag: string, query: string): boolean {
  const flat = (text: string) => text.toLowerCase().replace(/[\s_-]+/g, '');
  const q = flat(query);
  if (!q || flat(tag).includes(q) || flat(tagLabel(tag)).includes(q)) return true;
  const typed = words(query);
  return [words(tag), words(tagLabel(tag))].some((name) => {
    const free = [...name];
    return typed.every((word) => {
      const at = free.findIndex((part) => part.startsWith(word));
      if (at < 0) return false;
      free.splice(at, 1);
      return true;
    });
  });
}
