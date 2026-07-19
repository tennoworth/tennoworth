//! Fetch stages — each upstream mirrored from `csv_to_market_json.py`.
//!
//! Every function accepting `Http` can be swapped for a fixture in tests.
//! The live implementation uses `wfm_client` primitives (browser UA,
//! headers, envelope unwrap, retry). The trait is intentionally narrow:
//! one GET→JSON method — that's all the Python converter does.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::clock;
use crate::render::CatalogItemMeta;

/// Narrow GET interface so every fetch stage is testable offline.
pub trait Http {
    fn get_json(&self, url: &str) -> Result<serde_json::Value, String>;
}

/// Live implementation using `wfm_client`.
pub struct LiveHttp {
    pub client: reqwest::blocking::Client,
}

impl Http for LiveHttp {
    fn get_json(&self, url: &str) -> Result<serde_json::Value, String> {
        let resp = self
            .client
            .get(url)
            .send()
            .map_err(|e| format!("{url}: {e}"))?;
        let status = resp.status();
        let body = resp
            .text()
            .map_err(|e| format!("{url}: read body: {e}"))?;
        if !status.is_success() {
            return Err(format!("{url}: HTTP {status}: {body}"));
        }
        serde_json::from_str(&body).map_err(|e| format!("{url}: JSON parse: {e}"))
    }
}

