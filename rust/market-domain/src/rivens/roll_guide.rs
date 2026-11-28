//! The riven roll guide: which stat combinations buyers look for on each
//! weapon, and which negatives do no harm there. The data is
//! [roll-guide.json](roll-guide.json), keyed by WFM weapon slug and written in
//! the trade shorthand (CD, MS, DMG, ...) its `stats` table maps to WFM
//! attribute slugs. It started from 44bananas' guide (ATTRIBUTIONS.md) and is
//! changed through pull requests (CONTRIBUTING.md, Riven roll guide).
//!
//! A verdict judges which stats a riven has, never how strong each roll is:
//! the inventory fingerprint cannot give display values honestly (see
//! [`super::RivenStatRead`]).

use super::RivenStatRead;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;
use ts_rs::TS;

const GUIDE_JSON: &str = include_str!("roll-guide.json");

#[derive(Debug, Deserialize)]
struct GuideFile {
    stats: BTreeMap<String, String>,
    weapons: BTreeMap<String, WeaponFile>,
}

#[derive(Debug, Deserialize)]
struct WeaponFile {
    builds: Vec<BuildFile>,
    #[serde(default)]
    harmless: Vec<String>,
    #[serde(default)]
    note: Option<String>,
    source: String,
    checked: String,
}

#[derive(Debug, Deserialize)]
struct BuildFile {
    #[serde(default)]
    need: Vec<String>,
    #[serde(default)]
    pick: Vec<String>,
}

/// One combination buyers look for, as WFM slugs in the guide's order. FR and
/// AS are one WFM stat, so a slug appears once.
#[derive(Debug)]
struct Build {
    need: Vec<String>,
    pick: Vec<String>,
}

impl Build {
    fn wants(&self, slug: &str) -> bool {
        self.need.iter().chain(&self.pick).any(|s| s == slug)
    }
}

#[derive(Debug)]
struct WeaponGuide {
    builds: Vec<Build>,
    harmless: BTreeSet<String>,
    note: Option<String>,
    source: String,
    checked: String,
}

#[derive(Debug)]
pub(crate) struct Guide {
    weapons: BTreeMap<String, WeaponGuide>,
}

fn push_unique(into: &mut Vec<String>, slug: &str) {
    if !into.iter().any(|s| s == slug) {
        into.push(slug.to_string());
    }
}

/// Reads the guide, refusing an abbreviation its `stats` table does not define:
/// a typo would otherwise drop a stat silently and misjudge every riven on
/// that weapon.
pub(crate) fn parse(text: &str) -> Result<Guide, String> {
    let file: GuideFile = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let slug_of = |weapon: &str, abbr: &str| {
        file.stats
            .get(abbr)
            .cloned()
            .ok_or_else(|| format!("{weapon}: unknown stat {abbr}"))
    };
    let mut weapons = BTreeMap::new();
    for (weapon, entry) in &file.weapons {
        let mut builds = Vec::new();
        for build in &entry.builds {
            let mut need = Vec::new();
            for abbr in &build.need {
                push_unique(&mut need, &slug_of(weapon, abbr)?);
            }
            let mut pick = Vec::new();
            for abbr in &build.pick {
                let slug = slug_of(weapon, abbr)?;
                if !need.contains(&slug) {
                    push_unique(&mut pick, &slug);
                }
            }
            builds.push(Build { need, pick });
        }
        let harmless = entry
            .harmless
            .iter()
            .map(|abbr| slug_of(weapon, abbr))
            .collect::<Result<_, _>>()?;
        weapons.insert(
            weapon.clone(),
            WeaponGuide {
                builds,
                harmless,
                note: entry.note.clone().filter(|n| !n.trim().is_empty()),
                source: entry.source.clone(),
                checked: entry.checked.clone(),
            },
        );
    }
    Ok(Guide { weapons })
}

fn guide() -> Option<&'static Guide> {
    static GUIDE: OnceLock<Option<Guide>> = OnceLock::new();
    GUIDE.get_or_init(|| parse(GUIDE_JSON).ok()).as_ref()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum RollVerdict {
    /// A combination buyers look for, with a harmless negative or none.
    Sellable,
    /// One thing short: a missing required stat, an unwanted third positive or
    /// a negative that hurts.
    Almost,
    Weak,
}

