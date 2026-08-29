//! Turning DE's manifests and worldState into market.json surfaces.
//!
//! [`crate::de`] is transport and parsing; this module is the join. It exists
//! separately because the join is where the risk lives: DE speaks
//! `/Lotus/...` uniqueNames and warframe.market speaks slugs, and a silent
//! mismatch shows a user the wrong price - worse than showing none. Every
//! resolver here therefore returns `Option` and the callers count the misses.

use std::collections::HashMap;

use serde_json::Value;

use crate::de::{de_millis, manifest_rows_for, relay_name};

// ---------------------------------------------------------------------------
// Path resolution
// ---------------------------------------------------------------------------

/// Blueprint uniqueName → the component it builds.
///
/// This is the alias that closes most of the join. Relic tables reference
/// `.../OberonPrimeSystemsBlueprint`; the item catalogue knows
/// `.../OberonPrimeSystemsComponent`. `ExportRecipes.resultType` is the link,
/// and it lifts reward-path resolution from 69% to 87% (the rest - Forma,
/// Kuva, Exilus adapters, Kubrow collars - are genuinely untradeable and
/// *should* stay unresolved).
pub fn recipe_alias(recipes: &Value) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for row in manifest_rows_for(recipes, "ExportRecipes_en.json") {
        let (Some(u), Some(r)) = (
            row.get("uniqueName").and_then(|v| v.as_str()),
            row.get("resultType").and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        out.insert(u.to_string(), r.to_string());
    }
    out
}