/// Fetch WFM catalog (`/v2/items`) — returns name→slug catalog AND
/// per-item metadata (tags, ducats, max_rank, subtypes).
///
/// Retries 3× with backoff, matching Python's `fetch_catalog`. On total
/// failure, returns `None` so the caller can fall back to the prior
/// snapshot's catalog + items.
pub fn fetch_catalog_wfm(
    http: &dyn Http,
    url: &str,
) -> Result<(HashMap<String, String>, HashMap<String, CatalogItemMeta>), String> {
    let mut last_err = String::new();
    for attempt in 0..3u32 {
        match http.get_json(url) {
            Ok(body) => {
                let items = wfm_client::unwrap_envelope(&body);
                let arr = items.as_array().ok_or_else(|| format!("{url}: not an array"))?;
                let mut catalog = HashMap::new();
                let mut meta = HashMap::new();
                for it in arr {
                    let slug = it.get("slug").and_then(|s| s.as_str()).unwrap_or("");
                    let nm = it
                        .get("i18n")
                        .and_then(|i| i.get("en"))
                        .and_then(|n| n.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("");
                    if !slug.is_empty() && !nm.is_empty() {
                        catalog.insert(nm.to_lowercase(), slug.to_string());
                    }
                    if !slug.is_empty() {
                        let tags: Vec<String> = it
                            .get("tags")
                            .and_then(|t| t.as_array())
                            .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                            .unwrap_or_default();
                        meta.insert(
                            slug.to_string(),
                            CatalogItemMeta {
                                tags,
                                ducats: it.get("ducats").and_then(|d| d.as_i64()),
                                max_rank: it.get("maxRank").and_then(|r| r.as_i64()),
                                subtypes: it
                                    .get("subtypes")
                                    .and_then(|s| s.as_array())
                                    .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                                    .unwrap_or_default(),
                            },
                        );
                    }
                }
                return Ok((catalog, meta));
            }
            Err(e) => {
                last_err = e;
                if attempt + 1 < 3 {
                    std::thread::sleep(std::time::Duration::from_secs(2 * (attempt as u64 + 1)));
                }
            }
        }
    }
    Err(last_err)
}

/// Fetch warframestat parent endpoints → path_to_info + set_to_parts.
/// Returns `(path_to_info, set_to_parts, complete)` — `complete` is false
/// when any endpoint failed.
pub fn fetch_parent_data(
    http: &dyn Http,
    catalog: &HashMap<String, String>,
) -> (HashMap<String, serde_json::Value>, HashMap<String, serde_json::Value>, bool) {
    let endpoints = [
        ("https://api.warframestat.us/warframes/", "Warframes"),
        ("https://api.warframestat.us/weapons/", "Weapons"),
        ("https://api.warframestat.us/sentinels/", "Sentinels"),
    ];
    let mut path_to_info: HashMap<String, serde_json::Value> = HashMap::new();
    let mut set_to_parts: HashMap<String, serde_json::Value> = HashMap::new();
    let mut complete = true;

    for (url, fallback_cat) in &endpoints {
        let arr = match http.get_json(url) {
            Ok(body) => body,
            Err(e) => {
                eprintln!("  warning: could not fetch {url}: {e}");
                complete = false;
                continue;
            }
        };
        let items = match arr.as_array() {
            Some(a) => a,
            None => {
                eprintln!("  warning: {url} returned non-list (skipping)");
                complete = false;
                continue;
            }
        };
        for parent in items {
            let parent_name = parent.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if !parent_name.contains("Prime") {
                continue;
            }
            let parent_cat = parent.get("category").and_then(|c| c.as_str()).unwrap_or(fallback_cat);
            let set_slug = catalog.get(&format!("{} set", parent_name.to_lowercase()));

            let mut this_set_parts: Vec<serde_json::Value> = Vec::new();
            for comp in parent.get("components").and_then(|c| c.as_array()).unwrap_or(&vec![]) {
                let un = comp.get("uniqueName").and_then(|u| u.as_str()).unwrap_or("");
                let cn = comp.get("name").and_then(|n| n.as_str()).unwrap_or("");
                if un.is_empty() || cn.is_empty() {
                    continue;
                }
                if un.starts_with("/Lotus/Types/Items/MiscItems/") {
                    continue;
                }
                let full_name = format!("{parent_name} {cn}");
                let slug = catalog
                    .get(&format!("{} blueprint", full_name.to_lowercase()))
                    .or_else(|| catalog.get(&full_name.to_lowercase()))
                    .or(set_slug)
                    .cloned();
                let slug = match slug {
                    Some(s) => s,
                    None => continue,
                };
                let mut display_name = full_name.clone();
                if slug.ends_with("_set") && !full_name.ends_with("Set") {
                    display_name = format!("{full_name} → set");
                } else if slug.ends_with("_blueprint") && !full_name.ends_with("Blueprint") {
                    display_name = format!("{full_name} Blueprint");
                }
                path_to_info.insert(
                    un.to_string(),
                    serde_json::json!({"name": display_name, "slug": slug, "category": parent_cat}),
                );
                if set_slug != Some(&slug) {
                    this_set_parts.push(serde_json::json!({"slug": slug, "component_name": cn}));
                }
            }
            if let (Some(ss), false) = (set_slug, this_set_parts.is_empty()) {
                set_to_parts.insert(
                    ss.clone(),
                    serde_json::json!({"name": parent_name, "parts": this_set_parts}),
                );
            }
        }
    }
    (path_to_info, set_to_parts, complete)
}

/// Fetch relic drop tables (Intact state only) from drops.warframestat.us.
/// Returns {} on any failure — the relic planner UI degrades gracefully.
pub fn fetch_relic_rewards(
    http: &dyn Http,
    catalog: &HashMap<String, String>,
) -> HashMap<String, serde_json::Value> {
    let url = "https://drops.warframestat.us/data/relics.json";
    let body = match http.get_json(url) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("  warning: could not fetch {url}: {e}");
            return HashMap::new();
        }
    };
    let rows = match body.get("relics").and_then(|r| r.as_array()) {
        Some(a) => a,
        None => {
            eprintln!("  warning: relics.json unexpected shape");
            return HashMap::new();
        }
    };
    let mut out = HashMap::new();
    for row in rows {
        if row.get("state").and_then(|s| s.as_str()) != Some("Intact") {
            continue;
        }
        let tier = row.get("tier").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
        let name = row.get("relicName").and_then(|n| n.as_str()).unwrap_or("").to_lowercase();
        if tier.is_empty() || name.is_empty() {
            continue;
        }
        let relic_slug = format!("{tier}_{name}_relic");
        let mut rewards = Vec::new();
        for r in row.get("rewards").and_then(|r| r.as_array()).unwrap_or(&vec![]) {
            let reward_name = r.get("itemName").and_then(|n| n.as_str()).unwrap_or("");
            if reward_name.is_empty() {
                continue;
            }
            let reward_slug = catalog
                .get(&reward_name.to_lowercase())
                .or_else(|| catalog.get(&format!("{} blueprint", reward_name.to_lowercase())))
                .cloned();
            let reward_slug = match reward_slug {
                Some(s) => s,
                None => continue,
            };
            rewards.push(serde_json::json!({
                "reward_slug": reward_slug,
                "reward_name": reward_name,
                "rarity": r.get("rarity").and_then(|ra| ra.as_str()).unwrap_or(""),
                "chance": r.get("chance").and_then(|c| c.as_f64()).unwrap_or(0.0),
            }));
        }
        if !rewards.is_empty() {
            out.insert(relic_slug, serde_json::Value::Array(rewards));
        }
    }
    out
}

