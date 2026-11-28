//! Rivens: reading them out of the DE inventory, joining them to WFM's
//! manifest, the Update 44.1 splice recipes, the comps filter and the
//! stat-set similarity the Rivens view shows. The view words and formats all of
//! it; nothing here estimates what a riven is worth (see [`appraise`]).
//!
//! Parity with the TypeScript rules this replaced: tests/fixtures/rivens/rivens.json.

pub mod appraise;
pub mod roll_guide;
pub mod sold;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use ts_rs::TS;

/// Rivens live in the inventory's `Upgrades[]` under this path, with the weapon
/// and stats inside the `UpgradeFingerprint` JSON string.
const RIVEN_PATH_PREFIX: &str = "/Lotus/Upgrades/Mods/Randomized/";

/// Fingerprint keys this module reads, or knows carry nothing to show.
const KNOWN_FINGERPRINT_KEYS: [&str; 10] = [
    "compat",
    "lim",
    "lvl",
    "lvlReq",
    "rerolls",
    "pol",
    "buffs",
    "curses",
    "challenge",
    "IsSentinel",
];

/// WFM's search form takes at most three positive stats and one negative;
/// the desktop's comps command refuses anything larger.
pub const MAX_FILTER_POSITIVE: usize = 3;

/// One stat line of a riven fingerprint. `value` is a Q30 roll fraction used as
/// one input to DE's final stat formula; it is not the displayed stat.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct RivenFingerprintStat {
    pub tag: String,
    pub value: f64,
}

/// A riven parsed from the inventory's `Upgrades[]`. `slug`/`weaponName` are
/// filled once the market's rivens surface is available.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OwnedRiven {
    /// The full /Lotus/Upgrades/Mods/Randomized/... path.
    pub path: String,
    /// Weapon /Lotus/... path from the fingerprint; null on veiled rivens.
    pub compat: Option<String>,
    /// WFM weapon slug, resolved via the weapons manifest's `game_ref`.
    pub slug: Option<String>,
    /// WFM weapon display name once resolved.
    pub weapon_name: Option<String>,
    pub rerolls: f64,
    pub lvl: f64,
    pub pol: Option<String>,
    pub buffs: Vec<RivenFingerprintStat>,
    pub curses: Vec<RivenFingerprintStat>,
    /// A challenge instead of stats - the riven is veiled.
    pub veiled: bool,
    /// DE's fingerprint string exactly as scanned. Kept because DE adds riven
    /// state (locks, splices) before anyone knows its keys, and the parsed
    /// fields above drop what they do not name. Absent on snapshots saved
    /// before it was kept.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub raw: Option<String>,
}

fn non_empty_str(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// A present, non-null field: how `a ?? b` picks.
fn present<'a>(o: &'a Value, key: &str) -> Option<&'a Value> {
    o.get(key).filter(|v| !v.is_null())
}

// DE's fingerprint stat objects use capitalized keys (`Tag` / `Value`); the
// lowercase shape is accepted too.
fn stat_of(s: &Value) -> Option<RivenFingerprintStat> {
    if !s.is_object() {
        return None;
    }
    let tag = present(s, "Tag").or_else(|| present(s, "tag"))?.as_str()?;
    let value = present(s, "Value")
        .or_else(|| present(s, "value"))?
        .as_f64()?;
    Some(RivenFingerprintStat {
        tag: tag.to_owned(),
        value,
    })
}

fn stats_of(v: Option<&Value>) -> Vec<RivenFingerprintStat> {
    v.and_then(Value::as_array)
        .map(|list| list.iter().filter_map(stat_of).collect())
        .unwrap_or_default()
}