/// A riven read against the build it comes closest to. Stats are WFM slugs.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct RollRead {
    pub verdict: RollVerdict,
    /// The build's required stats, then the pool its other wanted stats come from.
    pub need: Vec<String>,
    pub pick: Vec<String>,
    /// Positives the build does not want.
    pub dead: Vec<String>,
    /// Required stats the riven lacks.
    pub missing: Vec<String>,
    /// The negative, when the guide does not call it harmless on this weapon.
    pub hurts: Option<String>,
    /// The negative, when the guide calls it harmless.
    pub harmless: Option<String>,
    /// The guide's note for the weapon, in English: build-specific caveats.
    pub note: Option<String>,
    /// Who wrote the entry and the game update it was last checked against.
    pub source: String,
    pub checked: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RollCheck {
    Read(Box<RollRead>),
    /// The guide has no entry for this weapon yet.
    NoGuide,
    /// A stat the manifest cannot name: judging without it could call a riven
    /// sellable because a trait is missing from view.
    UnknownStat,
}

struct Fit<'a> {
    build: &'a Build,
    wanted: usize,
    dead: Vec<String>,
    missing: Vec<String>,
    flaws: usize,
}

pub(crate) fn check(guide: &Guide, weapon: &str, stats: &[RivenStatRead]) -> RollCheck {
    let Some(entry) = guide.weapons.get(weapon) else {
        return RollCheck::NoGuide;
    };
    let mut positives = Vec::new();
    let mut negatives = Vec::new();
    for stat in stats {
        let Some(slug) = stat.slug.as_deref() else {
            return RollCheck::UnknownStat;
        };
        push_unique(
            if stat.positive {
                &mut positives
            } else {
                &mut negatives
            },
            slug,
        );
    }
    let hurts = negatives
        .iter()
        .find(|n| !entry.harmless.contains(*n))
        .cloned();
    let harmless = negatives
        .iter()
        .find(|n| entry.harmless.contains(*n))
        .cloned();
    // Builds are tried in the guide's order and the first fewest-flaws build is
    // kept, so equal fits report the build the guide lists first.
    let mut best: Option<Fit> = None;
    for build in &entry.builds {
        let wanted = positives.iter().filter(|p| build.wants(p)).count();
        let dead: Vec<String> = positives
            .iter()
            .filter(|p| !build.wants(p))
            .cloned()
            .collect();
        let missing: Vec<String> = build
            .need
            .iter()
            .filter(|n| !positives.contains(n))
            .cloned()
            .collect();
        let flaws = dead.len() + missing.len() + usize::from(hurts.is_some());
        let better = best
            .as_ref()
            .is_none_or(|b| flaws < b.flaws || (flaws == b.flaws && wanted > b.wanted));
        if better {
            best = Some(Fit {
                build,
                wanted,
                dead,
                missing,
                flaws,
            });
        }
    }
    let Some(fit) = best else {
        return RollCheck::NoGuide;
    };
    let verdict = match (fit.wanted >= 2, fit.flaws) {
        (true, 0) => RollVerdict::Sellable,
        (true, 1) => RollVerdict::Almost,
        _ => RollVerdict::Weak,
    };
    RollCheck::Read(Box::new(RollRead {
        verdict,
        need: fit.build.need.clone(),
        pick: fit.build.pick.clone(),
        dead: fit.dead,
        missing: fit.missing,
        hurts,
        harmless,
        note: entry.note.clone(),
        source: entry.source.clone(),
        checked: entry.checked.clone(),
    }))
}