/// Fetch prime vault status from WFCD warframe-items sources.
/// Returns `(vault_status, complete)` — `complete` false when any source
/// failed, so the caller can merge with prior.
pub fn fetch_vault_status(
    http: &dyn Http,
    catalog: &HashMap<String, String>,
    now: DateTime<Utc>,
) -> (HashMap<String, String>, bool) {
    let urls = [
        "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/Warframes.json",
        "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/Primary.json",
        "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/Secondary.json",
        "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/Melee.json",
        "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/Archwing.json",
        "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/Arch-Gun.json",
        "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/Arch-Melee.json",
        "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/SentinelWeapons.json",
        "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/Sentinels.json",
        "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/Pets.json",
    ];
    let vault_soon_cutoff = now + chrono::Duration::days(60);
    let mut out = HashMap::new();
    let mut complete = true;

    for url in &urls {
        let arr = match http.get_json(url) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("  warning: could not fetch {url}: {e}");
                complete = false;
                continue;
            }
        };
        let items = match arr.as_array() {
            Some(a) => a,
            None => {
                complete = false;
                continue;
            }
        };
        for parent in items {
            let parent_name = parent.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if !parent_name.contains("Prime") {
                continue;
            }
            let vaulted = parent.get("vaulted").and_then(|v| v.as_bool()).unwrap_or(false);
            let est_raw = parent.get("estimatedVaultDate").and_then(|d| d.as_str());
            let mut soon = false;
            if !vaulted {
                if let Some(est) = est_raw {
                    let fixed = est.replace('Z', "+00:00");
                    if let Some(est_dt) = clock::parse_isoformat_utc(&fixed) {
                        if est_dt < vault_soon_cutoff {
                            soon = true;
                        }
                    }
                }
            }
            let status = if vaulted {
                "vaulted"
            } else if soon {
                "vaulting-soon"
            } else {
                "available"
            };

            let mut candidate_names = vec![
                format!("{parent_name} set").to_lowercase(),
                format!("{parent_name} blueprint").to_lowercase(),
            ];
            for comp in parent.get("components").and_then(|c| c.as_array()).unwrap_or(&vec![]) {
                let cn = comp.get("name").and_then(|n| n.as_str()).unwrap_or("");
                if cn.is_empty() {
                    continue;
                }
                candidate_names.push(format!("{parent_name} {cn}").to_lowercase());
                candidate_names.push(format!("{parent_name} {cn} blueprint").to_lowercase());
            }
            let mut seen = std::collections::HashSet::new();
            for nm in &candidate_names {
                if let Some(slug) = catalog.get(nm) {
                    if seen.insert(slug.clone()) {
                        out.insert(slug.clone(), status.to_string());
                    }
                }
            }
        }
    }
    (out, complete)
}

