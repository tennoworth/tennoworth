//! Which owned riven a trade sold. The game's trade dialog names a riven by its
//! weapon and a generated word ("Acceltra Critacan"), never by its stats, so a
//! sale is matched against the rivens the last scan held for that weapon. When
//! those rivens disagree on their verdict the sale is not attributed: a price
//! under the wrong verdict would mislead more than a missing one.

use super::{
    extract_rivens, resolve_rivens, resolve_stats, roll_guide, RivenAttributeRef, RivenStatTag,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The market snapshot's riven surface, as far as a verdict needs it.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RivenRefs {
    /// WFM weapon slug to its entry, in manifest order.
    #[serde(default)]
    pub weapons: serde_json::Map<String, Value>,
    #[serde(default)]
    pub attributes: Vec<RivenAttributeRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SaleVerdict {
    Sellable,
    Almost,
    Weak,
    /// No guide entry for the weapon, or a stat the app cannot name.
    Unrated,
}

/// An unveiled riven the scan held, reduced to what a sale report may carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldRiven {
    /// WFM weapon slug.
    pub weapon: String,
    /// The weapon's display name, which starts the riven's in-game name.
    pub name: String,
    pub verdict: SaleVerdict,
}

/// The rivens in a raw inventory whose weapon the market knows, with their
/// roll verdicts. Veiled rivens have no weapon yet and are left out.
pub fn held_rivens(inventory: &Value, refs: &RivenRefs) -> Vec<HeldRiven> {
    let Some(upgrades) = inventory.get("Upgrades") else {
        return Vec::new();
    };
    let owned = extract_rivens(upgrades);
    let slugs: Vec<&String> = refs.weapons.keys().collect();
    let game_refs: Vec<Option<String>> = refs
        .weapons
        .values()
        .map(|w| w.get("game_ref").and_then(Value::as_str).map(str::to_owned))
        .collect();
    let compat: Vec<super::RivenCompat> = owned
        .iter()
        .map(|r| super::RivenCompat {
            compat: r.compat.clone(),
            veiled: r.veiled,
        })
        .collect();
    let tags = |stats: &[super::RivenFingerprintStat]| -> Vec<RivenStatTag> {
        stats
            .iter()
            .map(|s| RivenStatTag { tag: s.tag.clone() })
            .collect()
    };
    resolve_rivens(&compat, &game_refs)
        .into_iter()
        .zip(&owned)
        .filter_map(|(index, riven)| {
            let slug = slugs.get(index?)?;
            let name = refs.weapons.get(*slug)?.get("name")?.as_str()?;
            let stats = resolve_stats(&tags(&riven.buffs), &tags(&riven.curses), &refs.attributes);
            let verdict = match roll_guide::roll_check(Some(slug), riven.veiled, &stats)? {
                roll_guide::RollCheck::Read(read) => match read.verdict {
                    roll_guide::RollVerdict::Sellable => SaleVerdict::Sellable,
                    roll_guide::RollVerdict::Almost => SaleVerdict::Almost,
                    roll_guide::RollVerdict::Weak => SaleVerdict::Weak,
                },
                roll_guide::RollCheck::NoGuide | roll_guide::RollCheck::UnknownStat => {
                    SaleVerdict::Unrated
                }
            };
            Some(HeldRiven {
                weapon: (*slug).clone(),
                name: name.to_owned(),
                verdict,
            })
        })
        .collect()
}

/// The generated word after the weapon: letters with at most one hyphen, as
/// in "Critacan" or "Crita-satiata".
fn is_riven_word(word: &str) -> bool {
    let mut parts = word.split('-');
    let ok =
        |p: Option<&str>| p.is_some_and(|p| !p.is_empty() && p.chars().all(char::is_alphabetic));
    ok(parts.next()) && parts.next().is_none_or(|p| ok(Some(p))) && parts.next().is_none()
}