/// Owned rivens from the raw inventory's `Upgrades`. Only entries under the
/// Randomized path count; everything else (railjack avionics, regular mods)
/// is not a riven. A malformed fingerprint reads as an empty riven (veiled,
/// unlisted) - better than failing the whole inventory view over one bad mod.
pub fn extract_rivens(upgrades: &Value) -> Vec<OwnedRiven> {
    let Some(entries) = upgrades.as_array() else {
        return Vec::new();
    };
    let empty = Value::Null;
    entries
        .iter()
        .filter_map(|entry| {
            let path = entry.get("ItemType").and_then(Value::as_str)?;
            if !path.starts_with(RIVEN_PATH_PREFIX) {
                return None;
            }
            let raw = entry.get("UpgradeFingerprint").and_then(Value::as_str);
            let parsed = raw
                .filter(|r| !r.is_empty())
                .and_then(|r| serde_json::from_str::<Value>(r).ok());
            let fp = parsed.as_ref().filter(|p| p.is_object()).unwrap_or(&empty);
            let buffs = stats_of(fp.get("buffs"));
            let compat = non_empty_str(fp.get("compat"));
            Some(OwnedRiven {
                path: path.to_owned(),
                veiled: compat.is_none() && buffs.is_empty(),
                compat,
                slug: None,
                weapon_name: None,
                rerolls: fp.get("rerolls").and_then(Value::as_f64).unwrap_or(0.0),
                lvl: fp.get("lvl").and_then(Value::as_f64).unwrap_or(0.0),
                pol: non_empty_str(fp.get("pol")),
                buffs,
                curses: stats_of(fp.get("curses")),
                raw: raw.map(str::to_owned),
            })
        })
        .collect()
}

/// A canonical array index, which a JavaScript object lists before its other
/// keys whatever order they were written in.
fn array_index(key: &str) -> Option<u32> {
    let n: u32 = key.parse().ok()?;
    (n != u32::MAX && n.to_string() == key).then_some(n)
}

/// Fingerprint keys the app does not read. A spliced trait may not sit in
/// `buffs`: a server reimplementation's reroll code carries a TODO for an
/// `advancedTrait` alongside them. Until a real spliced fingerprint has been
/// seen, an unknown key is reported rather than guessed at, so a riven never
/// silently looks like it has fewer traits than it does. Listed in the order
/// the page's own `Object.keys` gave.
pub fn unread_fingerprint_keys(raw: Option<&str>) -> Vec<String> {
    let Some(Value::Object(map)) = raw.and_then(|r| serde_json::from_str::<Value>(r).ok()) else {
        return Vec::new();
    };
    let mut indexed: Vec<(u32, &String)> = Vec::new();
    let mut named: Vec<&String> = Vec::new();
    for key in map.keys() {
        match array_index(key) {
            Some(n) => indexed.push((n, key)),
            None => named.push(key),
        }
    }
    indexed.sort_by_key(|(n, _)| *n);
    indexed
        .into_iter()
        .map(|(_, key)| key)
        .chain(named)
        .filter(|key| !KNOWN_FINGERPRINT_KEYS.contains(&key.as_str()))
        .cloned()
        .collect()
}

/// A riven as the weapon join reads it.
#[derive(Debug, Clone, Deserialize, TS)]
pub struct RivenCompat {
    #[serde(default)]
    pub compat: Option<String>,
    #[serde(default)]
    pub veiled: bool,
}