/// Fetch Baro Ki'Teer's schedule from warframestat. Returns {} on failure
/// or missing fields — the Baro card hides.
pub fn fetch_baro(http: &dyn Http) -> HashMap<String, serde_json::Value> {
    let url = "https://api.warframestat.us/pc/voidTrader/";
    let data = match http.get_json(url) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("  warning: could not fetch {url}: {e}");
            return HashMap::new();
        }
    };
    if !data.is_object() {
        return HashMap::new();
    }
    let activation = data.get("activation").and_then(|a| a.as_str()).unwrap_or("");
    let expiry = data.get("expiry").and_then(|e| e.as_str()).unwrap_or("");
    let location = data.get("location").and_then(|l| l.as_str()).unwrap_or("");
    if activation.is_empty() || expiry.is_empty() || location.is_empty() {
        return HashMap::new();
    }
    let mut out = HashMap::new();
    out.insert("activation".into(), serde_json::Value::String(activation.into()));
    out.insert("expiry".into(), serde_json::Value::String(expiry.into()));
    out.insert("location".into(), serde_json::Value::String(location.into()));
    out
}

/// Fetch warframestat bulk item catalog (resolver data).
/// English is forced via custom header — the endpoint varies on
/// Accept-Language, and a localized catalog silently breaks the
/// name→WFM-slug join.
pub fn fetch_wfstat_slim() -> Result<Vec<serde_json::Value>, String> {
    let url = "https://api.warframestat.us/items/";
    let resp = reqwest::blocking::Client::builder()
        .user_agent(wfm_client::BROWSER_UA)
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("build client: {e}"))?
        .get(url)
        .header("Accept-Language", "en")
        .send()
        .map_err(|e| format!("{url}: {e}"))?;
    let status = resp.status();
    let body = resp.text().map_err(|e| format!("{url}: read: {e}"))?;
    if !status.is_success() {
        return Err(format!("{url}: HTTP {status}"));
    }
    let arr: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("{url}: JSON: {e}"))?;
    let items = arr.as_array().ok_or_else(|| format!("{url}: not an array"))?;
    let slim: Vec<serde_json::Value> = items
        .iter()
        .filter(|it| it.get("uniqueName").is_some() && it.get("name").is_some())
        .map(|it| {
            serde_json::json!([it["uniqueName"], {"name": it["name"], "category": it.get("category")}])
        })
        .collect();
    Ok(slim)
}

/// Fixture implementation of [`Http`] — serves pre-recorded responses from
/// a map. Missing keys are treated as an error, so the fixture can simulate
/// per-endpoint outages.
pub struct FixtureHttp {
    pub responses: HashMap<String, serde_json::Value>,
}

impl Http for FixtureHttp {
    fn get_json(&self, url: &str) -> Result<serde_json::Value, String> {
        self.responses
            .get(url)
            .cloned()
            .ok_or_else(|| format!("{url}: not in fixture set"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> FixtureHttp {
        let mut r = HashMap::new();
        r.insert(
            "https://api.warframe.market/v2/items".into(),
            serde_json::json!({"data": [
                {"slug": "primed_continuity", "i18n": {"en": {"name": "Primed Continuity"}}, "tags": ["mod"], "ducats": 0, "maxRank": 10, "subtypes": []},
                {"slug": "volt_prime_set", "i18n": {"en": {"name": "Volt Prime Set"}}, "tags": ["prime"], "ducats": null, "maxRank": null, "subtypes": []}
            ]}),
        );
        FixtureHttp { responses: r }
    }

    #[test]
    fn fetch_catalog_returns_name_slug_map_and_meta() {
        let (catalog, meta) = fetch_catalog_wfm(&fixture(), "https://api.warframe.market/v2/items").unwrap();
        assert_eq!(catalog.get("primed continuity"), Some(&"primed_continuity".into()));
        assert_eq!(catalog.get("volt prime set"), Some(&"volt_prime_set".into()));
        assert_eq!(meta.get("primed_continuity").unwrap().tags, vec!["mod"]);
        assert_eq!(meta.get("primed_continuity").unwrap().ducats, Some(0));
    }

    #[test]
    fn fetch_catalog_retries_3x_then_errors() {
        let empty = FixtureHttp { responses: HashMap::new() };
        let err = fetch_catalog_wfm(&empty, "https://api.warframe.market/v2/items");
        assert!(err.is_err());
    }
}