/// The weapon and verdict of the held riven a traded item names, or None when
/// the name is not one of them or the weapon's rivens disagree on a verdict.
/// The longest weapon name wins, so "Kuva Bramma" is not read as "Bramma".
pub fn sold_riven(item: &str, held: &[HeldRiven]) -> Option<(String, SaleVerdict)> {
    let mut name = item.trim().to_lowercase();
    // The dialog may follow a mod's name with its kind or rank in brackets.
    if let Some(open) = name.rfind(" (") {
        if name.ends_with(')') {
            name.truncate(open);
        }
    }
    let weapon = held
        .iter()
        .filter(|h| {
            let prefix = format!("{} ", h.name.to_lowercase());
            name.strip_prefix(&prefix).is_some_and(is_riven_word)
        })
        .max_by_key(|h| h.name.len())?;
    let mut verdicts = held
        .iter()
        .filter(|h| h.weapon == weapon.weapon)
        .map(|h| h.verdict);
    let first = verdicts.next()?;
    verdicts
        .all(|v| v == first)
        .then(|| (weapon.weapon.clone(), first))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn held(weapon: &str, name: &str, verdict: SaleVerdict) -> HeldRiven {
        HeldRiven {
            weapon: weapon.into(),
            name: name.into(),
            verdict,
        }
    }

    #[test]
    fn a_riven_name_finds_its_weapon_and_verdict() {
        let held = [
            held("acceltra", "Acceltra", SaleVerdict::Sellable),
            held("bramma", "Bramma", SaleVerdict::Weak),
            held("kuva_bramma", "Kuva Bramma", SaleVerdict::Almost),
        ];
        let sold = |item| sold_riven(item, &held);
        assert_eq!(
            sold("Acceltra Critacan"),
            Some(("acceltra".into(), SaleVerdict::Sellable))
        );
        assert_eq!(
            sold("  acceltra crita-satiata (Riven Mod) "),
            Some(("acceltra".into(), SaleVerdict::Sellable))
        );
        assert_eq!(
            sold("Kuva Bramma Visitox"),
            Some(("kuva_bramma".into(), SaleVerdict::Almost))
        );
        assert_eq!(
            sold("Bramma Visitox"),
            Some(("bramma".into(), SaleVerdict::Weak))
        );
    }

    #[test]
    fn anything_else_is_not_attributed() {
        let held = [
            held("acceltra", "Acceltra", SaleVerdict::Sellable),
            held("acceltra", "Acceltra", SaleVerdict::Weak),
            held("soma", "Soma", SaleVerdict::Sellable),
        ];
        let sold = |item| sold_riven(item, &held);
        assert_eq!(
            sold("Acceltra Critacan"),
            None,
            "two verdicts for one weapon"
        );
        assert_eq!(sold("Soma Prime Barrel"), None, "an item, not a riven");
        assert_eq!(sold("Soma"), None);
        assert_eq!(sold("Soma Crita-sati-ata"), None);
        assert_eq!(sold("Braton Critacan"), None, "no Braton riven was held");
        assert_eq!(sold_riven("Soma Critacan", &[]), None);
    }

    #[test]
    fn a_scan_gives_each_unveiled_riven_its_verdict() {
        let refs: RivenRefs = serde_json::from_value(json!({
            "weapons": {
                "acceltra": {"name": "Acceltra", "game_ref": "/Lotus/Weapons/Tenno/LongGuns/Acceltra/Acceltra"},
                "dex_nikana": {"name": "Dex Nikana", "game_ref": "/Lotus/Weapons/Tenno/Melee/Swords/DexNikana"},
            },
            "attributes": [
                {"game_ref": "WeaponCritDamageMod", "slug": "critical_damage", "name": "Critical Damage"},
                {"game_ref": "WeaponFireIterationsMod", "slug": "multishot", "name": "Multishot"},
                {"game_ref": "WeaponCritChanceMod", "slug": "critical_chance", "name": "Critical Chance"},
            ],
        }))
        .unwrap();
        let riven = |compat: &str, buffs: &[&str]| {
            let fingerprint = json!({
                "compat": compat,
                "buffs": buffs.iter().map(|t| json!({"Tag": t, "Value": 1})).collect::<Vec<_>>(),
                "curses": [],
            });
            json!({"ItemType": "/Lotus/Upgrades/Mods/Randomized/LotusRifleRandomModRare", "UpgradeFingerprint": fingerprint.to_string()})
        };
        let inventory = json!({"Upgrades": [
            riven("/Lotus/Weapons/Tenno/LongGuns/Acceltra/Acceltra", &["WeaponCritChanceMod", "WeaponCritDamageMod", "WeaponFireIterationsMod"]),
            riven("/Lotus/Weapons/Tenno/Melee/Swords/DexNikana", &["WeaponCritDamageMod"]),
            riven("/Lotus/Weapons/Tenno/LongGuns/Acceltra/Acceltra", &["NotARealTag"]),
            {"ItemType": "/Lotus/Upgrades/Mods/Randomized/LotusRifleRandomModRare", "UpgradeFingerprint": "{\"challenge\":{}}"},
            riven("/Lotus/Weapons/Brand/New", &["WeaponCritDamageMod"]),
        ]});
        let got = held_rivens(&inventory, &refs);
        assert_eq!(
            got.len(),
            3,
            "veiled and unknown-weapon rivens are left out: {got:?}"
        );
        assert_eq!(got[0].weapon, "acceltra");
        assert_ne!(
            got[0].verdict,
            SaleVerdict::Unrated,
            "the guide covers Acceltra"
        );
        assert_eq!(
            got[1],
            held("dex_nikana", "Dex Nikana", SaleVerdict::Unrated),
            "no guide entry"
        );
        assert_eq!(
            got[2].verdict,
            SaleVerdict::Unrated,
            "a stat the manifest cannot name"
        );
        assert!(held_rivens(&json!({}), &refs).is_empty());
    }
}