/// The roll check for one owned riven, or None when there is nothing to judge:
/// a veiled riven, or a weapon the market could not resolve.
pub fn roll_check(
    weapon: Option<&str>,
    veiled: bool,
    stats: &[RivenStatRead],
) -> Option<RollCheck> {
    if veiled {
        return None;
    }
    let weapon = weapon?;
    Some(guide().map_or(RollCheck::NoGuide, |g| check(g, weapon, stats)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const CASES: &str = r#"{
      "stats": {"CD": "critical_damage", "CC": "critical_chance", "MS": "multishot",
                "DMG": "base_damage_/_melee_damage", "FR": "fire_rate_/_attack_speed",
                "AS": "fire_rate_/_attack_speed", "RANGE": "range", "ZOOM": "zoom",
                "IMP": "impact_damage", "REC": "recoil", "HEAT": "heat_damage",
                "COLD": "cold_damage", "VIRAL": "viral"},
      "weapons": {
        "gun": {"builds": [{"need": [], "pick": ["CD", "MS", "DMG", "CC", "FR", "VIRAL"]}],
                "harmless": ["ZOOM", "IMP"], "note": "IMP only harmless for status builds",
                "source": "44bananas", "checked": "44.1.2"},
        "blade": {"builds": [{"need": ["CD", "AS", "RANGE"], "pick": []},
                             {"need": ["HEAT"], "pick": ["CD", "RANGE", "AS"]}],
                  "harmless": ["IMP"], "source": "community", "checked": "45.0"}
      }
    }"#;

    fn stat(slug: Option<&str>, positive: bool) -> RivenStatRead {
        RivenStatRead {
            tag: slug.unwrap_or("NewDeTrait").to_string(),
            slug: slug.map(str::to_string),
            positive,
            name: None,
            spliced: false,
        }
    }

    fn riven(plus: &[&str], minus: &[&str]) -> Vec<RivenStatRead> {
        plus.iter()
            .map(|s| stat(Some(s), true))
            .chain(minus.iter().map(|s| stat(Some(s), false)))
            .collect()
    }

    fn read(weapon: &str, stats: &[RivenStatRead]) -> RollRead {
        match check(&parse(CASES).unwrap(), weapon, stats) {
            RollCheck::Read(r) => *r,
            other => panic!("expected a read, got {other:?}"),
        }
    }

    #[test]
    fn wanted_stats_with_a_harmless_negative_are_sellable() {
        let r = read(
            "gun",
            &riven(
                &["critical_damage", "multishot", "base_damage_/_melee_damage"],
                &["zoom"],
            ),
        );
        assert_eq!(r.verdict, RollVerdict::Sellable);
        assert_eq!(r.harmless.as_deref(), Some("zoom"));
        assert_eq!(r.hurts, None);
        assert_eq!(
            r.note.as_deref(),
            Some("IMP only harmless for status builds")
        );
        assert_eq!(
            (r.source.as_str(), r.checked.as_str()),
            ("44bananas", "44.1.2")
        );
    }

    #[test]
    fn two_wanted_positives_and_no_negative_are_enough() {
        let r = read("gun", &riven(&["critical_damage", "multishot"], &[]));
        assert_eq!(r.verdict, RollVerdict::Sellable);
        assert_eq!((r.hurts, r.harmless), (None, None));
    }

    #[test]
    fn one_flaw_is_almost_and_names_it() {
        let dead_third = read(
            "gun",
            &riven(&["critical_damage", "multishot", "range"], &["zoom"]),
        );
        assert_eq!(dead_third.verdict, RollVerdict::Almost);
        assert_eq!(dead_third.dead, vec!["range"]);

        let bad_negative = read(
            "gun",
            &riven(&["critical_damage", "multishot"], &["recoil"]),
        );
        assert_eq!(bad_negative.verdict, RollVerdict::Almost);
        assert_eq!(bad_negative.hurts.as_deref(), Some("recoil"));
    }

    #[test]
    fn a_missing_required_stat_is_named() {
        let r = read(
            "blade",
            &riven(
                &["critical_damage", "fire_rate_/_attack_speed"],
                &["impact_damage"],
            ),
        );
        assert_eq!(r.verdict, RollVerdict::Almost);
        assert_eq!(r.missing, vec!["range"]);
        assert_eq!(
            r.need,
            vec!["critical_damage", "fire_rate_/_attack_speed", "range"]
        );
    }

    #[test]
    fn two_flaws_or_one_wanted_stat_is_weak() {
        let r = read(
            "gun",
            &riven(&["critical_damage", "range", "heat_damage"], &["recoil"]),
        );
        assert_eq!(r.verdict, RollVerdict::Weak);
        let lone = read("gun", &riven(&["critical_damage"], &["zoom"]));
        assert_eq!(lone.verdict, RollVerdict::Weak);
    }

    #[test]
    fn the_closest_alternative_build_is_reported() {
        let r = read("blade", &riven(&["heat_damage", "range"], &[]));
        assert_eq!(r.verdict, RollVerdict::Sellable);
        assert_eq!(r.need, vec!["heat_damage"]);
        assert_eq!(
            r.pick,
            vec!["critical_damage", "range", "fire_rate_/_attack_speed"]
        );
    }

    #[test]
    fn fire_rate_and_attack_speed_are_one_stat() {
        let guide = parse(CASES).unwrap();
        let gun = guide.weapons.get("gun").unwrap();
        assert_eq!(
            gun.builds
                .first()
                .unwrap()
                .pick
                .iter()
                .filter(|s| *s == "fire_rate_/_attack_speed")
                .count(),
            1
        );
        let blade = guide.weapons.get("blade").unwrap();
        assert_eq!(blade.builds.first().unwrap().need.len(), 3);
    }

    #[test]
    fn unknown_weapons_and_unrecognised_stats_are_not_judged() {
        let guide = parse(CASES).unwrap();
        assert_eq!(
            check(&guide, "no_such_weapon", &riven(&["critical_damage"], &[])),
            RollCheck::NoGuide
        );
        let stats = vec![stat(Some("critical_damage"), true), stat(None, true)];
        assert_eq!(check(&guide, "gun", &stats), RollCheck::UnknownStat);
        assert_eq!(roll_check(None, false, &stats), None);
        assert_eq!(roll_check(Some("acceltra"), true, &stats), None);
    }

    #[test]
    fn an_undefined_abbreviation_refuses_the_guide() {
        let broken = CASES.replace(
            r#""pick": ["CD", "MS", "DMG""#,
            r#""pick": ["CD", "MSS", "DMG""#,
        );
        assert_eq!(parse(&broken).unwrap_err(), "gun: unknown stat MSS");
        assert!(parse("{").is_err());
    }

    #[test]
    fn the_verdict_serialises_for_the_view() {
        let r = check(
            &parse(CASES).unwrap(),
            "gun",
            &riven(&["critical_damage", "multishot"], &[]),
        );
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["kind"], "read");
        assert_eq!(v["verdict"], "sellable");
        assert_eq!(
            serde_json::to_value(RollCheck::NoGuide).unwrap(),
            serde_json::json!({"kind": "no_guide"})
        );
    }

    // ---- The shipped guide, checked against the game's stat rules ----

    fn market_rivens() -> Value {
        let market: Value =
            serde_json::from_str(include_str!("../../../../frontend/public/market.json")).unwrap();
        market["rivens"].clone()
    }

    #[test]
    fn the_shipped_guide_parses() {
        let parsed = parse(GUIDE_JSON).unwrap();
        assert!(
            parsed.weapons.len() > 400,
            "{} weapons",
            parsed.weapons.len()
        );
        assert!(guide().is_some());
    }

    #[test]
    fn every_guide_stat_is_a_wfm_riven_stat() {
        let rivens = market_rivens();
        let known: BTreeSet<&str> = rivens["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|a| a["slug"].as_str())
            .collect();
        let file: GuideFile = serde_json::from_str(GUIDE_JSON).unwrap();
        let unknown: Vec<_> = file
            .stats
            .iter()
            .filter(|(_, slug)| !known.contains(slug.as_str()))
            .collect();
        assert!(
            unknown.is_empty(),
            "abbreviations naming no WFM stat: {unknown:?}"
        );
    }

    #[test]
    fn every_guide_weapon_is_a_wfm_riven_weapon() {
        let rivens = market_rivens();
        let file: GuideFile = serde_json::from_str(GUIDE_JSON).unwrap();
        let unknown: Vec<_> = file
            .weapons
            .keys()
            .filter(|w| rivens["weapons"].get(w.as_str()).is_none())
            .collect();
        assert!(unknown.is_empty(), "weapons WFM does not list: {unknown:?}");
    }

    /// Each entry is checked against what can actually roll: a stat the
    /// weapon's riven type cannot have, a wanted stat that only rolls negative
    /// or a harmless negative that only rolls positive is a mistake in the
    /// entry, not a judgement call.
    #[test]
    fn every_entry_is_a_roll_the_game_can_make() {
        let rivens = market_rivens();
        let limits: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/rivens/stat-limits.json"
        ))
        .unwrap();
        let guide = parse(GUIDE_JSON).unwrap();
        let mut faults = Vec::new();
        for (weapon, entry) in &guide.weapons {
            let kind = rivens["weapons"][weapon.as_str()]["riven_type"]
                .as_str()
                .unwrap_or("?");
            let allowed = |slug: &str, positive: bool| {
                let limit = &limits["stats"][slug];
                if limit.is_null() {
                    return Err(format!("{slug} has no limits entry"));
                }
                if let Some(only) = limit["only"].as_array() {
                    if !only.iter().any(|t| t.as_str() == Some(kind)) {
                        return Err(format!("{slug} cannot roll on a {kind}"));
                    }
                }
                match (limit["sign"].as_str(), positive) {
                    (Some("negative"), true) => Err(format!("{slug} only rolls negative")),
                    (Some("positive"), false) => Err(format!("{slug} only rolls positive")),
                    _ => Ok(()),
                }
            };
            for build in &entry.builds {
                if build.need.len() + build.pick.len() < 2 {
                    faults.push(format!("{weapon}: a build needs at least two stats"));
                }
                if build.need.len() > 3 {
                    faults.push(format!("{weapon}: a riven has at most three positives"));
                }
                for slug in build.need.iter().chain(&build.pick) {
                    if let Err(e) = allowed(slug, true) {
                        faults.push(format!("{weapon}: {e}"));
                    }
                }
            }
            for slug in &entry.harmless {
                if let Err(e) = allowed(slug, false) {
                    faults.push(format!("{weapon}: harmless {e}"));
                }
            }
            if entry.builds.is_empty()
                || entry.source.trim().is_empty()
                || entry.checked.trim().is_empty()
            {
                faults.push(format!(
                    "{weapon}: needs builds, a source and the update it was checked against"
                ));
            }
        }
        assert!(faults.is_empty(), "{}", faults.join("\n"));
    }
}
