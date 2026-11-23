//! warframe.market's item tags, sorted the way the game thinks about items.
//! Besides categories, the market tags a mod with whatever it belongs to (the
//! Nyx augments carry `nyx`, Grakata's exclusive mods `grakata`, a companion's
//! precepts its breed), so a large inventory carries some 150 tags, most of
//! them names. Anything outside the vocabularies below is such a name.

use crate::js;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;
use ts_rs::TS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum TagGroup {
    Type,
    Slot,
    Rarity,
    Era,
    Companion,
    Name,
}

const TYPE: &str = "mod prime weapon component warframe blueprint relic set augment arcane_enhancement
    arcane_helmet archwing archgun archmelee necramech railjack k_drive parazon sentinel companion pet
    imprint syndicate stance aura riven riven_mod veiled_riven peculiar any pvp focus lens fish gem
    skin emote scene misc tome ayatan_sculpture ayatan_star operator damaged locator key void_key
    sparring fusion_core mech collectible unique beacon modular triangulator helminth_charger";
const SLOT: &str =
    "primary secondary melee rifle shotgun pistol sniper bow assault_rifle rifle_no_aoe
    pistol_no_aoe thrown thrown_melee claws daggers dual_daggers dark_dagger dual_swords glaives
    gunblade hammers heavy_blade heavy_scythe machetes nikanas two_handed_nikana polearms rapiers
    scythes staves swords sword_and_shield blade_and_whip tonfas warfans whips bayonet
    kubrow_claws kavat_claws helminth_claws";
const RARITY: &str = "common uncommon rare legendary";
const ERA: &str = "lith meso neo axi requiem";
const COMPANION: &str = "kubrow kavat vulpaphyla predasite hound moa robotic beast";

// Breeds fold into their species: a Huras Kubrow's precepts are kubrow mods.
const SPECIES: [&str; 4] = ["kubrow", "kavat", "vulpaphyla", "predasite"];

// "weapon" spans primary, secondary and melee, so it narrows less than they do.
const NOT_ON_TOP: &str = "weapon";

/// How many tags the Sell view's single line holds when the caller names no limit.
pub const TOP_TAGS: f64 = 12.0;

/// The tag a filter works on. A few market tags use a space ("void key").
pub fn canonical_tag(tag: &str) -> String {
    // Whole-string lowercasing, as `toLowerCase()`: a final sigma depends on
    // its neighbours.
    let lower = tag.trim_matches(js::is_space).to_lowercase();
    let mut snake = String::with_capacity(lower.len());
    let mut in_space = false;
    for c in lower.chars() {
        if js::is_space(c) {
            if !in_space {
                snake.push('_');
            }
            in_space = true;
        } else {
            snake.push(c);
            in_space = false;
        }
    }
    for species in SPECIES {
        if let Some(breed) = snake
            .strip_suffix(species)
            .and_then(|rest| rest.strip_suffix('_'))
        {
            if !breed.is_empty() && breed.bytes().all(|b| b.is_ascii_lowercase()) {
                return species.to_owned();
            }
        }
    }
    snake
}

/// Each tag once, canonical, in first-seen order.
pub fn canonical_tags<S: AsRef<str>>(tags: &[S]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(tags.len());
    for tag in tags {
        let tag = canonical_tag(tag.as_ref());
        if !out.contains(&tag) {
            out.push(tag);
        }
    }
    out
}

// The chip row groups every tag on every render, so the vocabularies are read
// into one map once rather than scanned per tag.
fn vocabulary() -> &'static HashMap<&'static str, TagGroup> {
    static GROUPS: OnceLock<HashMap<&'static str, TagGroup>> = OnceLock::new();
    GROUPS.get_or_init(|| {
        let mut groups = HashMap::new();
        for (words, group) in [
            (TYPE, TagGroup::Type),
            (SLOT, TagGroup::Slot),
            (RARITY, TagGroup::Rarity),
            (ERA, TagGroup::Era),
            (COMPANION, TagGroup::Companion),
        ] {
            for word in words.split_whitespace() {
                groups.insert(word, group);
            }
        }
        groups
    })
}

fn group_of_canonical(tag: &str) -> TagGroup {
    vocabulary().get(tag).copied().unwrap_or(TagGroup::Name)
}

pub fn tag_group(tag: &str) -> TagGroup {
    group_of_canonical(&canonical_tag(tag))
}

/// The tags worth a place on the Sell view's single line: the broadest item
/// types and slots, never rarity (most mods are rare) and never names. Expects
/// tags sorted by count, as the chip row sorts them.
pub fn top_tags(available: &[(String, f64)], limit: f64) -> Vec<String> {
    let mut top: Vec<String> = available
        .iter()
        .filter(|(tag, count)| {
            *count > 0.0
                && tag != NOT_ON_TOP
                && matches!(tag_group(tag), TagGroup::Type | TagGroup::Slot)
        })
        .map(|(tag, _)| tag.clone())
        .collect();
    top.truncate(js::slice_index(limit, top.len()));
    top
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_every_breed_into_its_species_and_nothing_else() {
        assert_eq!(
            canonical_tags(&[
                "mod",
                "huras_kubrow",
                "kubrow",
                "smeeta_kavat",
                "panzer_vulpaphyla"
            ]),
            ["mod", "kubrow", "kavat", "vulpaphyla"]
        );
        assert_eq!(canonical_tag("kubrow_claws"), "kubrow_claws");
        assert_eq!(canonical_tag("void key"), "void_key");
        assert_eq!(canonical_tag("  Void \t\u{feff} Key "), "void_key");
        assert_eq!(canonical_tag("_kubrow"), "_kubrow");
        assert_eq!(canonical_tag("two_word_kubrow"), "two_word_kubrow");
        assert_eq!(canonical_tag("helminth_predasite"), "predasite");
    }

    #[test]
    fn sorts_tags_the_way_the_game_does_with_names_last() {
        assert_eq!(tag_group("relic"), TagGroup::Type);
        assert_eq!(tag_group("nikanas"), TagGroup::Slot);
        assert_eq!(tag_group("rare"), TagGroup::Rarity);
        assert_eq!(tag_group("axi"), TagGroup::Era);
        assert_eq!(tag_group("sunika_kubrow"), TagGroup::Companion);
        assert_eq!(tag_group("nyx"), TagGroup::Name);
        assert_eq!(tag_group("ax_52"), TagGroup::Name);
        assert_eq!(tag_group("Void Key"), TagGroup::Type);
        assert_eq!(
            canonical_tag("ΟΔΟΣ ΟΔΟΣ"),
            "\u{3bf}\u{3b4}\u{3bf}\u{3c2}_\u{3bf}\u{3b4}\u{3bf}\u{3c2}"
        );
    }

    #[test]
    fn keeps_rarity_names_and_the_catch_all_weapon_off_the_main_line() {
        let available: Vec<(String, f64)> = [
            ("mod", 321.0),
            ("rare", 240.0),
            ("warframe", 127.0),
            ("weapon", 68.0),
            ("melee", 46.0),
            ("nyx", 40.0),
            ("relic", 16.0),
            ("empty", 0.0),
        ]
        .iter()
        .map(|(t, n)| ((*t).to_owned(), *n))
        .collect();
        assert_eq!(
            top_tags(&available, TOP_TAGS),
            ["mod", "warframe", "melee", "relic"]
        );
        assert_eq!(top_tags(&available, 2.0), ["mod", "warframe"]);
        assert_eq!(top_tags(&available, -1.0), ["mod", "warframe", "melee"]);
    }
}