/// The manifest weapon each riven belongs to: an index into `game_refs` (the
/// weapons in manifest order), or None for a veiled riven and one whose weapon
/// WFM does not list yet (brand-new content), which the view shows without a
/// band or comps. Two weapons claiming one path resolve to the later one.
pub fn resolve_rivens(rivens: &[RivenCompat], game_refs: &[Option<String>]) -> Vec<Option<usize>> {
    let mut index: HashMap<&str, usize> = HashMap::new();
    for (i, game_ref) in game_refs.iter().enumerate() {
        if let Some(game_ref) = game_ref.as_deref().filter(|g| !g.is_empty()) {
            index.insert(game_ref, i);
        }
    }
    rivens
        .iter()
        .map(|r| {
            let compat = r.compat.as_deref().filter(|c| !c.is_empty() && !r.veiled)?;
            index.get(compat).copied()
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandTier {
    Unrolled,
    Rolled,
}

/// The DE weekly band for a riven: the rerolled tier when it has rerolls, else
/// unrolled; falls back to the other tier when DE only published one.
pub fn band_tier(rerolls: f64, has_unrolled: bool, has_rolled: bool) -> Option<BandTier> {
    let (preferred, other) = if rerolls > 0.0 {
        (
            (has_rolled, BandTier::Rolled),
            (has_unrolled, BandTier::Unrolled),
        )
    } else {
        (
            (has_unrolled, BandTier::Unrolled),
            (has_rolled, BandTier::Rolled),
        )
    };
    [preferred, other]
        .into_iter()
        .find(|(has, _)| *has)
        .map(|(_, tier)| tier)
}

/// One stat of WFM's `/riven/attributes` manifest, keyed by the fingerprint's DE
/// tag (`game_ref`).
#[derive(Debug, Clone, Default, Deserialize, TS)]
pub struct RivenAttributeRef {
    #[serde(default)]
    #[ts(optional)]
    pub game_ref: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub slug: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
}

fn attribute_for_tag<'a>(
    tag: &str,
    attrs: &'a [RivenAttributeRef],
) -> Option<&'a RivenAttributeRef> {
    attrs.iter().find(|a| a.game_ref.as_deref() == Some(tag))
}

/// One fingerprint stat joined to the manifest. `slug` and `name` are null when
/// WFM's manifest has no entry for the tag; the view then shows the raw tag,
/// marked, so a new DE trait is visible as unknown instead of passing for a
/// real stat name. Exact values cannot be recovered from the roll fraction
/// alone: DE's formula also needs the Riven class, weapon disposition, rank,
/// stat-count weights, and per-stat base values.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct RivenStatRead {
    pub tag: String,
    pub slug: Option<String>,
    pub positive: bool,
    pub name: Option<String>,
    pub spliced: bool,
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct RivenStatTag {
    pub tag: String,
}

fn resolve_stats(
    buffs: &[RivenStatTag],
    curses: &[RivenStatTag],
    attrs: &[RivenAttributeRef],
) -> Vec<RivenStatRead> {
    let one = |stat: &RivenStatTag, positive: bool| {
        let attr = attribute_for_tag(&stat.tag, attrs);
        let slug = attr.and_then(|a| a.slug.clone());
        RivenStatRead {
            tag: stat.tag.clone(),
            spliced: slug.as_deref().is_some_and(is_spliced_stat),
            slug,
            positive,
            name: attr.and_then(|a| a.name.clone()),
        }
    };
    buffs
        .iter()
        .map(|s| one(s, true))
        .chain(curses.iter().map(|s| one(s, false)))
        .collect()
}

// ---- Riven splicing (Update 44.1) ----
//
// A Riven Splicer consumes two specific traits and writes one spliced trait
// plus a random trait in their place. Recipes are DE's published table (PC
// patch notes 44.1.0), keyed here by WFM attribute slug. Spliced traits exist
// only through splicing, so a listing or fingerprint carrying one of these
// slugs is a spliced riven; WFM marks nothing else.

/// Which riven classes a recipe applies to. Archgun rivens report `rifle`,
/// companion weapons report the class of riven they take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum SpliceScope {
    Ranged,
    Melee,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
pub struct SpliceRecipe {
    pub result: &'static str,
    /// DE's name for the spliced trait, for when the snapshot's manifest
    /// predates it.
    pub name: &'static str,
    pub ingredients: [&'static str; 2],
    pub scope: SpliceScope,
}

const DAMAGE: &str = "base_damage_/_melee_damage";
const ATTACK_SPEED: &str = "fire_rate_/_attack_speed";

const fn recipe(
    result: &'static str,
    name: &'static str,
    ingredients: [&'static str; 2],
    scope: SpliceScope,
) -> SpliceRecipe {
    SpliceRecipe {
        result,
        name,
        ingredients,
        scope,
    }
}

pub const SPLICE_RECIPES: [SpliceRecipe; 22] = [
    recipe(
        "weak_point_damage",
        "Weak Point Damage",
        [DAMAGE, "zoom"],
        SpliceScope::Ranged,
    ),
    recipe(
        "weak_point_damage",
        "Weak Point Damage",
        [DAMAGE, "multishot"],
        SpliceScope::Ranged,
    ),
    recipe(
        "weak_point_critical_chance",
        "Weak Point Critical Chance",
        ["critical_chance", "zoom"],
        SpliceScope::Ranged,
    ),
    recipe(
        "weak_point_critical_chance",
        "Weak Point Critical Chance",
        ["critical_chance", "multishot"],
        SpliceScope::Ranged,
    ),
    recipe(
        "ammo_efficiency",
        "Ammo Efficiency",
        ["magazine_capacity", "reload_speed"],
        SpliceScope::Ranged,
    ),
    recipe(
        "ammo_efficiency",
        "Ammo Efficiency",
        ["recoil", "ammo_maximum"],
        SpliceScope::Ranged,
    ),
    recipe(
        "magazine_reloaded_s_when_holstered",
        "Magazine Reload While Holstered",
        ["ammo_maximum", "reload_speed"],
        SpliceScope::Ranged,
    ),
    recipe(
        "magazine_reloaded_s_when_holstered",
        "Magazine Reload While Holstered",
        ["ammo_maximum", "magazine_capacity"],
        SpliceScope::Ranged,
    ),
    // Ranged "Damage" and melee "Melee Damage" are the same WFM attribute.
    recipe(
        "status_damage",
        "Status Damage",
        [DAMAGE, "status_chance"],
        SpliceScope::All,
    ),
    recipe(
        "melee_damage_on_heavy_attack",
        "Heavy Attack Damage",
        ["channeling_efficiency", "chance_to_gain_extra_combo_count"],
        SpliceScope::Melee,
    ),
    recipe(
        "heavy_attack_wind_up_speed",
        "Heavy Attack Wind Up Speed",
        ["channeling_efficiency", "combo_duration"],
        SpliceScope::Melee,
    ),
    recipe(
        "parry_angle",
        "Parry Angle",
        [ATTACK_SPEED, "range"],
        SpliceScope::Melee,
    ),
    recipe(
        "slam_attack_damage",
        "Slam Damage",
        [DAMAGE, ATTACK_SPEED],
        SpliceScope::Melee,
    ),
    recipe(
        "gas",
        "Gas",
        ["toxin_damage", "heat_damage"],
        SpliceScope::All,
    ),
    recipe(
        "corrosive",
        "Corrosive",
        ["toxin_damage", "electric_damage"],
        SpliceScope::All,
    ),
    recipe(
        "viral",
        "Viral",
        ["toxin_damage", "cold_damage"],
        SpliceScope::All,
    ),
    recipe(
        "radiation",
        "Radiation",
        ["heat_damage", "electric_damage"],
        SpliceScope::All,
    ),
    recipe(
        "blast",
        "Blast",
        ["heat_damage", "cold_damage"],
        SpliceScope::All,
    ),
    recipe(
        "magnetic",
        "Magnetic",
        ["electric_damage", "cold_damage"],
        SpliceScope::All,
    ),
    recipe(
        "damage_to_orokin",
        "Damage to Orokin",
        ["damage_vs_corpus", "damage_vs_grineer"],
        SpliceScope::All,
    ),
    recipe(
        "damage_to_techrot",
        "Damage to Techrot",
        ["damage_vs_corpus", "damage_vs_infested"],
        SpliceScope::All,
    ),
    recipe(
        "damage_to_scaldra",
        "Damage to Scaldra",
        ["damage_vs_infested", "damage_vs_grineer"],
        SpliceScope::All,
    ),
];

pub fn is_spliced_stat(slug: &str) -> bool {
    SPLICE_RECIPES.iter().any(|r| r.result == slug)
}

/// The spliced traits, once each, in recipe order.
pub fn spliced_traits() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for r in &SPLICE_RECIPES {
        if !out.contains(&r.result) {
            out.push(r.result);
        }
    }
    out
}

/// The recipe class for a weapon's `riven_type`; None when WFM gave none or one
/// we do not recognise, which limits a riven to the all-class recipes.
fn splice_scope_of(riven_type: Option<&str>) -> Option<SpliceScope> {
    match riven_type? {
        "rifle" | "shotgun" | "pistol" | "kitgun" => Some(SpliceScope::Ranged),
        "melee" | "zaw" => Some(SpliceScope::Melee),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SpliceChoice {
    pub recipe: SpliceRecipe,
    /// The ingredients as they sit on the riven, in recipe order: indexes into
    /// the row's stats.
    pub consumed: [usize; 2],
    /// DE: "if one of the consumed traits was a negative bonus, the random
    /// trait will also be a negative".
    pub random_is_negative: bool,
}

/// The splices this riven could take right now.
///
/// One spliced trait per riven, so a riven that already carries one has none.
/// Each recipe needs both ingredients present. DE documents a negative
/// ingredient beside a positive one (the spliced trait takes the positive
/// one's grade); two negative ingredients are undocumented, so they are not
/// offered.
fn splice_options(stats: &[RivenStatRead], riven_type: Option<&str>) -> Vec<SpliceChoice> {
    if stats.iter().any(|s| s.spliced) {
        return Vec::new();
    }
    let scope = splice_scope_of(riven_type);
    let find = |slug: &str| stats.iter().position(|s| s.slug.as_deref() == Some(slug));
    SPLICE_RECIPES
        .iter()
        .filter(|r| r.scope == SpliceScope::All || Some(r.scope) == scope)
        .filter_map(|recipe| {
            let [first, second] = recipe.ingredients;
            let (a, b) = (find(first)?, find(second)?);
            let positive = |i: usize| stats.get(i).is_some_and(|s| s.positive);
            if !positive(a) && !positive(b) {
                return None;
            }
            Some(SpliceChoice {
                recipe: *recipe,
                consumed: [a, b],
                random_is_negative: !positive(a) || !positive(b),
            })
        })
        .collect()
}

/// Narrows a comps search to auctions carrying every listed stat with that sign.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct RivenStatFilter {
    pub positive: Vec<String>,
    pub negative: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct RivenStatFilterPlan {
    pub filter: RivenStatFilter,
    /// False when a positive stat could not be resolved to a WFM slug, so the
    /// filter is broader than the riven.
    pub complete: bool,
}

/// A comps filter on the riven's positive stats, or None when none resolve.
/// Negatives are left out: matching them too leaves most weapons with no
/// listings, and the stat match on each row already shows them.
fn comps_filter_for(stats: &[RivenStatRead]) -> Option<RivenStatFilterPlan> {
    let positives = stats.iter().filter(|s| s.positive).count();
    let slugs: Vec<String> = stats
        .iter()
        .filter(|s| s.positive)
        .filter_map(|s| s.slug.clone().filter(|slug| !slug.is_empty()))
        .collect();
    if slugs.is_empty() {
        return None;
    }
    let complete = slugs.len() == positives && slugs.len() <= MAX_FILTER_POSITIVE;
    Some(RivenStatFilterPlan {
        filter: RivenStatFilter {
            positive: slugs.into_iter().take(MAX_FILTER_POSITIVE).collect(),
            negative: Vec::new(),
        },
        complete,
    })
}

/// One owned riven as the Rivens view's row reads it.
#[derive(Debug, Clone, Deserialize, TS)]
pub struct RivenRowInput {
    pub buffs: Vec<RivenStatTag>,
    pub curses: Vec<RivenStatTag>,
    #[serde(default)]
    #[ts(optional)]
    pub raw: Option<String>,
    #[serde(default)]
    pub veiled: bool,
    /// The weapon's WFM `riven_type`, which picks the splice recipes.
    #[serde(default)]
    #[ts(optional)]
    pub riven_type: Option<String>,
    /// The weapon's WFM slug, which picks the roll guide entry.
    #[serde(default)]
    #[ts(optional)]
    pub weapon: Option<String>,
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct RivenRowsRequest {
    pub attributes: Vec<RivenAttributeRef>,
    pub rivens: Vec<RivenRowInput>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct RivenRow {
    pub stats: Vec<RivenStatRead>,
    pub splices: Vec<SpliceChoice>,
    pub filter: Option<RivenStatFilterPlan>,
    pub unread: Vec<String>,
    /// The roll guide's verdict; None for a veiled riven or an unresolved weapon.
    pub roll: Option<roll_guide::RollCheck>,
}

pub fn riven_rows(request: &RivenRowsRequest) -> Vec<RivenRow> {
    request
        .rivens
        .iter()
        .map(|riven| {
            let stats = resolve_stats(&riven.buffs, &riven.curses, &request.attributes);
            RivenRow {
                splices: if riven.veiled {
                    Vec::new()
                } else {
                    splice_options(&stats, riven.riven_type.as_deref())
                },
                filter: comps_filter_for(&stats),
                unread: unread_fingerprint_keys(riven.raw.as_deref()),
                roll: roll_guide::roll_check(riven.weapon.as_deref(), riven.veiled, &stats),
                stats,
            }
        })
        .collect()
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct AuctionStatSign {
    pub url_name: String,
    pub positive: bool,
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct RivenSimilarityRequest {
    pub attributes: Vec<RivenAttributeRef>,
    pub buffs: Vec<RivenStatTag>,
    pub curses: Vec<RivenStatTag>,
    /// Each auction's stats.
    pub auctions: Vec<Vec<AuctionStatSign>>,
}

fn signed(positive: bool, name: &str) -> String {
    format!("{}:{name}", if positive { '+' } else { '-' })
}

/// Signed-stat Jaccard similarity, from 0-100. This compares which effects the
/// two Rivens have, not roll strength - the inventory fingerprint alone cannot
/// supply a final display value honestly. A stat the manifest cannot name still
/// counts as one of the riven's effects, matching nothing: dropping it would
/// shrink the riven and overstate the match.
pub fn riven_similarity(
    buffs: &[RivenStatTag],
    curses: &[RivenStatTag],
    auction: &[AuctionStatSign],
    attrs: &[RivenAttributeRef],
) -> Option<f64> {
    let owned_key = |stat: &RivenStatTag, positive: bool| match attribute_for_tag(&stat.tag, attrs)
    {
        // A manifest entry without a slug compared as the page's template string did.
        Some(attr) => signed(positive, attr.slug.as_deref().unwrap_or("undefined")),
        None => signed(positive, &format!("?{}", stat.tag)),
    };
    let owned: BTreeSet<String> = buffs
        .iter()
        .map(|s| owned_key(s, true))
        .chain(curses.iter().map(|s| owned_key(s, false)))
        .collect();
    let comparable: BTreeSet<String> = auction
        .iter()
        .map(|s| signed(s.positive, &s.url_name))
        .collect();
    let keys = owned.union(&comparable).count();
    if keys == 0 || owned.is_empty() || comparable.is_empty() {
        return None;
    }
    let matches = owned.intersection(&comparable).count();
    Some((matches as f64 / keys as f64 * 100.0).round())
}

pub fn riven_similarities(request: &RivenSimilarityRequest) -> Vec<Option<f64>> {
    request
        .auctions
        .iter()
        .map(|auction| {
            riven_similarity(
                &request.buffs,
                &request.curses,
                auction,
                &request.attributes,
            )
        })
        .collect()
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct ExtractRivensRequest {
    /// The inventory's `Upgrades`, as DE wrote it.
    #[serde(default)]
    #[ts(type = "unknown")]
    pub upgrades: Value,
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct ResolveRivensRequest {
    pub rivens: Vec<RivenCompat>,
    /// Each manifest weapon's `game_ref`, in manifest order.
    pub game_refs: Vec<Option<String>>,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    /// Equal as the page compares JSON: numbers by value, so 10 and 10.0 match.
    pub(crate) fn same(got: &Value, want: &Value, case: &str) {
        match (got, want) {
            (Value::Number(a), Value::Number(b)) => {
                assert_eq!(a.as_f64(), b.as_f64(), "{case}: {got} != {want}");
            }
            (Value::Array(a), Value::Array(b)) => {
                assert_eq!(a.len(), b.len(), "{case}: {got} != {want}");
                for (x, y) in a.iter().zip(b) {
                    same(x, y, case);
                }
            }
            (Value::Object(a), Value::Object(b)) => {
                assert_eq!(
                    a.keys().collect::<BTreeSet<_>>(),
                    b.keys().collect::<BTreeSet<_>>(),
                    "{case}: {got} != {want}"
                );
                for (k, v) in a {
                    same(v, &b[k], case);
                }
            }
            _ => assert_eq!(got, want, "{case}"),
        }
    }

    fn fixture() -> Value {
        serde_json::from_str(include_str!(
            "../../../../tests/fixtures/rivens/rivens.json"
        ))
        .unwrap()
    }

    fn attrs(f: &Value, key: &Value) -> Vec<RivenAttributeRef> {
        serde_json::from_value(f["attribute_sets"][key.as_str().unwrap()].clone())
            .unwrap_or_default()
    }

    fn tags(v: &Value) -> Vec<RivenStatTag> {
        serde_json::from_value(v.clone()).unwrap()
    }

    #[test]
    fn extraction_matches_the_shared_fixture() {
        for c in fixture()["extract"].as_array().unwrap() {
            let upgrades = c
                .get("inventory")
                .and_then(|i| i.get("Upgrades"))
                .cloned()
                .unwrap_or(Value::Null);
            let got = serde_json::to_value(extract_rivens(&upgrades)).unwrap();
            same(&got, &c["expected"], c["name"].as_str().unwrap());
        }
    }

    #[test]
    fn the_weapon_join_matches_the_shared_fixture() {
        for c in fixture()["resolve"].as_array().unwrap() {
            let weapons = c
                .get("weapons")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let game_refs: Vec<Option<String>> = weapons
                .values()
                .map(|w| w.get("game_ref").and_then(Value::as_str).map(str::to_owned))
                .collect();
            let rivens: Vec<RivenCompat> = serde_json::from_value(c["rivens"].clone()).unwrap();
            let got: Vec<Value> = resolve_rivens(&rivens, &game_refs)
                .into_iter()
                .map(|i| match i {
                    None => Value::Null,
                    Some(i) => {
                        let (slug, w) = weapons.iter().nth(i).unwrap();
                        json!({ "slug": slug, "weaponName": w["name"] })
                    }
                })
                .collect();
            same(
                &Value::Array(got),
                &c["expected"],
                c["name"].as_str().unwrap(),
            );
        }
    }

    #[test]
    fn band_choice_matches_the_shared_fixture() {
        let f = fixture();
        for c in f["band"].as_array().unwrap() {
            let entry = c["riven"]["slug"]
                .as_str()
                .and_then(|slug| f["band_stats"][c["stats"].as_str().unwrap()].get(slug));
            let tier = |k: &str| entry.and_then(|e| e.get(k)).filter(|t| !t.is_null());
            let got = entry.and_then(|_| {
                band_tier(
                    c["riven"]["rerolls"].as_f64().unwrap(),
                    tier("unrolled").is_some(),
                    tier("rolled").is_some(),
                )
            });
            let picked = got.and_then(|t| {
                tier(if t == BandTier::Rolled {
                    "rolled"
                } else {
                    "unrolled"
                })
            });
            assert_eq!(
                picked.unwrap_or(&Value::Null),
                &c["expected"],
                "{}",
                c["name"]
            );
        }
    }

    #[test]
    fn rows_match_the_shared_fixture() {
        let f = fixture();
        let recipes = serde_json::to_value(SPLICE_RECIPES).unwrap();
        assert_eq!(recipes, f["recipes"]);
        for c in f["rows"].as_array().unwrap() {
            let name = c["name"].as_str().unwrap();
            let mut input = c["riven"].clone();
            input["riven_type"] = c.get("riven_type").cloned().unwrap_or(Value::Null);
            let request = RivenRowsRequest {
                attributes: attrs(&f, &c["attributes"]),
                rivens: vec![serde_json::from_value(input).unwrap()],
            };
            let row = riven_rows(&request).remove(0);
            let want = &c["expected"];
            let stats: Vec<Value> = row
                .stats
                .iter()
                .map(|s| {
                    let sign = if s.positive { "+" } else { "-" };
                    let label = match &s.name {
                        Some(n) => format!("{sign}{n}"),
                        None => format!("{sign}{} (unrecognised)", s.tag),
                    };
                    json!({ "tag": s.tag, "slug": s.slug, "positive": s.positive, "label": label, "spliced": s.spliced })
                })
                .collect();
            same(&Value::Array(stats), &want["stats"], name);
            let splices: Vec<Value> = row
                .splices
                .iter()
                .map(|o| {
                    let index = SPLICE_RECIPES.iter().position(|r| *r == o.recipe).unwrap();
                    json!({ "recipe": index, "consumed": o.consumed, "randomIsNegative": o.random_is_negative })
                })
                .collect();
            same(&Value::Array(splices), &want["splices"], name);
            same(
                &serde_json::to_value(&row.filter).unwrap(),
                &want["filter"],
                name,
            );
            same(
                &serde_json::to_value(&row.unread).unwrap(),
                &want["unread"],
                name,
            );
        }
    }

    #[test]
    fn similarity_matches_the_shared_fixture() {
        let f = fixture();
        for c in f["similarity"].as_array().unwrap() {
            let auction: Vec<AuctionStatSign> =
                serde_json::from_value(c["auction"].clone()).unwrap();
            let got = riven_similarity(
                &tags(&c["riven"]["buffs"]),
                &tags(&c["riven"]["curses"]),
                &auction,
                &attrs(&f, &c["attributes"]),
            );
            assert_eq!(got, c["expected"].as_f64(), "{}", c["name"]);
        }
    }

    #[test]
    fn spliced_traits_match_the_shared_fixture() {
        for c in fixture()["spliced"].as_array().unwrap() {
            let slug = c["slug"].as_str().unwrap();
            assert_eq!(
                is_spliced_stat(slug),
                c["expected"].as_bool().unwrap(),
                "{slug}"
            );
            assert_eq!(spliced_traits().contains(&slug), is_spliced_stat(slug));
        }
    }

    // WFM /v2/riven/attributes slugs as served on 2026-10-08, after the Update
    // 44.1 traits were added. Guards the recipe table against a typo'd slug,
    // which would otherwise silently never match.
    const WFM_ATTRIBUTE_SLUGS: [&str; 50] = [
        "ammo_efficiency",
        "ammo_maximum",
        "base_damage_/_melee_damage",
        "blast",
        "chance_to_gain_combo_count",
        "chance_to_gain_extra_combo_count",
        "channeling_damage",
        "channeling_efficiency",
        "cold_damage",
        "combo_duration",
        "corrosive",
        "critical_chance",
        "critical_chance_on_slide_attack",
        "critical_damage",
        "damage_to_orokin",
        "damage_to_scaldra",
        "damage_to_techrot",
        "damage_vs_corpus",
        "damage_vs_grineer",
        "damage_vs_infested",
        "electric_damage",
        "finisher_damage",
        "fire_rate_/_attack_speed",
        "gas",
        "heat_damage",
        "heavy_attack_wind_up_speed",
        "impact_damage",
        "magazine_capacity",
        "magazine_reloaded_s_when_holstered",
        "magnetic",
        "melee_damage_on_heavy_attack",
        "multishot",
        "parry_angle",
        "projectile_speed",
        "punch_through",
        "puncture_damage",
        "radiation",
        "range",
        "recoil",
        "reload_speed",
        "slam_attack_damage",
        "slash_damage",
        "status_chance",
        "status_damage",
        "status_duration",
        "toxin_damage",
        "viral",
        "weak_point_critical_chance",
        "weak_point_damage",
        "zoom",
    ];

    #[test]
    fn recipes_name_only_attributes_wfm_lists() {
        for r in &SPLICE_RECIPES {
            assert!(WFM_ATTRIBUTE_SLUGS.contains(&r.result), "{}", r.result);
            for slug in r.ingredients {
                assert!(WFM_ATTRIBUTE_SLUGS.contains(&slug), "{slug}");
            }
        }
    }

    #[test]
    fn unread_keys_follow_javascript_key_order() {
        let raw = r#"{"b":1,"10":2,"a":3,"2":4,"01":5,"compat":"x","b":9}"#;
        assert_eq!(
            unread_fingerprint_keys(Some(raw)),
            ["2", "10", "b", "a", "01"]
        );
        assert!(unread_fingerprint_keys(Some("{broken")).is_empty());
        assert!(unread_fingerprint_keys(None).is_empty());
    }

    #[test]
    fn a_riven_of_unknown_class_takes_only_the_all_class_recipes() {
        let attrs: Vec<RivenAttributeRef> = serde_json::from_value(json!([
            { "game_ref": "Heat", "slug": "heat_damage", "name": "Heat" },
            { "game_ref": "Cold", "slug": "cold_damage", "name": "Cold" },
            { "game_ref": "Dmg", "slug": DAMAGE, "name": "Damage" },
            { "game_ref": "Zoom", "slug": "zoom", "name": "Zoom" },
        ]))
        .unwrap();
        let tag = |t: &str| RivenStatTag { tag: t.to_owned() };
        let row = |riven_type: Option<&str>| {
            let stats = resolve_stats(
                &[tag("Heat"), tag("Cold"), tag("Dmg")],
                &[tag("Zoom")],
                &attrs,
            );
            splice_options(&stats, riven_type)
                .into_iter()
                .map(|o| (o.recipe.result, o.random_is_negative))
                .collect::<Vec<_>>()
        };
        assert_eq!(row(None), [("blast", false)]);
        assert_eq!(
            row(Some("rifle")),
            [("weak_point_damage", true), ("blast", false)]
        );
    }
}