/// Resolve a DE path to the pipeline's existing `path_to_info` entry.
///
/// Tries the path as given, then with the store alias stripped, then through
/// the recipe alias. Returns `None` rather than guessing.
pub fn resolve_path<'a>(
    unique: &str,
    path_to_info: &'a HashMap<String, Value>,
    alias: &HashMap<String, String>,
) -> Option<&'a Value> {
    let direct = unique.to_string();
    let stripped = unique
        .strip_prefix("/Lotus/StoreItems")
        .map(|rest| format!("/Lotus{rest}"))
        .unwrap_or_else(|| direct.clone());

    for key in [&direct, &stripped] {
        if let Some(v) = path_to_info.get(key) {
            return Some(v);
        }
    }
    for key in [&direct, &stripped] {
        if let Some(target) = alias.get(key) {
            if let Some(v) = path_to_info.get(target) {
                return Some(v);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Ducats
// ---------------------------------------------------------------------------

/// The five ducat tiers DE actually uses. A value outside this set means the
/// parse is wrong, not that a sixth tier appeared.
pub const DUCAT_TIERS: &[i64] = &[15, 25, 45, 65, 100];

/// `uniqueName → ducat value`, first-party.
///
/// `primeSellingPrice` sits on the **recipe** (the blueprint you trade), not on
/// its `resultType` - Nova Prime Blueprint is 45 while the frame it builds has
/// no ducat value at all. Keying on the recipe is therefore correct, and it is
/// the difference between a right and a wrong number.
///
/// Values off the known tiers are dropped with a warning rather than trusted.
pub fn ducats_from_recipes(recipes: &Value) -> HashMap<String, i64> {
    let mut out = HashMap::new();
    let mut odd = 0usize;
    for row in manifest_rows_for(recipes, "ExportRecipes_en.json") {
        let Some(unique) = row.get("uniqueName").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(price) = row.get("primeSellingPrice").and_then(|v| v.as_i64()) else {
            continue;
        };
        if !DUCAT_TIERS.contains(&price) {
            odd += 1;
            continue;
        }
        out.insert(unique.to_string(), price);
    }
    if odd > 0 {
        eprintln!("  warning: {odd} recipes carried a ducat value off the known tiers - dropped");
    }
    out
}

// ---------------------------------------------------------------------------
// Relic rewards
// ---------------------------------------------------------------------------

/// Per-slot drop chance by rarity and refinement.
///
/// **These are ours, not DE's.** The export ships four uniqueName variants per
/// relic (Bronze / Silver / Gold / Platinum = intact / exceptional / flawless /
/// radiant), but their reward lists are byte-identical - refinement changes the
/// odds, not the contents, and DE does not publish the odds. So the four tiers
/// are derived here from the long-standing published table. If DE ever changes
/// it, this constant goes silently wrong; it is the one number in the relic
/// path worth re-checking on a major update.
///
/// Order is [intact, exceptional, flawless, radiant].
pub const REFINEMENT_CHANCE: &[(&str, [f64; 4])] = &[
    ("COMMON", [25.33, 23.33, 20.00, 16.67]),
    ("UNCOMMON", [11.00, 13.00, 17.00, 20.00]),
    ("RARE", [2.00, 4.00, 6.00, 10.00]),
];

pub const REFINEMENTS: [&str; 4] = ["intact", "exceptional", "flawless", "radiant"];

fn chances_for(rarity: &str) -> Option<[f64; 4]> {
    REFINEMENT_CHANCE
        .iter()
        .find(|(r, _)| r.eq_ignore_ascii_case(rarity))
        .map(|(_, c)| *c)
}

/// "Lith H2 Relic" → `lith_h2_relic`, the slug the rest of the app keys on.
pub fn relic_slug(name: &str) -> Option<String> {
    let lower = name.trim().to_lowercase();
    let stem = lower.strip_suffix(" relic")?;
    if stem.is_empty() {
        return None;
    }
    Some(format!("{}_relic", stem.replace(' ', "_")))
}

/// Title-case DE's SCREAMING rarity so the surface keeps the casing the SPA
/// already renders.
fn pretty_rarity(r: &str) -> String {
    let lower = r.to_lowercase();
    let mut c = lower.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => lower,
    }
}

/// Result of building the relic surface, so the caller can report the misses
/// instead of hiding them.
#[derive(Debug, Default)]
pub struct RelicBuild {
    pub rewards: HashMap<String, Value>,
    pub resolved: usize,
    pub unresolved: usize,
    /// A few unresolved paths, for the run report.
    pub samples: Vec<String>,
}

/// Build `relic_rewards` from `ExportRelicArcane`, with all four refinements.
///
/// Two things this fixes versus the drop-table source it replaces: coverage of
/// every refinement rather than intact only, and **correct rarity labels** -
/// the old source labels 25.33% drops "Uncommon", which is the common tier.
pub fn relic_rewards_from_de(
    relics: &Value,
    recipes: &Value,
    path_to_info: &HashMap<String, Value>,
) -> RelicBuild {
    let alias = recipe_alias(recipes);
    let mut build = RelicBuild::default();

    for row in manifest_rows_for(relics, "ExportRelicArcane_en.json") {
        let Some(name) = row.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(slug) = relic_slug(name) else { continue };
        // The four refinement variants carry identical reward lists, so the
        // first one wins and the rest are skipped rather than merged.
        if build.rewards.contains_key(&slug) {
            continue;
        }
        let Some(rewards) = row.get("relicRewards").and_then(|v| v.as_array()) else {
            continue;
        };

        let mut out = Vec::new();
        for rw in rewards {
            let Some(path) = rw.get("rewardName").and_then(|v| v.as_str()) else {
                continue;
            };
            let rarity = rw.get("rarity").and_then(|v| v.as_str()).unwrap_or("");
            let Some(chances) = chances_for(rarity) else { continue };

            let Some(info) = resolve_path(path, path_to_info, &alias) else {
                build.unresolved += 1;
                if build.samples.len() < 8 {
                    build.samples.push(path.to_string());
                }
                continue;
            };
            let (Some(reward_slug), Some(reward_name)) = (
                info.get("slug").and_then(|v| v.as_str()),
                info.get("name").and_then(|v| v.as_str()),
            ) else {
                build.unresolved += 1;
                continue;
            };
            build.resolved += 1;

            let mut by_refinement = serde_json::Map::new();
            for (i, key) in REFINEMENTS.iter().enumerate() {
                by_refinement.insert((*key).to_string(), Value::from(chances[i]));
            }
            out.push(serde_json::json!({
                "reward_slug": reward_slug,
                "reward_name": reward_name,
                "rarity": pretty_rarity(rarity),
                // Intact stays the bare `chance` so consumers written against
                // the old single-tier surface keep working unchanged.
                "chance": chances[0],
                "chances": Value::Object(by_refinement),
                "item_count": rw.get("itemCount").and_then(|v| v.as_i64()).unwrap_or(1),
            }));
        }
        if !out.is_empty() {
            build.rewards.insert(slug, Value::Array(out));
        }
    }
    build
}

// ---------------------------------------------------------------------------
// Recipes - what building a thing actually costs
// ---------------------------------------------------------------------------

/// Build costs, keyed by the slug of the TRADEABLE item each recipe belongs to.
///
/// Two shapes, and missing the second one silently undercounts every build:
///
/// - A component recipe produces the component the user holds, so its
///   `resultType` resolves and that is the key.
/// - A **final assembly** recipe (`NovaPrimeBlueprint`) produces the finished
///   Warframe - `/Lotus/Powersuits/AntiMatter/NovaPrime` - which the item
///   catalogue does not carry at all, because you cannot trade a built frame.
///   Its tradeable identity is the blueprint you feed it, which is the
///   recipe's own `uniqueName`. Without that fallback the last and most
///   expensive step of a build (25k credits, 3 days, 50p to rush on Nova)
///   vanishes and the plan reads cheaper than it is. The fallback also lifts
///   coverage from 157 recipes to 312.
///
/// `ingredients` keep their display name even when they do not resolve to a
/// market slug - Orokin Cells and Argon Crystals are the majority of a build
/// and are not tradeable, so a consumer must be able to show them as an
/// unchecked requirement rather than pretend the build is free.
///
/// Returns the map plus a count of key collisions. `path_to_info` is not
/// guaranteed one-to-one, so two recipes could in principle land on one slug;
/// the FIRST wins and the caller reports the rest rather than letting a later
/// row silently replace an earlier one. (Zero collisions were observed against
/// the live export when this was written.)
pub fn recipes_from_export(
    recipes: &Value,
    path_to_info: &HashMap<String, Value>,
) -> (HashMap<String, Value>, usize) {
    let alias = recipe_alias(recipes);
    let mut out: HashMap<String, Value> = HashMap::new();
    let mut collisions = 0usize;

    for row in manifest_rows_for(recipes, "ExportRecipes_en.json") {
        let unique = row.get("uniqueName").and_then(|v| v.as_str()).unwrap_or("");
        let result = row.get("resultType").and_then(|v| v.as_str()).unwrap_or("");
        // Product first (component recipes), then the recipe's own blueprint
        // (final assembly). Only recipes tied to something we can price are
        // useful here.
        let Some(info) = resolve_path(result, path_to_info, &alias)
            .or_else(|| resolve_path(unique, path_to_info, &alias))
        else {
            continue;
        };
        let Some(slug) = info.get("slug").and_then(|v| v.as_str()) else { continue };

        let mut entry = serde_json::Map::new();
        for (src, dst) in [
            ("buildPrice", "build_price"),
            ("buildTime", "build_time"),
            ("skipBuildTimePrice", "rush_price"),
        ] {
            if let Some(v) = row.get(src).and_then(|v| v.as_i64()) {
                entry.insert(dst.into(), Value::from(v));
            }
        }

        let ingredients: Vec<Value> = row
            .get("ingredients")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|ing| {
                        let path = ing.get("ItemType").and_then(|v| v.as_str())?;
                        let count = ing.get("ItemCount").and_then(|v| v.as_i64()).unwrap_or(1);
                        let hit = resolve_path(path, path_to_info, &alias);
                        let mut row = serde_json::Map::new();
                        row.insert(
                            "name".into(),
                            Value::String(
                                hit.and_then(|i| i.get("name"))
                                    .and_then(|v| v.as_str())
                                    .map(String::from)
                                    .unwrap_or_else(|| readable_from_path(path)),
                            ),
                        );
                        row.insert("count".into(), Value::from(count));
                        // Absent slug is meaningful: the ingredient cannot be
                        // bought, so a build plan must list it, not cost it.
                        if let Some(s) = hit.and_then(|i| i.get("slug")).and_then(|v| v.as_str()) {
                            row.insert("slug".into(), Value::String(s.to_string()));
                        }
                        Some(Value::Object(row))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !ingredients.is_empty() {
            entry.insert("ingredients".into(), Value::Array(ingredients));
        }
        if entry.is_empty() {
            continue;
        }
        if out.contains_key(slug) {
            collisions += 1;
            continue;
        }
        out.insert(slug.to_string(), Value::Object(entry));
    }
    (out, collisions)
}

// ---------------------------------------------------------------------------
// Dispositions
// ---------------------------------------------------------------------------

/// Lowercased weapon display name → riven disposition.
///
/// `omegaAttenuation` is the disposition, 0.50–1.55. Joining by display name
/// rather than uniqueName is deliberate: the riven surface is already keyed by
/// WFM slug via display name, so this drops straight in beside it.
pub fn dispositions_from_weapons(weapons: &Value) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    for row in manifest_rows_for(weapons, "ExportWeapons_en.json") {
        let (Some(name), Some(dispo)) = (
            row.get("name").and_then(|v| v.as_str()),
            row.get("omegaAttenuation").and_then(|v| v.as_f64()),
        ) else {
            continue;
        };
        // 0 shows up on non-riven-able entries; the real floor is 0.5.
        if dispo <= 0.0 {
            continue;
        }
        out.insert(name.to_lowercase(), dispo);
    }
    out
}

// ---------------------------------------------------------------------------
// Usage telemetry
// ---------------------------------------------------------------------------

/// Highest Mastery Rank bucket DE reports.
pub const MAX_MR: usize = 33;

/// Usage share per item, with its Mastery-Rank curve.
///
/// Answers the question price alone cannot: a part that is cheap and heavily
/// played sells today, while a part that is cheap and unplayed sits in your
/// listings for a month looking identical in every metric the app currently
/// shows.
///
/// Keyed by the WFM slug the display name resolves to. DE names the frame or
/// weapon ("Braton Prime"), and warframe.market lists the SET
/// ("Braton Prime Set"), so both spellings are tried - and the value is
/// therefore the PARENT's usage, which a consumer propagates to its parts.
/// Nothing is emitted for a name that does not resolve; a renamed weapon
/// misses rather than being guessed at.
///
/// Values are percentages of that category's total usage, rounded to four
/// decimals: the source is a fraction with far more digits than a telemetry
/// sample justifies, and the rounding keeps the surface from bloating
/// market.json with noise.
pub fn usage_from_export(
    doc: &Value,
    year: u16,
    catalog: &HashMap<String, String>,
) -> (HashMap<String, Value>, usize) {
    let mut out = HashMap::new();
    let mut unmatched = 0usize;

    let Some(all) = doc.get("ALL").and_then(|v| v.as_object()) else {
        return (out, 0);
    };

    for (category, items) in all {
        let Some(items) = items.as_object() else { continue };
        for (name, buckets) in items {
            let Some(buckets) = buckets.as_object() else { continue };
            let Some(share) = buckets.get("ALL").and_then(|v| v.as_f64())
                .filter(|share| share.is_finite() && *share >= 0.0)
            else { continue };
            let lower = name.to_lowercase();
            // DE says "Braton Prime"; WFM lists "Braton Prime Set".
            let Some(slug) = catalog
                .get(&lower)
                .or_else(|| catalog.get(&format!("{lower} set")))
            else {
                unmatched += 1;
                continue;
            };

            let mut by_mr = Vec::with_capacity(MAX_MR + 1);
            let mut peak_mr = 0usize;
            let mut peak = f64::NEG_INFINITY;
            for mr in 0..=MAX_MR {
                let v = buckets
                    .get(&mr.to_string())
                    .and_then(|v| v.as_f64())
                    .filter(|v| v.is_finite() && *v >= 0.0)
                    .unwrap_or(0.0);
                if v > peak {
                    peak = v;
                    peak_mr = mr;
                }
                by_mr.push(Value::from(round4(v * 100.0)));
            }

            out.insert(
                (*slug).clone(),
                serde_json::json!({
                    "name": name,
                    "category": category,
                    "year": year,
                    "share": round4(share * 100.0),
                    "peak_mr": peak_mr,
                    "by_mr": by_mr,
                }),
            );
        }
    }
    (out, unmatched)
}

/// Compact annual usage row for longitudinal comparisons. Historical years
/// deliberately omit the 34-element MR curve; the newest rich `usage` surface
/// remains the place for that detail.
pub fn usage_history_from_export(
    doc: &Value,
    catalog: &HashMap<String, String>,
) -> (HashMap<String, Value>, usize, usize) {
    let mut out = HashMap::new();
    let mut accepted = 0usize;
    let mut unmatched = 0usize;
    let Some(all) = doc.get("ALL").and_then(|v| v.as_object()) else {
        return (out, accepted, unmatched);
    };
    for (category, items) in all {
        let Some(items) = items.as_object() else { continue };
        for (name, buckets) in items {
            let Some(buckets) = buckets.as_object() else { continue };
            let Some(share) = buckets.get("ALL").and_then(|v| v.as_f64())
                .filter(|share| share.is_finite() && *share >= 0.0)
            else { continue };
            accepted += 1;
            let lower = name.to_lowercase();
            let Some(slug) = catalog
                .get(&lower)
                .or_else(|| catalog.get(&format!("{lower} set")))
            else {
                unmatched += 1;
                continue;
            };
            out.insert(
                (*slug).clone(),
                serde_json::json!({
                    "name": name,
                    "category": category,
                    "share": round4(share * 100.0),
                }),
            );
        }
    }
    (out, accepted, unmatched)
}

fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

// ---------------------------------------------------------------------------
// worldState → Baro
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct EventRewardBuild {
    pub rows: std::collections::BTreeMap<String, Value>,
    pub raw_rows: usize,
    pub invalid_rows: usize,
    pub unknown_rows: usize,
}

fn reward_items(
    reward: &Value,
    path_to_info: &HashMap<String, Value>,
    alias: &HashMap<String, String>,
) -> Option<(Vec<Value>, bool, i64)> {
    let obj = reward.as_object()?;
    let recognized = ["credits", "items", "countedItems"]
        .iter().any(|key| obj.contains_key(*key));
    if !recognized { return None; }
    let credits = match obj.get("credits") {
        Some(value) => value.as_i64().filter(|value| *value >= 0)?,
        None => 0,
    };
    let items: &[Value] = match obj.get("items") {
        Some(value) => value.as_array()?.as_slice(),
        None => &[],
    };
    let counted: &[Value] = match obj.get("countedItems") {
        Some(value) => value.as_array()?.as_slice(),
        None => &[],
    };
    let mut out = Vec::new();
    let mut complete = true;
    let mut push = |unique: &str, quantity: i64| {
        if quantity <= 0 { return false; }
        let mut row = serde_json::Map::new();
        row.insert("unique".into(), Value::String(unique.to_string()));
        row.insert("quantity".into(), Value::from(quantity));
        if let Some(info) = resolve_path(unique, path_to_info, alias) {
            if let Some(name) = info.get("name").and_then(|v| v.as_str()) {
                row.insert("name".into(), Value::String(name.to_string()));
            }
            if let Some(slug) = info.get("slug").and_then(|v| v.as_str()) {
                row.insert("slug".into(), Value::String(slug.to_string()));
            }
        } else {
            complete = false;
            row.insert("name".into(), Value::String(readable_from_path(unique)));
        }
        out.push(Value::Object(row));
        true
    };
    for item in items {
        if !item.as_str().is_some_and(|path| push(path, 1)) { return None; }
    }
    for item in counted {
        let unique = item.get("ItemType").and_then(|v| v.as_str())?;
        let quantity = item.get("ItemCount").and_then(|v| v.as_i64())?;
        if !push(unique, quantity) { return None; }
    }
    Some((out, complete, credits))
}

fn reward_group(kind: &str, threshold: Option<f64>, items: Vec<Value>, credits: i64) -> Value {
    let mut group = serde_json::Map::new();
    group.insert("kind".into(), Value::String(kind.to_string()));
    if let Some(threshold) = threshold { group.insert("threshold".into(), Value::from(threshold)); }
    if credits > 0 { group.insert("credits".into(), Value::from(credits)); }
    group.insert("rewards".into(), Value::Array(items));
    Value::Object(group)
}

fn world_id(row: &Value, source: &str) -> String {
    if let Some(id) = row.get("_id").and_then(|v| v.get("$oid")).and_then(|v| v.as_str()) {
        return id.to_string();
    }
    let tag = row.get("Tag").or_else(|| row.get("Prop")).and_then(|v| v.as_str()).unwrap_or("untagged");
    let start = de_millis(row.get("Activation").or_else(|| row.get("EventStartDate"))).unwrap_or(0);
    let end = de_millis(row.get("Expiry").or_else(|| row.get("EventEndDate"))).unwrap_or(0);
    format!("{source}:{tag}:{start}:{end}")
}

/// Extract only reward containers observed live on 2026-08-22: Goals.Reward,
/// Goals.BonusReward, and paired InterimGoals/InterimRewards. Events were
/// announcement rows with no reward-bearing field, so they are dated unknown
/// entries rather than being interpreted from an unobserved community schema.
pub fn event_rewards_from_world_child(
    world: &Value,
    key: &str,
    path_to_info: &HashMap<String, Value>,
    alias: &HashMap<String, String>,
    iso: impl Fn(i64) -> String,
) -> EventRewardBuild {
    let mut build = EventRewardBuild::default();
    let Some(rows) = world.get(key).and_then(|v| v.as_array()) else { return build };
    build.raw_rows = rows.len();
    for row in rows {
        let Some(obj) = row.as_object() else { build.invalid_rows += 1; continue };
        let start_field = if key == "Goals" { "Activation" } else { "EventStartDate" };
        let end_field = if key == "Goals" { "Expiry" } else { "EventEndDate" };
        let Some(start) = de_millis(obj.get(start_field)) else { build.invalid_rows += 1; continue };
        let Some(end) = de_millis(obj.get(end_field)) else { build.invalid_rows += 1; continue };
        if end <= start { build.invalid_rows += 1; continue }
        let source = if key == "Goals" { "goal" } else { "event" };
        let id = world_id(row, source);
        let title = if key == "Goals" {
            obj.get("Tag").or_else(|| obj.get("Desc")).and_then(|v| v.as_str())
                .map(readable_from_path)
        } else {
            obj.get("Messages").and_then(|v| v.as_array()).and_then(|messages| {
                messages.iter()
                    .find(|message| message.get("LanguageCode").and_then(|v| v.as_str()) == Some("en"))
                    .or_else(|| messages.first())
                    .and_then(|message| message.get("Message")).and_then(|v| v.as_str())
                    .map(str::to_string)
            }).or_else(|| obj.get("Prop").and_then(|v| v.as_str()).map(readable_from_path))
        }.unwrap_or_else(|| "Warframe event".into());
        if key != "Goals" {
            build.unknown_rows += 1;
            build.rows.insert(id.clone(), serde_json::json!({
                "id": id, "source": source, "title": title,
                "starts_at": iso(start), "ends_at": iso(end),
                "completeness": "unknown", "groups": [],
            }));
            continue;
        }
        let mut groups = Vec::new();
        let mut complete = true;
        let mut malformed = false;

        if obj.contains_key("InterimGoals") || obj.contains_key("InterimRewards") {
            match (
                obj.get("InterimGoals").and_then(|v| v.as_array()),
                obj.get("InterimRewards").and_then(|v| v.as_array()),
            ) {
                (Some(goals), Some(rewards)) if goals.len() == rewards.len() => {
                    for (threshold, reward) in goals.iter().zip(rewards) {
                        let Some(threshold) = threshold.as_f64() else { malformed = true; break };
                        let Some((items, resolved, credits)) = reward_items(reward, path_to_info, alias) else { malformed = true; break };
                        complete &= resolved;
                        groups.push(reward_group("milestone", Some(threshold), items, credits));
                    }
                }
                _ => malformed = true,
            }
        }
        for (field, kind) in [("Reward", "final"), ("BonusReward", "bonus")] {
            if let Some(reward) = obj.get(field) {
                let Some((items, resolved, credits)) = reward_items(reward, path_to_info, alias) else { malformed = true; break };
                complete &= resolved;
                groups.push(reward_group(kind, None, items, credits));
            }
        }
        if malformed { build.invalid_rows += 1; continue }
        if groups.is_empty() {
            build.unknown_rows += 1;
            build.rows.insert(id.clone(), serde_json::json!({
                "id": id, "source": source, "title": title,
                "starts_at": iso(start), "ends_at": iso(end),
                "completeness": "unknown", "groups": [],
            }));
            continue;
        }
        build.rows.insert(id.clone(), serde_json::json!({
            "id": id, "source": source, "title": title,
            "starts_at": iso(start), "ends_at": iso(end),
            "completeness": if complete { "complete" } else { "partial" },
            "groups": groups,
        }));
    }
    build
}

// ---------------------------------------------------------------------------
// worldState → Baro
// ---------------------------------------------------------------------------

/// Build the `baro` surface from worldState.
///
/// The upgrade over the drop-in it replaces: worldState carries the manifest
/// **from announcement**, so the stock is known days before he lands, where the
/// old source returned an empty list between visits and published no schedule
/// at all. Rows keep the same `{item, ducats, credits}` shape the SPA already
/// renders, and gain `unique` so a consumer can join without a name match.
///
/// Cosmetics and bundles resolve to no slug. They are kept with a name and no
/// price rather than dropped - a missing row reads as "he isn't selling it".
pub fn baro_from_world(
    world: &Value,
    path_to_info: &HashMap<String, Value>,
    alias: &HashMap<String, String>,
    iso: impl Fn(i64) -> String,
) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    let Some(trader) = world.get("VoidTraders").and_then(|v| v.as_array()).and_then(|a| a.first())
    else {
        return out;
    };
    let (Some(activation), Some(expiry)) = (
        de_millis(trader.get("Activation")),
        de_millis(trader.get("Expiry")),
    ) else {
        return out;
    };
    let node = trader.get("Node").and_then(|v| v.as_str()).unwrap_or("");

    let activation_iso = iso(activation);
    out.insert("activation".into(), Value::String(activation_iso.clone()));
    out.insert("expiry".into(), Value::String(iso(expiry)));
    out.insert("location".into(), Value::String(relay_name(node)));
    if let Some(ch) = trader.get("Character").and_then(|v| v.as_str()) {
        out.insert("character".into(), Value::String(ch.to_string()));
    }

    let mut inventory = Vec::new();
    for entry in trader.get("Manifest").and_then(|v| v.as_array()).unwrap_or(&Vec::new()) {
        let Some(path) = entry.get("ItemType").and_then(|v| v.as_str()) else {
            continue;
        };
        let info = resolve_path(path, path_to_info, alias);
        let name = info
            .and_then(|i| i.get("name"))
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| readable_from_path(path));

        let mut row = serde_json::Map::new();
        row.insert("item".into(), Value::String(name));
        row.insert("unique".into(), Value::String(path.to_string()));
        if let Some(slug) = info.and_then(|i| i.get("slug")).and_then(|v| v.as_str()) {
            row.insert("slug".into(), Value::String(slug.to_string()));
        }
        if let Some(d) = entry.get("PrimePrice").and_then(|v| v.as_i64()) {
            row.insert("ducats".into(), Value::from(d));
        }
        if let Some(c) = entry.get("RegularPrice").and_then(|v| v.as_i64()) {
            row.insert("credits".into(), Value::from(c));
        }
        inventory.push(Value::Object(row));
    }

    if !inventory.is_empty() {
        out.insert("inventory".into(), Value::Array(inventory));
        out.insert("inventory_for".into(), Value::String(activation_iso));
    }
    out
}

/// Last-resort display name: split the final path segment on camel case.
///
/// Only used for items with no catalogue entry (cosmetics, bundles). Better
/// than showing a raw `/Lotus/...` path, and honest that it is derived - the
/// row carries no slug and therefore no price.
pub fn readable_from_path(path: &str) -> String {
    let seg = path.rsplit('/').next().unwrap_or(path);
    let mut out = String::new();
    for (i, ch) in seg.chars().enumerate() {
        if i > 0 && ch.is_uppercase() && !out.ends_with(' ') {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

// ---------------------------------------------------------------------------
// worldState → vault rotation
// ---------------------------------------------------------------------------

/// Prime Vault rotation, announced rather than estimated.
///
/// The surface it supplements derives `vaulting-soon` from an estimated vault
/// date. When DE has actually announced a rotation, this is the real thing -
/// which matters because an unvaulting is the most expensive surprise in prime
/// trading, and the estimate can be weeks out.
pub fn vault_rotation_from_world(world: &Value, iso: impl Fn(i64) -> String) -> Vec<Value> {
    let mut out = Vec::new();
    for trader in world.get("PrimeVaultTraders").and_then(|v| v.as_array()).unwrap_or(&Vec::new()) {
        let Some(activation) = de_millis(trader.get("Activation")) else {
            continue;
        };
        let items: Vec<Value> = trader
            .get("Manifest")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|e| e.get("ItemType").and_then(|v| v.as_str()))
                    .map(|s| Value::String(s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        if items.is_empty() {
            continue;
        }
        let mut row = serde_json::Map::new();
        row.insert("activation".into(), Value::String(iso(activation)));
        if let Some(e) = de_millis(trader.get("Expiry")) {
            row.insert("expiry".into(), Value::String(iso(e)));
        }
        row.insert("items".into(), Value::Array(items));
        out.push(Value::Object(row));
    }
    out
}

// ---------------------------------------------------------------------------
// worldState → dated events
// ---------------------------------------------------------------------------

/// Dated, price-relevant events: Darvo's daily deal and the void trader window.
///
/// Deliberately narrow. `Goals`/`Events` reward tables vary by event type and
/// several carry no usable table at all, so parsing them into "this item is
/// about to be given away" is a separate piece of work with its own failure
/// modes - better absent than wrong.
pub fn deals_from_world(
    world: &Value,
    path_to_info: &HashMap<String, Value>,
    alias: &HashMap<String, String>,
    iso: impl Fn(i64) -> String,
) -> Vec<Value> {
    let mut out = Vec::new();
    for deal in world.get("DailyDeals").and_then(|v| v.as_array()).unwrap_or(&Vec::new()) {
        let Some(path) = deal.get("StoreItem").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(expiry) = de_millis(deal.get("Expiry")) else { continue };
        let info = resolve_path(path, path_to_info, alias);
        let mut row = serde_json::Map::new();
        row.insert(
            "item".into(),
            Value::String(
                info.and_then(|i| i.get("name"))
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| readable_from_path(path)),
            ),
        );
        row.insert("expiry".into(), Value::String(iso(expiry)));
        for (src, dst) in [
            ("Discount", "discount"),
            ("OriginalPrice", "original_price"),
            ("SalePrice", "sale_price"),
            ("AmountTotal", "stock"),
            ("AmountSold", "sold"),
        ] {
            if let Some(v) = deal.get(src).and_then(|v| v.as_i64()) {
                row.insert(dst.into(), Value::from(v));
            }
        }
        out.push(Value::Object(row));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iso(ms: i64) -> String {
        format!("ms:{ms}")
    }

    fn p2i() -> HashMap<String, Value> {
        let mut m = HashMap::new();
        m.insert(
            "/Lotus/Types/Recipes/WarframeRecipes/OberonPrimeSystemsComponent".to_string(),
            serde_json::json!({"name": "Oberon Prime Systems Blueprint", "slug": "oberon_prime_systems_blueprint"}),
        );
        m.insert(
            "/Lotus/Upgrades/Mods/Pistol/Expert/WeaponReloadSpeedModExpert".to_string(),
            serde_json::json!({"name": "Primed Quickdraw", "slug": "primed_quickdraw"}),
        );
        m
    }

    fn recipes() -> Value {
        serde_json::json!({"ExportRecipes": [
            {"uniqueName": "/Lotus/Types/Recipes/WarframeRecipes/OberonPrimeSystemsBlueprint",
             "resultType": "/Lotus/Types/Recipes/WarframeRecipes/OberonPrimeSystemsComponent",
             "primeSellingPrice": 15, "buildPrice": 15000, "buildTime": 43200,
             "skipBuildTimePrice": 25,
             "ingredients": [{"ItemType": "/Lotus/Types/Items/MiscItems/OrokinCell",
                              "ItemCount": 2, "ProductCategory": "MiscItems"}]},
            {"uniqueName": "/Lotus/Types/Recipes/WarframeRecipes/NovaPrimeChassisBlueprint",
             "resultType": "/Lotus/Types/Recipes/WarframeRecipes/NovaPrimeChassisComponent",
             "primeSellingPrice": 100},
            {"uniqueName": "/Lotus/Types/Recipes/Components/FormaBlueprint",
             "resultType": "/Lotus/Types/Recipes/Components/Forma"}
        ]})
    }

    #[test]
    fn ducats_key_on_the_recipe_not_its_result() {
        // Nova Prime Chassis Blueprint is 100 ducats; the component it builds
        // has no ducat value. Keying the wrong one is a wrong number, silently.
        let d = ducats_from_recipes(&recipes());
        assert_eq!(
            d.get("/Lotus/Types/Recipes/WarframeRecipes/NovaPrimeChassisBlueprint"),
            Some(&100)
        );
        assert!(!d.contains_key("/Lotus/Types/Recipes/WarframeRecipes/NovaPrimeChassisComponent"));
    }

    #[test]
    fn ducats_reject_values_off_the_known_tiers() {
        let odd = serde_json::json!({"ExportRecipes": [
            {"uniqueName": "/a", "primeSellingPrice": 37}
        ]});
        assert!(ducats_from_recipes(&odd).is_empty());
    }

    #[test]
    fn resolve_path_walks_store_alias_then_recipe_alias() {
        let info = p2i();
        let alias = recipe_alias(&recipes());
        // The path relic tables actually use: store alias + blueprint name.
        let hit = resolve_path(
            "/Lotus/StoreItems/Types/Recipes/WarframeRecipes/OberonPrimeSystemsBlueprint",
            &info,
            &alias,
        );
        assert_eq!(hit.unwrap()["slug"], "oberon_prime_systems_blueprint");
    }

    #[test]
    fn resolve_path_returns_none_rather_than_guessing() {
        let info = p2i();
        let alias = recipe_alias(&recipes());
        assert!(resolve_path("/Lotus/Types/Recipes/Components/FormaBlueprint", &info, &alias).is_none());
    }

    #[test]
    fn relic_slug_matches_the_existing_key_shape() {
        assert_eq!(relic_slug("Lith H2 Relic").as_deref(), Some("lith_h2_relic"));
        assert_eq!(relic_slug("Axi A1 Relic").as_deref(), Some("axi_a1_relic"));
        assert_eq!(relic_slug("Not A Thing"), None);
    }

    #[test]
    fn relic_rewards_carry_four_refinements_and_correct_rarity() {
        let relics = serde_json::json!({"ExportRelicArcane": [
            {"name": "Lith H2 Relic",
             "uniqueName": "/Lotus/Types/Game/Projections/T1VoidProjectionXBronze",
             "relicRewards": [
               {"rewardName": "/Lotus/StoreItems/Types/Recipes/WarframeRecipes/OberonPrimeSystemsBlueprint",
                "rarity": "COMMON", "tier": 0, "itemCount": 1},
               {"rewardName": "/Lotus/Types/Recipes/Components/FormaBlueprint",
                "rarity": "COMMON", "tier": 0, "itemCount": 1}
             ]},
            // The Gold variant is the same relic at a different refinement and
            // must not double the reward list.
            {"name": "Lith H2 Relic",
             "uniqueName": "/Lotus/Types/Game/Projections/T1VoidProjectionXGold",
             "relicRewards": [
               {"rewardName": "/Lotus/StoreItems/Types/Recipes/WarframeRecipes/OberonPrimeSystemsBlueprint",
                "rarity": "COMMON", "tier": 0, "itemCount": 1}
             ]}
        ]});
        let build = relic_rewards_from_de(&relics, &recipes(), &p2i());

        assert_eq!(build.rewards.len(), 1, "the four variants are one relic");
        let rows = build.rewards["lith_h2_relic"].as_array().unwrap();
        assert_eq!(rows.len(), 1, "Forma is untradeable and stays unresolved");
        assert_eq!(rows[0]["rarity"], "Common", "not the old source's 'Uncommon'");
        assert_eq!(rows[0]["chance"], 25.33, "bare `chance` stays intact for old consumers");
        assert_eq!(rows[0]["chances"]["radiant"], 16.67);
        assert_eq!(rows[0]["chances"]["intact"], 25.33);
        assert_eq!(build.unresolved, 1);
    }

    #[test]
    fn recipes_key_on_the_item_produced_not_the_blueprint() {
        // "What does it cost me to end up with a Volt Prime Chassis" has to be
        // reachable from the part the user is looking at.
        let (r, collisions) = recipes_from_export(&recipes(), &p2i());
        assert_eq!(collisions, 0);
        let entry = r
            .get("oberon_prime_systems_blueprint")
            .expect("keyed by the produced item's slug");
        assert_eq!(entry["build_price"], 15000);
        assert_eq!(entry["build_time"], 43200);
        assert_eq!(entry["rush_price"], 25);
    }

    #[test]
    fn recipe_ingredients_keep_untradeable_items_visible_but_unpriced() {
        let (r, _) = recipes_from_export(&recipes(), &p2i());
        let ing = r["oberon_prime_systems_blueprint"]["ingredients"].as_array().unwrap();
        assert_eq!(ing.len(), 1);
        // Orokin Cell has no market slug. It must still be listed, or a build
        // plan silently claims a build is free when it is not.
        assert_eq!(ing[0]["name"], "Orokin Cell");
        assert_eq!(ing[0]["count"], 2);
        assert!(ing[0].get("slug").is_none());
    }

    #[test]
    fn recipes_skip_products_we_cannot_price() {
        // Forma resolves to nothing, so it contributes no row rather than a
        // row with no slug.
        let (r, _) = recipes_from_export(&recipes(), &p2i());
        assert!(!r.keys().any(|k| k.contains("forma")));
    }

    #[test]
    fn final_assembly_recipes_key_on_the_blueprint_you_trade() {
        // A finished Warframe is not a tradeable item, so its recipe's
        // resultType resolves to nothing. Its identity is the blueprint -
        // and without this the last and most expensive step of every build
        // silently vanished from the cost.
        let mut info = p2i();
        info.insert(
            "/Lotus/Types/Recipes/WarframeRecipes/NovaPrimeBlueprint".to_string(),
            serde_json::json!({"name": "Nova Prime Blueprint", "slug": "nova_prime_blueprint"}),
        );
        let recipes = serde_json::json!({"ExportRecipes": [
            {"uniqueName": "/Lotus/Types/Recipes/WarframeRecipes/NovaPrimeBlueprint",
             "resultType": "/Lotus/Powersuits/AntiMatter/NovaPrime",
             "buildPrice": 25000, "buildTime": 259200, "skipBuildTimePrice": 50}
        ]});
        let (r, _) = recipes_from_export(&recipes, &info);
        let entry = r.get("nova_prime_blueprint").expect("final assembly is costed");
        assert_eq!(entry["build_price"], 25000);
        assert_eq!(entry["rush_price"], 50);
    }

    #[test]
    fn colliding_recipes_keep_the_first_and_are_counted() {
        let mut info = p2i();
        info.insert(
            "/a".to_string(),
            serde_json::json!({"name": "A", "slug": "same_slug"}),
        );
        info.insert(
            "/b".to_string(),
            serde_json::json!({"name": "B", "slug": "same_slug"}),
        );
        let recipes = serde_json::json!({"ExportRecipes": [
            {"uniqueName": "/r1", "resultType": "/a", "buildPrice": 15000},
            {"uniqueName": "/r2", "resultType": "/b", "buildPrice": 5000}
        ]});
        let (r, collisions) = recipes_from_export(&recipes, &info);
        assert_eq!(collisions, 1, "the second is reported, not silently applied");
        assert_eq!(r["same_slug"]["build_price"], 15000, "first wins, deterministically");
    }

    #[test]
    fn usage_joins_through_the_set_name_wfm_actually_lists() {
        // DE says "Braton Prime"; warframe.market lists "Braton Prime Set".
        let mut catalog = HashMap::new();
        catalog.insert("braton prime set".to_string(), "braton_prime_set".to_string());
        catalog.insert("torid".to_string(), "torid".to_string());

        let mut buckets = serde_json::Map::new();
        buckets.insert("ALL".into(), Value::from(0.0342));
        for mr in 0..=MAX_MR {
            // Peaks at MR 10 - a starter-tier weapon's audience.
            let v = if mr == 10 { 0.09 } else { 0.001 };
            buckets.insert(mr.to_string(), Value::from(v));
        }
        let doc = serde_json::json!({"ALL": {
            "Primary": {
                "Braton Prime": Value::Object(buckets),
                "Torid": {"ALL": 0.0962},
                "SomeRenamedGun": {"ALL": 0.01}
            }
        }});

        let (u, unmatched) = usage_from_export(&doc, 2025, &catalog);
        assert_eq!(unmatched, 1, "a name that does not resolve misses, it is not guessed");
        let braton = &u["braton_prime_set"];
        assert_eq!(braton["share"], 3.42, "stored as a percentage");
        assert_eq!(braton["peak_mr"], 10);
        assert_eq!(braton["category"], "Primary");
        assert_eq!(braton["year"], 2025);
        assert_eq!(braton["by_mr"].as_array().unwrap().len(), MAX_MR + 1);
        assert_eq!(u["torid"]["share"], 9.62);
    }

    #[test]
    fn usage_is_empty_rather_than_wrong_on_an_unexpected_shape() {
        let (u, n) = usage_from_export(&serde_json::json!({"nope": 1}), 2025, &HashMap::new());
        assert!(u.is_empty());
        assert_eq!(n, 0);
    }

    #[test]
    fn compact_usage_is_strict_and_omits_mastery_buckets() {
        let catalog = HashMap::from([
            ("direct".into(), "direct".into()),
            ("parent set".into(), "parent_set".into()),
        ]);
        let doc = serde_json::json!({"ALL":{"Primary":{
            "Direct":{"ALL":0.1,"0":0.2},
            "Parent":{"ALL":0.2},
            "Missing":{"ALL":0.3},
            "Negative":{"ALL":-0.1},
            "Malformed":{"ALL":"bad"}
        }}});
        let (rows, accepted, unmatched) = usage_history_from_export(&doc, &catalog);
        assert_eq!(accepted, 3);
        assert_eq!(unmatched, 1);
        assert_eq!(rows["direct"], serde_json::json!({"name":"Direct","category":"Primary","share":10.0}));
        assert_eq!(rows["parent_set"]["share"], 20.0);
        assert!(rows.values().all(|row| row.get("by_mr").is_none()));
    }

    #[test]
    fn live_shape_fixture_preserves_goal_reward_groups() {
        let world: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/de-events/world-shapes.json"
        ))).unwrap();
        let p2i = HashMap::from([
            ("/Lotus/Weapons/TestFinalWeapon".into(), serde_json::json!({"name":"Test Final Weapon","slug":"test_final_weapon"})),
            ("/Lotus/Upgrades/Mods/TestMilestoneMod".into(), serde_json::json!({"name":"Test Milestone Mod","slug":"test_milestone_mod"})),
        ]);
        let goals = event_rewards_from_world_child(&world, "Goals", &p2i, &HashMap::new(), |ms| ms.to_string());
        assert_eq!(goals.raw_rows, 3);
        assert_eq!(goals.rows.len(), 3);
        assert_eq!(goals.unknown_rows, 1, "Jobs reward tables are observed but not fixed containers");
        let first = &goals.rows["redacted-goal-milestones"];
        assert_eq!(first["groups"].as_array().unwrap().len(), 3);
        assert_eq!(first["groups"][0]["kind"], "milestone");
        assert_eq!(first["groups"][0]["threshold"], 5.0);
        assert_eq!(first["groups"][2]["kind"], "final");
        assert_eq!(first["groups"][2]["rewards"][0]["slug"], "test_final_weapon");
        assert_eq!(first["completeness"], "partial", "an unresolved token keeps reach conservative");
        assert_eq!(first["groups"][1]["credits"], 50_000);
        assert_eq!(goals.rows["redacted-goal-bonus"]["groups"][0]["credits"], 50_000);
        assert_eq!(goals.rows["redacted-goal-jobs"]["completeness"], "unknown");

        let events = event_rewards_from_world_child(&world, "Events", &p2i, &HashMap::new(), |ms| ms.to_string());
        assert_eq!(events.rows["redacted-announcement"]["title"], "Redacted announcement");
        assert_eq!(events.rows["redacted-announcement"]["completeness"], "unknown");
        assert_eq!(events.unknown_rows, 1, "live Events were announcements, not reward rows");

        let credit_only = serde_json::json!({"Goals":[{
            "_id":{"$oid":"credits"}, "Activation":1, "Expiry":2, "Tag":"CreditGoal",
            "Reward":{"credits":50000,"items":[],"countedItems":[]}
        }]});
        let credit_goal = event_rewards_from_world_child(&credit_only, "Goals", &HashMap::new(), &HashMap::new(), |ms| ms.to_string());
        assert_eq!(credit_goal.rows["credits"]["groups"][0]["credits"], 50_000);
        assert_eq!(credit_goal.rows["credits"]["groups"][0]["rewards"].as_array().unwrap().len(), 0);

        let empty_reward = serde_json::json!({"Goals":[{
            "_id":{"$oid":"empty"}, "Activation":1, "Expiry":2, "Tag":"EmptyGoal", "Reward":{}
        }]});
        let rejected = event_rewards_from_world_child(&empty_reward, "Goals", &HashMap::new(), &HashMap::new(), |ms| ms.to_string());
        assert!(rejected.rows.is_empty());
        assert_eq!(rejected.invalid_rows, 1);

        let explicit_zero = serde_json::json!({"Goals":[{
            "_id":{"$oid":"zero"}, "Activation":1, "Expiry":2, "Tag":"ZeroGoal",
            "Reward":{"credits":0,"items":[],"countedItems":[]}
        }]});
        let accepted = event_rewards_from_world_child(&explicit_zero, "Goals", &HashMap::new(), &HashMap::new(), |ms| ms.to_string());
        assert_eq!(accepted.invalid_rows, 0);
        assert_eq!(accepted.rows["zero"]["completeness"], "complete");
        assert_eq!(accepted.rows["zero"]["groups"][0]["rewards"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn dispositions_skip_non_riven_weapons() {
        let w = serde_json::json!({"ExportWeapons": [
            {"name": "Kuva Nukor", "omegaAttenuation": 0.7},
            {"name": "Prisma Grakata", "omegaAttenuation": 1.3},
            {"name": "Some Fixture", "omegaAttenuation": 0.0}
        ]});
        let d = dispositions_from_weapons(&w);
        assert_eq!(d.len(), 2);
        assert_eq!(d["kuva nukor"], 0.7);
    }

    fn world() -> Value {
        serde_json::json!({
          "VoidTraders": [{
            "Activation": {"$date": {"$numberLong": "1787317200000"}},
            "Expiry": {"$date": {"$numberLong": "1787490000000"}},
            "Character": "Baro'Ki Teel",
            "Node": "PlutoHUB",
            "Manifest": [
              {"ItemType": "/Lotus/StoreItems/Upgrades/Mods/Pistol/Expert/WeaponReloadSpeedModExpert",
               "PrimePrice": 375, "RegularPrice": 120000},
              {"ItemType": "/Lotus/StoreItems/Types/StoreItems/CreditBundles/KiteerSekhara",
               "PrimePrice": 200, "RegularPrice": 50000}
            ]
          }],
          "PrimeVaultTraders": [{
            "Activation": {"$date": {"$numberLong": "1786039200000"}},
            "Manifest": [{"ItemType": "/Lotus/Types/StoreItems/Packages/MPVRevenantPrimeSinglePack",
                          "PrimePrice": 0}]
          }],
          "DailyDeals": [{
            "StoreItem": "/Lotus/Weapons/Tenno/Pistol/RevolverPistol",
            "Expiry": {"$date": {"$numberLong": "1787410800000"}},
            "Discount": 40, "OriginalPrice": 190, "SalePrice": 114,
            "AmountTotal": 100, "AmountSold": 13
          }]
        })
    }

    #[test]
    fn baro_keeps_the_existing_shape_and_prices_the_manifest() {
        let alias = recipe_alias(&recipes());
        let b = baro_from_world(&world(), &p2i(), &alias, iso);
        assert_eq!(b["location"], "Pluto Relay");
        assert_eq!(b["activation"], "ms:1787317200000");
        let inv = b["inventory"].as_array().unwrap();
        assert_eq!(inv.len(), 2);
        assert_eq!(inv[0]["item"], "Primed Quickdraw");
        assert_eq!(inv[0]["slug"], "primed_quickdraw");
        assert_eq!(inv[0]["ducats"], 375);
        assert_eq!(inv[0]["credits"], 120000);
    }

    #[test]
    fn baro_keeps_unpriceable_cosmetics_without_inventing_a_slug() {
        let alias = recipe_alias(&recipes());
        let b = baro_from_world(&world(), &p2i(), &alias, iso);
        let inv = b["inventory"].as_array().unwrap();
        // Readable, but explicitly no slug - so nothing downstream can price it.
        assert_eq!(inv[1]["item"], "Kiteer Sekhara");
        assert!(inv[1].get("slug").is_none());
    }

    #[test]
    fn baro_is_empty_when_worldstate_has_no_trader() {
        let empty = serde_json::json!({"VoidTraders": []});
        assert!(baro_from_world(&empty, &p2i(), &HashMap::new(), iso).is_empty());
    }

    #[test]
    fn vault_rotation_reads_the_announced_manifest() {
        let v = vault_rotation_from_world(&world(), iso);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0]["items"].as_array().unwrap().len(), 1);
        assert_eq!(v[0]["activation"], "ms:1786039200000");
    }

    #[test]
    fn deals_carry_the_discount_fields() {
        let d = deals_from_world(&world(), &p2i(), &HashMap::new(), iso);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0]["discount"], 40);
        assert_eq!(d[0]["sale_price"], 114);
        assert_eq!(d[0]["item"], "Revolver Pistol");
    }
}
