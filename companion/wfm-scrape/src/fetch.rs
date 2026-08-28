//! Fetch stages — each upstream endpoint the converter needs.
//!
//! Every function accepting `Http` can be swapped for a fixture in tests.
//! The live implementation uses `wfm_client` primitives (browser UA,
//! headers, envelope unwrap, retry). The trait is intentionally narrow:
//! one GET→JSON method — that's all the Python converter does.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::clock;
use crate::render::CatalogItemMeta;

/// Days before `estimatedVaultDate` a part is tagged "vaulting-soon" instead
/// of "available". (Kept from the retired Python converter's `VAULT_SOON_DAYS`.)
const VAULT_SOON_DAYS: i64 = 60;

/// Narrow GET interface so every fetch stage is testable offline.
pub trait Http {
    fn get_json(&self, url: &str) -> Result<serde_json::Value, String>;
    /// Raw-text GET for non-JSON endpoints (DE's weekly riven stats file is a
    /// JS object literal). The default impl serves a JSON *string* fixture -
    /// how tests stand in for a raw body without a second trait method.
    fn get_text(&self, url: &str) -> Result<String, String> {
        self.get_json(url).and_then(|v| {
            v.as_str()
                .map(String::from)
                .ok_or_else(|| format!("{url}: expected a string fixture"))
        })
    }
    /// Raw-bytes GET for binary endpoints — DE's export index is an LZMA-alone
    /// stream, which neither of the above can carry. The default impl reads a
    /// `"base64:..."` string fixture, so a fixture can hold a real compressed
    /// body rather than a pretend one; a plain string fixture is passed through
    /// as UTF-8 bytes for the cases where that is enough.
    fn get_bytes(&self, url: &str) -> Result<Vec<u8>, String> {
        let text = self.get_text(url)?;
        match text.strip_prefix("base64:") {
            Some(b64) => {
                use base64::Engine as _;
                base64::engine::general_purpose::STANDARD
                    .decode(b64.trim())
                    .map_err(|e| format!("{url}: fixture base64 decode: {e}"))
            }
            None => Ok(text.into_bytes()),
        }
    }
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

    fn get_text(&self, url: &str) -> Result<String, String> {
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
        Ok(body)
    }

    /// Bytes, not text. The default impl round-trips through `String`, which
    /// would corrupt an LZMA stream — so the live path must not inherit it.
    fn get_bytes(&self, url: &str) -> Result<Vec<u8>, String> {
        let resp = self
            .client
            .get(url)
            .send()
            .map_err(|e| format!("{url}: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("{url}: HTTP {status}"));
        }
        resp.bytes()
            .map(|b| b.to_vec())
            .map_err(|e| format!("{url}: read body: {e}"))
    }
}

/// Fetch WFM catalog (`/v2/items`) — returns name→slug catalog AND
/// per-item metadata (tags, ducats, max_rank, subtypes).
///
/// Retries 3× with backoff, matching Python's `fetch_catalog`. On total
/// failure, returns `None` so the caller can fall back to the prior
/// snapshot's catalog + items.
pub type CatalogFetch = (
    HashMap<String, String>,
    HashMap<String, CatalogItemMeta>,
);

pub fn fetch_catalog_wfm(
    http: &dyn Http,
    url: &str,
) -> Result<CatalogFetch, String> {
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
    wfstat_items: Option<&serde_json::Value>,
) -> (HashMap<String, serde_json::Value>, HashMap<String, serde_json::Value>, bool) {
    // NOTE: no /sentinels/ here. That endpoint started failing ~2026-07-31 and
    // now hard-404s with `Data key 'sentinels' not found`; /companions/ and
    // /pets/ do not exist either. The same parents are in the bulk /items/
    // payload under `category: "Sentinels"`, and that payload is already
    // fetched for the resolver catalog — so sentinels are read from it rather
    // than costing a second 44 MB download.
    let endpoints = [
        ("https://api.warframestat.us/warframes/", "Warframes"),
        ("https://api.warframestat.us/weapons/", "Weapons"),
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
            absorb_parent(parent, fallback_cat, catalog, &mut path_to_info, &mut set_to_parts);
        }
    }

    // Sentinels, from the bulk catalog. A missing payload marks the surface
    // incomplete for the same reason a failed endpoint does: reconcile must
    // merge over the prior snapshot rather than replace it, or every sentinel
    // prime silently disappears from set_to_parts.
    match wfstat_items.and_then(|v| v.as_array()) {
        Some(all) => {
            for parent in all
                .iter()
                .filter(|it| it.get("category").and_then(|c| c.as_str()) == Some("Sentinels"))
            {
                absorb_parent(parent, "Sentinels", catalog, &mut path_to_info, &mut set_to_parts);
            }
        }
        None => {
            eprintln!("  warning: no bulk item payload — sentinel parents unavailable this run");
            complete = false;
        }
    }

    (path_to_info, set_to_parts, complete)
}

/// Fold one warframestat parent (a Warframe/weapon/sentinel with `components`)
/// into the component-path map and the set→parts map.
///
/// Extracted so the sentinel source, which no longer comes from its own
/// endpoint, runs byte-identical logic to the two that still do.
fn absorb_parent(
    parent: &serde_json::Value,
    fallback_cat: &str,
    catalog: &HashMap<String, String>,
    path_to_info: &mut HashMap<String, serde_json::Value>,
    set_to_parts: &mut HashMap<String, serde_json::Value>,
) {
    let parent_name = parent.get("name").and_then(|n| n.as_str()).unwrap_or("");
    if !parent_name.contains("Prime") {
        return;
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
            let quantity = comp
                .get("itemCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(1)
                .max(1);
            this_set_parts.push(serde_json::json!({
                "slug": slug,
                "component_name": cn,
                "quantity": quantity,
            }));
        }
    }
    if let (Some(ss), false) = (set_slug, this_set_parts.is_empty()) {
        set_to_parts.insert(
            ss.clone(),
            serde_json::json!({"name": parent_name, "parts": this_set_parts}),
        );
    }
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
    let vault_soon_cutoff = now + chrono::Duration::days(VAULT_SOON_DAYS);
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

    // What he is actually selling — the part that decides whether any of this
    // is actionable. It exists ONLY while he is at a relay: between visits the
    // endpoint returns `inventory: []`, and warframestat publishes no schedule
    // and no history (both verified empty against the live API). So a visit's
    // stock can only be recorded during the ~48h he is present; miss the window
    // and the next chance is his next visit, two weeks later. The caller carries
    // the last captured list forward for exactly that reason.
    //
    // Absent (rather than empty) when he is away, so `carry_baro_inventory`
    // can tell "no data this fetch" from "he is here selling nothing", and so
    // reconcile's emptiness check keeps working on the surface as a whole.
    let inventory: Vec<serde_json::Value> = data
        .get("inventory")
        .and_then(|i| i.as_array())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|e| {
                    let name = e.get("item").and_then(|n| n.as_str())?;
                    let mut row = serde_json::Map::new();
                    row.insert("item".into(), serde_json::Value::String(name.into()));
                    // Ducat and credit cost are the whole point of the
                    // sell-vs-feed-him comparison; keep them when present.
                    for key in ["ducats", "credits"] {
                        if let Some(v) = e.get(key).and_then(|v| v.as_i64()) {
                            row.insert(key.into(), serde_json::Value::from(v));
                        }
                    }
                    Some(serde_json::Value::Object(row))
                })
                .collect()
        })
        .unwrap_or_default();
    if !inventory.is_empty() {
        out.insert("inventory".into(), serde_json::Value::Array(inventory));
        // Which visit this stock belongs to, so a consumer can tell the live
        // list from a leftover one without guessing from timestamps.
        out.insert(
            "inventory_for".into(),
            serde_json::Value::String(activation.into()),
        );
    }
    out
}

/// Carry a previously captured Baro inventory forward when the current fetch
/// has none.
///
/// `reconcile` cannot do this: it only substitutes the prior value when the
/// WHOLE surface is empty, and Baro's is never empty — activation, expiry and
/// location keep arriving between visits. Without this, the one list that is
/// only obtainable during a 48h window would be dropped on the very next
/// scrape after he leaves.
///
/// `inventory_for` travels with the list, so a consumer can compare it against
/// the current `activation` and tell "what he is selling right now" from "what
/// he sold last time".
pub fn carry_baro_inventory(
    fresh: &mut HashMap<String, serde_json::Value>,
    prior: Option<&HashMap<String, serde_json::Value>>,
) {
    if fresh.is_empty() || fresh.contains_key("inventory") {
        return;
    }
    let Some(prior) = prior else { return };
    let (Some(inv), Some(for_)) = (prior.get("inventory"), prior.get("inventory_for")) else {
        return;
    };
    fresh.insert("inventory".into(), inv.clone());
    fresh.insert("inventory_for".into(), for_.clone());
}

pub const WFM_RIVEN_WEAPONS_URL: &str = "https://api.warframe.market/v2/riven/weapons";

/// How long a disposition change stays in the snapshot's rolling change log.
/// DE ships disposition passes with each Prime Access (~quarterly); 90 days
/// keeps the last pass visible until the next one lands.
pub const RIVEN_CHANGE_RETENTION_DAYS: i64 = 90;

/// Fetch WFM's riven-weapon manifest and reduce it to
/// `weapons: {slug: {name, disposition, group, riven_type, req_mr}}`, then
/// diff dispositions against the prior snapshot's `rivens.weapons` into a
/// rolling `changes: [{slug, name, from, to, seen_at}]` (newest first,
/// bounded by [`RIVEN_CHANGE_RETENTION_DAYS`]).
///
/// Why: DE has stopped decreasing dispositions and only raises them (2024
/// policy, restated in the 2025-26 workshops), so a change is a one-sided
/// price event for anyone holding that weapon's rivens — and repricing on
/// WFM lands within a day of the patch notes. The scrape runs every 2 h, so
/// the log catches it the same day. `seen_at` is when WE first saw the new
/// value, not DE's patch time.
///
/// Returns `{}` on fetch failure so reconcile falls back to the prior surface.
/// The riven weapons manifest: slug → {name, disposition, group, riven_type,
/// req_mr, game_ref}. Dispositions here are warframe.market's MIRROR of DE's;
/// the caller overrides them from `ExportWeapons` and only then computes the
/// change log — see `riven_change_log` for why the order matters.
pub fn fetch_rivens(http: &dyn Http) -> HashMap<String, serde_json::Value> {
    let data = match http.get_json(WFM_RIVEN_WEAPONS_URL) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("  warning: could not fetch {WFM_RIVEN_WEAPONS_URL}: {e}");
            return HashMap::new();
        }
    };
    let arr = data
        .get("data")
        .and_then(|d| d.as_array())
        .or_else(|| data.as_array());
    let Some(arr) = arr else {
        eprintln!("  warning: {WFM_RIVEN_WEAPONS_URL}: unexpected shape");
        return HashMap::new();
    };

    let mut weapons = serde_json::Map::new();
    for w in arr {
        let Some(slug) = w.get("slug").and_then(|v| v.as_str()) else { continue };
        let Some(dispo) = w.get("disposition").and_then(|v| v.as_f64()) else { continue };
        let name = w
            .get("i18n").and_then(|i| i.get("en")).and_then(|e| e.get("name")).and_then(|n| n.as_str())
            .unwrap_or(slug);
        let mut row = serde_json::Map::new();
        row.insert("name".into(), serde_json::Value::String(name.into()));
        row.insert("disposition".into(), serde_json::json!(dispo));
        for (src, dst) in [("group", "group"), ("rivenType", "riven_type")] {
            if let Some(v) = w.get(src).and_then(|v| v.as_str()) {
                row.insert(dst.into(), serde_json::Value::String(v.into()));
            }
        }
        if let Some(mr) = w.get("reqMasteryRank").and_then(|v| v.as_i64()) {
            row.insert("req_mr".into(), serde_json::Value::from(mr));
        }
        // The weapon's in-game path — the SPA maps a scanned riven's `compat`
        // fingerprint field to this slug through it.
        if let Some(gr) = w.get("gameRef").and_then(|v| v.as_str()) {
            row.insert("game_ref".into(), serde_json::Value::String(gr.into()));
        }
        weapons.insert(slug.to_string(), serde_json::Value::Object(row));
    }
    if weapons.is_empty() {
        return HashMap::new();
    }

    // NOTE: the disposition change log is NOT computed here. It has to diff
    // the values that actually get PUBLISHED, and the DE override is applied
    // by the caller after this returns — see `riven_change_log`.

    // The stat-name/unit manifest (`/v2/riven/attributes`). A scanned
    // riven's fingerprint names stats by DE tag (`WeaponCritDamageMod`); this
    // maps that tag (game_ref) to a display name + whether it is a percent
    // stat, so the Rivens view can render the raw fingerprint value as
    // "+95.3% crit damage" instead of a bare integer.
    let mut attributes: Vec<serde_json::Value> = Vec::new();
    if let Ok(data) = http.get_json(WFM_RIVEN_ATTRIBUTES_URL) {
        let arr = data.get("data").and_then(|d| d.as_array()).or_else(|| data.as_array());
        if let Some(arr) = arr {
            for a in arr {
                let Some(gr) = a.get("gameRef").and_then(|v| v.as_str()) else { continue };
                let mut row = serde_json::Map::new();
                row.insert("game_ref".into(), serde_json::Value::String(gr.into()));
                if let Some(slug) = a.get("slug").and_then(|v| v.as_str()) {
                    row.insert("slug".into(), serde_json::Value::String(slug.into()));
                }
                if let Some(name) = a.get("i18n").and_then(|i| i.get("en")).and_then(|e| e.get("name")).and_then(|n| n.as_str()) {
                    row.insert("name".into(), serde_json::Value::String(name.into()));
                }
                if let Some(unit) = a.get("unit").and_then(|v| v.as_str()) {
                    row.insert("unit".into(), serde_json::Value::String(unit.into()));
                }
                attributes.push(serde_json::Value::Object(row));
            }
        }
    } else {
        eprintln!("  warning: could not fetch {WFM_RIVEN_ATTRIBUTES_URL}");
    }

    let mut out = HashMap::new();
    out.insert("weapons".into(), serde_json::Value::Object(weapons));
    if !attributes.is_empty() {
        out.insert("attributes".into(), serde_json::Value::Array(attributes));
    }
    out
}

/// The rolling disposition change log, diffed against the FINAL published
/// weapons map.
///
/// This used to live inside `fetch_rivens`, which made it diff
/// warframe.market's fresh mirror against the prior snapshot's STORED values —
/// and those are DE's, because the caller overrides them afterwards. The
/// result was wrong in both directions at once:
///
/// - **Phantom changes.** WFM's mirror lags DE. Every cycle where it still
///   disagreed logged a change from DE's stored value to WFM's stale one, and
///   then the override rewrote the value back to DE's. Nothing had changed;
///   the log said something had.
/// - **Missed changes.** A genuine DE disposition move is applied by the
///   override *after* the diff ran, so it never appeared in the log at all.
///
/// Diffing the published values against the previously published values is the
/// only comparison that means anything: the log describes what the snapshot
/// says, not what one upstream happened to report mid-pipeline.
pub fn riven_change_log(
    weapons: &serde_json::Map<String, serde_json::Value>,
    prior: Option<&HashMap<String, serde_json::Value>>,
    now: DateTime<Utc>,
) -> Vec<serde_json::Value> {
    let prior_weapons = prior
        .and_then(|p| p.get("weapons"))
        .and_then(|w| w.as_object());
    let mut changes: Vec<serde_json::Value> = Vec::new();
    if let Some(pw) = prior_weapons {
        for (slug, row) in weapons {
            let Some(to) = row.get("disposition").and_then(|d| d.as_f64()) else { continue };
            let Some(from) = pw.get(slug).and_then(|r| r.get("disposition")).and_then(|d| d.as_f64())
            else {
                continue;
            };
            // Dispositions are quoted to 2 dp; anything under half a hundredth
            // is float noise, not a change.
            if (to - from).abs() < 0.005 {
                continue;
            }
            changes.push(serde_json::json!({
                "slug": slug,
                "name": row.get("name").cloned().unwrap_or(serde_json::Value::String(slug.clone())),
                "from": from,
                "to": to,
                "seen_at": clock::iso_z(now),
            }));
        }
    }
    // Carry the prior log forward, dropping entries past retention and any
    // entry for a slug that just changed again (the new row supersedes it).
    let cutoff = now - chrono::Duration::days(RIVEN_CHANGE_RETENTION_DAYS);
    let changed_now: std::collections::HashSet<String> = changes
        .iter()
        .filter_map(|c| c.get("slug").and_then(|s| s.as_str()).map(String::from))
        .collect();
    if let Some(old) = prior.and_then(|p| p.get("changes")).and_then(|c| c.as_array()) {
        for c in old {
            let slug = c.get("slug").and_then(|s| s.as_str()).unwrap_or("");
            if changed_now.contains(slug) {
                continue;
            }
            let keep = c
                .get("seen_at")
                .and_then(|s| s.as_str())
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|t| t.with_timezone(&Utc) >= cutoff)
                .unwrap_or(false);
            if keep {
                changes.push(c.clone());
            }
        }
    }
    changes.sort_by(|a, b| {
        let sa = a.get("seen_at").and_then(|s| s.as_str()).unwrap_or("");
        let sb = b.get("seen_at").and_then(|s| s.as_str()).unwrap_or("");
        sb.cmp(sa).then_with(|| {
            a.get("slug").and_then(|s| s.as_str()).unwrap_or("")
                .cmp(b.get("slug").and_then(|s| s.as_str()).unwrap_or(""))
        })
    });
    changes
}

pub const WFM_RIVEN_ATTRIBUTES_URL: &str = "https://api.warframe.market/v2/riven/attributes";

/// DE's weekly riven price statistics, published every Monday as a JS object
/// literal (NOT JSON: unquoted keys, single-quoted strings), keyed by weapon
/// display name x `rerolled`. ~150 KB; the only stats that actually sample
/// riven auctions — WFM's `/statistics` has no riven rows at all.
pub use crate::de::DE_WEEKLY_RIVENS_URL;

/// The same file per platform. Console riven markets diverge sharply from PC's
/// — different populations, different metas, and far smaller samples — and DE
/// publishes all four while nothing in the ecosystem compares them.
///
/// PC stays the primary surface (`unrolled`/`rolled` at the top level); the
/// consoles ride along under `platforms` so a consumer that only knows about
/// PC keeps working untouched.
pub use crate::de::DE_WEEKLY_RIVEN_PLATFORMS;

/// One row of `DE_WEEKLY_RIVENS_URL`: one weapon x reroll-state's price band.
/// Generic rows (`compatibility: null` — "Rifle Riven Mod") carry no weapon
/// and are dropped by `fetch_riven_stats`.
#[derive(Debug, Clone, PartialEq)]
pub struct WeeklyRivenRow {
    pub item_type: String,
    pub compatibility: Option<String>,
    pub rerolled: bool,
    pub avg: f64,
    pub stddev: f64,
    pub min: f64,
    pub max: f64,
    pub pop: u64,
    pub median: f64,
}

#[derive(Debug, Clone, PartialEq)]
enum JsTok {
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Str(String),
    /// unquoted key or the bare literals true/false/null
    Ident(String),
    Num(f64),
}

fn tokenize_js_literal(text: &str) -> Result<Vec<JsTok>, String> {
    let b = text.as_bytes();
    let mut i = 0usize;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i] as char;
        match c {
            ' ' | '\t' | '\r' | '\n' => i += 1,
            '{' => { out.push(JsTok::LBrace); i += 1; }
            '}' => { out.push(JsTok::RBrace); i += 1; }
            '[' => { out.push(JsTok::LBracket); i += 1; }
            ']' => { out.push(JsTok::RBracket); i += 1; }
            ',' => { out.push(JsTok::Comma); i += 1; }
            ':' => { out.push(JsTok::Colon); i += 1; }
            '\'' => {
                let mut s = String::new();
                i += 1;
                loop {
                    if i >= b.len() {
                        return Err(format!("unterminated string at byte {i}"));
                    }
                    let ch = b[i] as char;
                    if ch == '\\' {
                        i += 1;
                        if i >= b.len() {
                            return Err(format!("unterminated escape at byte {i}"));
                        }
                        s.push(b[i] as char);
                        i += 1;
                    } else if ch == '\'' {
                        i += 1;
                        break;
                    } else {
                        s.push(ch);
                        i += 1;
                    }
                }
                out.push(JsTok::Str(s));
            }
            '0'..='9' | '-' => {
                let start = i;
                while i < b.len()
                    && (b[i].is_ascii_digit()
                        || matches!(b[i], b'.' | b'-' | b'+' | b'e' | b'E'))
                {
                    i += 1;
                }
                let num = &text[start..i];
                let v: f64 = num
                    .parse()
                    .map_err(|_| format!("bad number in {DE_WEEKLY_RIVENS_URL}: {num:?}"))?;
                out.push(JsTok::Num(v));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                    i += 1;
                }
                out.push(JsTok::Ident(text[start..i].to_string()));
            }
            other => return Err(format!("unexpected char {other:?} at byte {i}")),
        }
    }
    Ok(out)
}

/// Recursive-descent parse of the JS-literal subset DE actually emits: objects,
/// arrays, strings, numbers, `null`, booleans. Anything else is an error — a
/// shape change upstream should fail the surface loudly, not silently zero it.
fn js_literal_to_json(tokens: &[JsTok], pos: &mut usize) -> Result<serde_json::Value, String> {
    let Some(tok) = tokens.get(*pos) else {
        return Err(format!("unexpected end of {DE_WEEKLY_RIVENS_URL}"));
    };
    *pos += 1;
    match tok {
        JsTok::LBrace => {
            let mut map = serde_json::Map::new();
            loop {
                if matches!(tokens.get(*pos), Some(JsTok::RBrace)) {
                    *pos += 1;
                    break;
                }
                let key = match tokens.get(*pos) {
                    Some(JsTok::Str(s)) | Some(JsTok::Ident(s)) => s.clone(),
                    _ => return Err(format!("expected object key at token {}", *pos)),
                };
                *pos += 1;
                if !matches!(tokens.get(*pos), Some(JsTok::Colon)) {
                    return Err(format!("expected ':' after {key:?}"));
                }
                *pos += 1;
                let value = js_literal_to_json(tokens, pos)?;
                map.insert(key, value);
                if matches!(tokens.get(*pos), Some(JsTok::Comma)) {
                    *pos += 1;
                }
            }
            Ok(serde_json::Value::Object(map))
        }
        JsTok::LBracket => {
            let mut arr = Vec::new();
            loop {
                if matches!(tokens.get(*pos), Some(JsTok::RBracket)) {
                    *pos += 1;
                    break;
                }
                arr.push(js_literal_to_json(tokens, pos)?);
                match tokens.get(*pos) {
                    Some(JsTok::Comma) => *pos += 1,
                    Some(JsTok::RBracket) => {
                        *pos += 1;
                        break;
                    }
                    _ => return Err(format!("expected ',' or ']' at token {}", *pos)),
                }
            }
            Ok(serde_json::Value::Array(arr))
        }
        JsTok::Str(s) => Ok(serde_json::Value::String(s.clone())),
        JsTok::Num(n) => Ok(serde_json::Value::from(*n)),
        JsTok::Ident(id) => match id.as_str() {
            "true" => Ok(serde_json::Value::Bool(true)),
            "false" => Ok(serde_json::Value::Bool(false)),
            "null" => Ok(serde_json::Value::Null),
            other => Err(format!("unexpected identifier {other:?}")),
        },
        _ => Err(format!("unexpected token at position {}", *pos)),
    }
}

/// Parse the full `DE_WEEKLY_RIVENS_URL` body into rows. Rows missing the
/// price fields are dropped (a shape change upstream surfaces as a count
/// drop, which the build logs).
fn parse_weekly_rivens_counted(text: &str) -> Result<(Vec<WeeklyRivenRow>, usize), String> {
    let tokens = tokenize_js_literal(text)?;
    let mut pos = 0usize;
    let value = js_literal_to_json(&tokens, &mut pos)?;
    let arr = value.as_array().ok_or_else(|| format!("{DE_WEEKLY_RIVENS_URL}: not an array"))?;
    let raw_count = arr.len();
    let mut rows = Vec::new();
    for v in arr {
        let Some(obj) = v.as_object() else { continue };
        let f = |k: &str| obj.get(k).and_then(|v| v.as_f64());
        let Some(avg) = f("avg") else { continue };
        let Some(median) = f("median") else { continue };
        rows.push(WeeklyRivenRow {
            item_type: obj.get("itemType").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            compatibility: obj.get("compatibility").and_then(|v| v.as_str()).map(String::from),
            rerolled: obj.get("rerolled").and_then(|v| v.as_bool()).unwrap_or(false),
            avg,
            stddev: f("stddev").unwrap_or(0.0),
            min: f("min").unwrap_or(0.0),
            max: f("max").unwrap_or(0.0),
            // as_u64() is None for the f64 the JS literal produces (10 → 10.0)
            pop: obj.get("pop").and_then(|v| v.as_f64()).map(|f| f as u64).unwrap_or(0),
            median,
        });
    }
    Ok((rows, raw_count))
}

#[cfg(test)]
fn parse_weekly_rivens(text: &str) -> Result<Vec<WeeklyRivenRow>, String> {
    parse_weekly_rivens_counted(text).map(|(rows, _)| rows)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RivenChildOutcome {
    Unavailable,
    Invalid,
    AuthoritativeEmpty,
    Usable,
}

/// Build the `riven_stats` surface: DE's weekly price bands per weapon x
/// reroll-state, keyed by WFM slug (`unrolled` / `rolled` tiers, each
/// `{avg, median, min, max, stddev, pop}`). `weapons_by_name` maps
/// display-name-lower -> slug from the riven-weapons manifest (the same source
/// the disposition surface uses), so DE's names join without a second
/// request. Returns the unmatched-name count for the build log. `{}` + 0 on
/// fetch/parse failure so reconcile falls back to the prior surface.
pub fn fetch_riven_stats(
    http: &dyn Http,
    weapons_by_name: &HashMap<String, String>,
) -> (HashMap<String, serde_json::Value>, usize, HashMap<String, RivenChildOutcome>) {
    let mut outcomes = HashMap::new();
    let text = match http.get_text(DE_WEEKLY_RIVENS_URL) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("  warning: could not fetch {DE_WEEKLY_RIVENS_URL}: {e}");
            outcomes.insert("pc".into(), RivenChildOutcome::Unavailable);
            return (HashMap::new(), 0, outcomes);
        }
    };
    let (rows, raw_count) = match parse_weekly_rivens_counted(&text) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  warning: {DE_WEEKLY_RIVENS_URL}: {e}");
            outcomes.insert("pc".into(), RivenChildOutcome::Invalid);
            return (HashMap::new(), 0, outcomes);
        }
    };
    if raw_count == 0 {
        outcomes.insert("pc".into(), RivenChildOutcome::AuthoritativeEmpty);
        return (HashMap::new(), 0, outcomes);
    }
    if rows.is_empty() {
        outcomes.insert("pc".into(), RivenChildOutcome::Invalid);
        return (HashMap::new(), 0, outcomes);
    }
    if rows.len() < raw_count {
        outcomes.insert("pc".into(), RivenChildOutcome::Invalid);
        return (HashMap::new(), 0, outcomes);
    }
    let mut stats: HashMap<String, serde_json::Value> = HashMap::new();
    let mut unmatched = 0usize;
    for row in rows {
        let Some(weapon) = row.compatibility else { continue };
        let Some(slug) = weapons_by_name.get(&weapon.to_lowercase()) else {
            unmatched += 1;
            continue;
        };
        let tier_key = if row.rerolled { "rolled" } else { "unrolled" };
        let tier = serde_json::json!({
            "avg": row.avg, "median": row.median, "min": row.min, "max": row.max,
            "stddev": row.stddev, "pop": row.pop,
        });
        let entry = stats
            .entry(slug.clone())
            .or_insert_with(|| serde_json::json!({ "name": weapon }));
        entry[tier_key] = tier;
    }

    // Consoles, folded in under `platforms`. A platform that fails to fetch or
    // parse is skipped with a warning rather than failing the surface — PC is
    // what the app actually prices against, and losing it to a Switch outage
    // would be absurd.
    for (platform, url) in DE_WEEKLY_RIVEN_PLATFORMS {
        let Ok(text) = http.get_text(url) else {
            outcomes.insert((*platform).to_string(), RivenChildOutcome::Unavailable);
            eprintln!("  warning: could not fetch {url}");
            continue;
        };
        let (rows, raw_count) = match parse_weekly_rivens_counted(&text) {
            Ok(r) => r,
            Err(e) => {
                outcomes.insert((*platform).to_string(), RivenChildOutcome::Invalid);
                eprintln!("  warning: {url}: {e}");
                continue;
            }
        };
        if raw_count == 0 {
            outcomes.insert((*platform).to_string(), RivenChildOutcome::AuthoritativeEmpty);
            continue;
        }
        if rows.is_empty() {
            outcomes.insert((*platform).to_string(), RivenChildOutcome::Invalid);
            continue;
        }
        if rows.len() < raw_count {
            outcomes.insert((*platform).to_string(), RivenChildOutcome::Invalid);
            continue;
        }
        let mut joined = 0usize;
        for row in rows {
            let Some(weapon) = row.compatibility else { continue };
            let Some(slug) = weapons_by_name.get(&weapon.to_lowercase()) else { continue };
            // Only alongside a weapon PC already knows. A console-only row has
            // no PC baseline to compare against, which is the only reason to
            // carry it.
            let Some(entry) = stats.get_mut(slug) else { continue };
            let tier_key = if row.rerolled { "rolled" } else { "unrolled" };
            let tier = serde_json::json!({
                "avg": row.avg, "median": row.median, "min": row.min, "max": row.max,
                "stddev": row.stddev, "pop": row.pop,
            });
            if !entry.get("platforms").map(|p| p.is_object()).unwrap_or(false) {
                entry["platforms"] = serde_json::json!({});
            }
            let plat = &mut entry["platforms"][*platform];
            if !plat.is_object() {
                *plat = serde_json::json!({});
            }
            plat[tier_key] = tier;
            joined += 1;
        }
        outcomes.insert(
            (*platform).to_string(),
            if joined == 0 { RivenChildOutcome::Invalid } else { RivenChildOutcome::Usable },
        );
    }

    if stats.is_empty() {
        outcomes.insert("pc".into(), RivenChildOutcome::Invalid);
    } else {
        outcomes.insert("pc".into(), RivenChildOutcome::Usable);
    }
    (stats, unmatched, outcomes)
}

/// Retain only failed console children while replacing the successful PC
/// baseline. Iterating fresh PC slugs prevents a stale console-only weapon from
/// being resurrected after it disappears from the primary feed.
pub fn carry_failed_riven_platforms(
    fresh: &mut HashMap<String, serde_json::Value>,
    prior: Option<&HashMap<String, serde_json::Value>>,
    outcomes: &HashMap<String, RivenChildOutcome>,
) {
    let Some(prior) = prior else { return };
    for (platform, _) in DE_WEEKLY_RIVEN_PLATFORMS {
        if matches!(outcomes.get(*platform), Some(RivenChildOutcome::Usable | RivenChildOutcome::AuthoritativeEmpty)) {
            continue;
        }
        for (slug, row) in fresh.iter_mut() {
            let Some(old_platform) = prior
                .get(slug)
                .and_then(|old| old.get("platforms"))
                .and_then(|p| p.get(*platform))
                .cloned()
            else { continue };
            let Some(obj) = row.as_object_mut() else { continue };
            let platforms = obj
                .entry("platforms")
                .or_insert_with(|| serde_json::json!({}));
            if let Some(map) = platforms.as_object_mut() {
                map.insert((*platform).to_string(), old_platform);
            }
        }
    }
}

pub const WFSTAT_VAULT_TRADER_URL: &str = "https://api.warframestat.us/pc/vaultTrader/";

/// The price-shock calendar the hold/sell advisor reasons over, as one
/// `calendar` surface in market.json:
///
/// - `primes: {set_slug: {name, released, vaulted, vault_date,
///   est_vault_date}}` — per prime set, from warframestat's item catalog
///   (WFCD warframe-items carries `releaseDate` / `vaultDate` /
///   `estimatedVaultDate` / `vaulted`; the same payload the build already
///   downloads for the resolver, so no extra request).
/// - `resurgence: [{from, to, frames: [set_slug]}]` — every Prime Resurgence
///   rotation warframestat knows (its `vaultTrader.schedule` is the full
///   history since 2022, one entry per rotation with the pack name and its
///   expiry; a rotation runs from the previous expiry to its own), plus
///   `resurgence_current` for the one running now.
///
/// Why: a set's price has three predictable shocks — Prime Access release
/// (day-1 flood), vaulting (supply cap, months-long ramp), and Resurgence
/// (Varzia sells the relics again for four weeks → temporary flood). Dates
/// make those computable per owned set instead of folklore.
///
/// Set names → WFM slugs via the catalog (`name_lower → slug`; a set is
/// "<name> set"). Unmatched names are dropped and counted in the warning.
pub fn fetch_calendar(
    http: &dyn Http,
    wfstat_raw: Option<&serde_json::Value>,
    catalog: &HashMap<String, String>,
) -> HashMap<String, serde_json::Value> {
    let mut out = HashMap::new();

    // ---- primes: release / vault dates ----
    let mut primes = serde_json::Map::new();
    let mut unmatched = 0usize;
    if let Some(items) = wfstat_raw.and_then(|v| v.as_array()) {
        for it in items {
            let Some(name) = it.get("name").and_then(|n| n.as_str()) else { continue };
            if !name.ends_with(" Prime") {
                continue;
            }
            let Some(released) = it.get("releaseDate").and_then(|d| d.as_str()) else { continue };
            let Some(slug) = catalog.get(&format!("{} set", name.to_lowercase())) else {
                unmatched += 1;
                continue;
            };
            let mut row = serde_json::Map::new();
            row.insert("name".into(), serde_json::Value::String(name.into()));
            row.insert("released".into(), serde_json::Value::String(released.into()));
            row.insert(
                "vaulted".into(),
                serde_json::Value::Bool(it.get("vaulted").and_then(|v| v.as_bool()).unwrap_or(false)),
            );
            for (src, dst) in [("vaultDate", "vault_date"), ("estimatedVaultDate", "est_vault_date")] {
                if let Some(d) = it.get(src).and_then(|d| d.as_str()) {
                    row.insert(dst.into(), serde_json::Value::String(d.into()));
                }
            }
            primes.insert(slug.clone(), serde_json::Value::Object(row));
        }
    }
    if unmatched > 0 {
        eprintln!("  calendar: {unmatched} primes with release dates have no WFM set in the catalog");
    }
    if !primes.is_empty() {
        out.insert("primes".into(), serde_json::Value::Object(primes));
    }

    // ---- resurgence rotations ----
    match http.get_json(WFSTAT_VAULT_TRADER_URL) {
        Ok(vt) => {
            let (rotations, current) = resurgence_rotations(&vt, catalog);
            if !rotations.is_empty() {
                out.insert("resurgence".into(), serde_json::Value::Array(rotations));
            }
            if let Some(c) = current {
                out.insert("resurgence_current".into(), c);
            }
        }
        Err(e) => eprintln!("  warning: could not fetch {WFSTAT_VAULT_TRADER_URL}: {e}"),
    }
    out
}

/// "M P V Revenant Baruuk Prime Dual Pack" → ["Revenant Prime", "Baruuk Prime"];
/// "Nezha & Octavia Prime Dual Pack" → ["Nezha Prime", "Octavia Prime"];
/// "M P V Oberon Prime Single Pack" → ["Oberon Prime"]. "Last Chance Item C"
/// and anything else without "Prime" → [].
pub fn frames_in_pack_name(name: &str) -> Vec<String> {
    let mut s = name.trim();
    for prefix in ["M P V ", "MPV "] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest;
        }
    }
    for suffix in [" Dual Pack", " Single Pack", " Pack"] {
        if let Some(rest) = s.strip_suffix(suffix) {
            s = rest;
        }
    }
    let Some(base) = s.strip_suffix(" Prime") else { return vec![] };
    base.replace(" & ", " ")
        .split_whitespace()
        .filter(|w| !w.is_empty())
        .map(|w| format!("{w} Prime"))
        .collect()
}

/// Rotations from a `vaultTrader` payload: consecutive `schedule` entries
/// bound each window (`from` = previous expiry or `initialStart`, `to` =
/// own expiry). Returns (all rotations with ≥1 resolvable frame, the current
/// one from activation/expiry).
pub fn resurgence_rotations(
    vt: &serde_json::Value,
    catalog: &HashMap<String, String>,
) -> (Vec<serde_json::Value>, Option<serde_json::Value>) {
    let slugs_for = |pack: &str| -> Vec<serde_json::Value> {
        frames_in_pack_name(pack)
            .iter()
            .filter_map(|f| catalog.get(&format!("{} set", f.to_lowercase())))
            .map(|s| serde_json::Value::String(s.clone()))
            .collect()
    };
    let mut rotations = Vec::new();
    let mut prev_expiry: Option<String> = vt.get("initialStart").and_then(|s| s.as_str()).map(String::from);
    if let Some(sched) = vt.get("schedule").and_then(|s| s.as_array()) {
        for entry in sched {
            let Some(expiry) = entry.get("expiry").and_then(|e| e.as_str()) else { continue };
            let pack = entry.get("item").and_then(|i| i.as_str()).unwrap_or("");
            let frames = slugs_for(pack);
            if !frames.is_empty() {
                if let Some(from) = &prev_expiry {
                    rotations.push(serde_json::json!({
                        "from": from,
                        "to": expiry,
                        "pack": pack,
                        "frames": frames,
                    }));
                }
            }
            prev_expiry = Some(expiry.to_string());
        }
    }
    let current = match (
        vt.get("activation").and_then(|a| a.as_str()),
        vt.get("expiry").and_then(|e| e.as_str()),
    ) {
        (Some(from), Some(to)) => {
            // The current pack is whichever inventory entry names frames.
            let mut frames: Vec<serde_json::Value> = Vec::new();
            if let Some(inv) = vt.get("inventory").and_then(|i| i.as_array()) {
                for e in inv {
                    if let Some(item) = e.get("item").and_then(|i| i.as_str()) {
                        for f in slugs_for(item) {
                            if !frames.contains(&f) {
                                frames.push(f);
                            }
                        }
                    }
                }
            }
            Some(serde_json::json!({ "from": from, "to": to, "frames": frames }))
        }
        _ => None,
    };
    (rotations, current)
}

pub const WFSTAT_ITEMS_URL: &str = "https://api.warframestat.us/items/";

/// Reduce the warframestat bulk item list to the resolver's slim
/// `[uniqueName, {name, category}]` pairs.
///
/// Shared by the live fetch and the fixture path. It was written out twice,
/// once each, and the filter and the shape have to agree exactly — the browser
/// resolver joins on these pairs, so a divergence surfaces as owned items that
/// silently fail to resolve.
pub fn slim_wfstat_items(arr: &serde_json::Value, url: &str) -> Result<Vec<serde_json::Value>, String> {
    let items = arr.as_array().ok_or_else(|| format!("{url}: not an array"))?;
    Ok(items
        .iter()
        .filter(|it| it.get("uniqueName").is_some() && it.get("name").is_some())
        .map(|it| {
            serde_json::json!([it["uniqueName"], {"name": it["name"], "category": it.get("category")}])
        })
        .collect())
}

/// Fetch the warframestat bulk item catalog (resolver data).
///
/// Builds its own client rather than going through [`Http`], because English
/// must be forced per-call: the endpoint varies on `Accept-Language` and a
/// localized catalog silently breaks the name→WFM-slug join. The trait's
/// `get_json(url)` has nowhere to put a header, and pushing `Accept-Language`
/// onto the shared client would send it on every other endpoint too. Longer
/// timeout for the same reason: the body is multi-MB.
pub fn fetch_wfstat_slim() -> Result<Vec<serde_json::Value>, String> {
    slim_wfstat_items(&fetch_wfstat_raw()?, WFSTAT_ITEMS_URL)
}

/// The bulk warframestat item payload, unreduced.
///
/// Split out from `fetch_wfstat_slim` because this ~44 MB response now feeds
/// two consumers — the resolver catalog AND the sentinel parents, whose own
/// endpoint 404s — and downloading it twice would add minutes to a scrape that
/// already runs close to its systemd timeout.
pub fn fetch_wfstat_raw() -> Result<serde_json::Value, String> {
    let url = WFSTAT_ITEMS_URL;
    let resp = reqwest::blocking::Client::builder()
        .user_agent(wfm_client::user_agent("wfm-scrape", env!("CARGO_PKG_VERSION")))
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
    serde_json::from_str(&body).map_err(|e| format!("{url}: JSON: {e}"))
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

    /// A raw-body fixture may be written either way: a JSON string (for
    /// genuinely non-JSON bodies like DE's weekly riven JS literal) or a plain
    /// JSON object (for DE's export manifests, which ARE JSON and would be
    /// unreadable in the fixture file if escaped into a string).
    fn get_text(&self, url: &str) -> Result<String, String> {
        let v = self.get_json(url)?;
        Ok(match v.as_str() {
            Some(s) => s.to_string(),
            None => v.to_string(),
        })
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

    const BARO_URL: &str = "https://api.warframestat.us/pc/voidTrader/";

    fn baro_http(body: serde_json::Value) -> FixtureHttp {
        let mut r = HashMap::new();
        r.insert(BARO_URL.into(), body);
        FixtureHttp { responses: r }
    }

    #[test]
    fn baro_inventory_is_captured_while_he_is_present() {
        let got = fetch_baro(&baro_http(serde_json::json!({
            "activation": "2026-08-21T13:00:00.000Z",
            "expiry": "2026-08-23T13:00:00.000Z",
            "location": "Orcus Relay (Pluto)",
            "inventory": [
                {"item": "Primed Fury", "ducats": 350, "credits": 200000},
                {"item": "Prisma Grakata", "ducats": 500, "credits": 300000}
            ]
        })));
        let inv = got.get("inventory").unwrap().as_array().unwrap();
        assert_eq!(inv.len(), 2);
        assert_eq!(inv[0].get("item").unwrap(), "Primed Fury");
        assert_eq!(inv[0].get("ducats").unwrap(), 350);
        assert_eq!(inv[0].get("credits").unwrap(), 200000);
        // Tagged with the visit it belongs to.
        assert_eq!(
            got.get("inventory_for").unwrap(),
            "2026-08-21T13:00:00.000Z"
        );
    }

    #[test]
    fn baro_inventory_is_absent_not_empty_between_visits() {
        // The live shape while he is away: schedule fields present, inventory [].
        let got = fetch_baro(&baro_http(serde_json::json!({
            "activation": "2026-08-21T13:00:00.000Z",
            "expiry": "2026-08-23T13:00:00.000Z",
            "location": "Orcus Relay (Pluto)",
            "inventory": []
        })));
        assert!(got.contains_key("activation"), "schedule still lands");
        assert!(!got.contains_key("inventory"), "absent, so carry-forward can fire");
        assert!(!got.contains_key("inventory_for"));
    }

    #[test]
    fn baro_entries_without_an_item_name_are_dropped() {
        let got = fetch_baro(&baro_http(serde_json::json!({
            "activation": "a", "expiry": "b", "location": "c",
            "inventory": [{"ducats": 350}, {"item": "Primed Fury"}]
        })));
        let inv = got.get("inventory").unwrap().as_array().unwrap();
        assert_eq!(inv.len(), 1);
        assert_eq!(inv[0].get("item").unwrap(), "Primed Fury");
        // Optional fields simply do not appear rather than defaulting to 0 —
        // a missing ducat cost is unknown, not free.
        assert!(inv[0].get("ducats").is_none());
    }

    fn riven_http(weapons: &[(&str, &str, f64)]) -> FixtureHttp {
        let arr: Vec<serde_json::Value> = weapons
            .iter()
            .map(|(slug, name, d)| serde_json::json!({
                "slug": slug, "disposition": d, "group": "primary", "rivenType": "rifle",
                "reqMasteryRank": 8, "gameRef": format!("/Lotus/Weapons/{slug}"),
                "i18n": {"en": {"name": name}}
            }))
            .collect();
        let mut r = HashMap::new();
        r.insert(WFM_RIVEN_WEAPONS_URL.into(), serde_json::json!({"apiVersion": "0.25.0", "data": arr}));
        FixtureHttp { responses: r }
    }

    fn rivens_surface(weapons: &[(&str, f64)], changes: serde_json::Value) -> HashMap<String, serde_json::Value> {
        let mut w = serde_json::Map::new();
        for (slug, d) in weapons {
            w.insert(slug.to_string(), serde_json::json!({"name": slug, "disposition": d}));
        }
        let mut m = HashMap::new();
        m.insert("weapons".into(), serde_json::Value::Object(w));
        m.insert("changes".into(), changes);
        m
    }

    #[test]
    fn rivens_reduce_the_manifest_and_report_no_changes_without_a_prior() {
        let now = clock::parse_isoformat_utc("2026-08-16T12:00:00Z").unwrap();
        let got = fetch_rivens(&riven_http(&[("kulstar", "Kulstar", 1.3), ("braton", "Braton", 1.15)]));
        let w = got.get("weapons").unwrap().as_object().unwrap();
        assert_eq!(w.len(), 2);
        assert_eq!(w["kulstar"]["name"], "Kulstar");
        assert_eq!(w["kulstar"]["disposition"], 1.3);
        assert_eq!(w["kulstar"]["group"], "primary");
        assert_eq!(w["kulstar"]["riven_type"], "rifle");
        assert_eq!(w["kulstar"]["req_mr"], 8);
        // The fetch no longer computes the log at all — the caller does, after
        // applying DE's dispositions. See `riven_change_log`.
        assert!(!got.contains_key("changes"));
        let changes = riven_change_log(got["weapons"].as_object().unwrap(), None, now);
        assert!(changes.is_empty(), "no prior means no changes");
    }

    #[test]
    fn rivens_diff_against_prior_and_carry_the_log_within_retention() {
        let now = clock::parse_isoformat_utc("2026-08-16T12:00:00Z").unwrap();
        let prior = rivens_surface(
            &[("kulstar", 1.3), ("braton", 1.10), ("lato", 1.4)],
            serde_json::json!([
                // recent: kept
                {"slug": "lato", "name": "Lato", "from": 1.35, "to": 1.4, "seen_at": "2026-07-01T00:00:00Z"},
                // older than 90 d: dropped
                {"slug": "burston", "name": "Burston", "from": 1.0, "to": 1.05, "seen_at": "2026-04-01T00:00:00Z"},
                // superseded by a change seen now
                {"slug": "braton", "name": "Braton", "from": 1.05, "to": 1.10, "seen_at": "2026-07-15T00:00:00Z"}
            ]),
        );
        let got = fetch_rivens(&riven_http(&[
            ("kulstar", "Kulstar", 1.3),
            ("braton", "Braton", 1.15),
            ("lato", "Lato", 1.4),
        ]));
        let ch = riven_change_log(got["weapons"].as_object().unwrap(), Some(&prior), now);
        let slugs: Vec<&str> = ch.iter().map(|c| c["slug"].as_str().unwrap()).collect();
        assert_eq!(slugs, vec!["braton", "lato"], "newest first; burston aged out; braton superseded");
        assert_eq!(ch[0]["from"], 1.10);
        assert_eq!(ch[0]["to"], 1.15);
        assert_eq!(ch[0]["seen_at"], "2026-08-16T12:00:00Z");
    }

    #[test]
    fn riven_change_log_diffs_published_values_not_the_wfm_mirror() {
        // The bug: the log used to be computed inside the fetch, so it diffed
        // warframe.market's fresh mirror against the prior snapshot's STORED
        // values — which are DE's, because the override lands afterwards.
        //
        // Here WFM still reports the OLD 1.30 while DE has already moved
        // Kulstar to 1.15, and the prior snapshot recorded DE's old 1.30.
        // Diffing the mirror would log 1.30 → 1.30 (nothing) and then publish
        // 1.15 unannounced; diffing what we actually publish logs the real move.
        let now = clock::parse_isoformat_utc("2026-08-16T12:00:00Z").unwrap();
        let prior = rivens_surface(&[("kulstar", 1.30)], serde_json::json!([]));

        let mut published = serde_json::Map::new();
        published.insert(
            "kulstar".into(),
            serde_json::json!({"name": "Kulstar", "disposition": 1.15}),
        );
        let ch = riven_change_log(&published, Some(&prior), now);
        assert_eq!(ch.len(), 1, "the real DE move is logged");
        assert_eq!(ch[0]["from"], 1.30);
        assert_eq!(ch[0]["to"], 1.15);
    }

    #[test]
    fn riven_change_log_reports_nothing_when_the_published_value_is_unchanged() {
        // The other half: on a carry cycle the published value equals the prior
        // one even though WFM's mirror disagrees. No change occurred, so none
        // may be logged — a phantom entry here would tell users a disposition
        // moved when it did not.
        let now = clock::parse_isoformat_utc("2026-08-16T12:00:00Z").unwrap();
        let prior = rivens_surface(&[("kulstar", 1.15)], serde_json::json!([]));
        let mut published = serde_json::Map::new();
        published.insert(
            "kulstar".into(),
            serde_json::json!({"name": "Kulstar", "disposition": 1.15}),
        );
        assert!(riven_change_log(&published, Some(&prior), now).is_empty());
    }

    #[test]
    fn rivens_fetch_failure_is_an_empty_surface_for_reconcile_to_fall_back() {
        let got = fetch_rivens(&FixtureHttp { responses: HashMap::new() });
        assert!(got.is_empty());
    }

    #[test]
    fn rivens_carry_game_ref_and_the_attributes_manifest() {
        // The riven-weapons fixture does not serve /riven/attributes, so the
        // surface is built without it and the call still succeeds.
        let got = fetch_rivens(&riven_http(&[("kulstar", "Kulstar", 1.3)]));
        let w = got.get("weapons").unwrap().as_object().unwrap();
        assert_eq!(w["kulstar"]["game_ref"], "/Lotus/Weapons/kulstar");

        // Serve the attributes manifest and re-run: the surface gains it.
        let mut r = HashMap::new();
        r.insert(
            WFM_RIVEN_ATTRIBUTES_URL.into(),
            serde_json::json!({"data": [
                {"gameRef": "WeaponCritDamageMod", "slug": "critical_damage",
                 "i18n": {"en": {"name": "Critical Damage"}}, "unit": "percent"},
                {"gameRef": "WeaponPunctureDepthMod", "slug": "punch_through",
                 "i18n": {"en": {"name": "Punch Through"}}}
            ]}),
        );
        let mut weapons = serde_json::Map::new();
        weapons.insert(
            "kulstar".into(),
            serde_json::json!({"slug": "kulstar", "name": "Kulstar", "disposition": 1.3}),
        );
        r.insert(
            WFM_RIVEN_WEAPONS_URL.into(),
            serde_json::json!({"data": [
                {"slug": "kulstar", "disposition": 1.3, "gameRef": "/Lotus/x",
                 "i18n": {"en": {"name": "Kulstar"}}}
            ]}),
        );
        let got2 = fetch_rivens(&FixtureHttp { responses: r });
        let attrs = got2.get("attributes").unwrap().as_array().unwrap();
        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs[0]["game_ref"], "WeaponCritDamageMod");
        assert_eq!(attrs[0]["unit"], "percent");
        assert!(attrs[1].get("unit").is_none(), "non-percent stats carry no unit");
    }

    fn weekly_http(body: &str) -> FixtureHttp {
        // The DE file is a JS literal; the raw-text fetch stands in for it as
        // a JSON string fixture (see Http::get_text's default impl).
        let mut r = HashMap::new();
        r.insert(DE_WEEKLY_RIVENS_URL.into(), serde_json::Value::String(body.into()));
        FixtureHttp { responses: r }
    }

    fn weapons_by_name() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("acceltra".into(), "acceltra".into());
        m.insert("ack & brunt".into(), "ack_brunt".into());
        m.insert("ax-52".into(), "ax_52".into());
        m
    }

    #[test]
    fn weekly_rivens_parses_the_js_object_literal() {
        let body = r#"[
            { itemType: 'Rifle Riven Mod', compatibility: null, rerolled: false,
              avg: 68.39, stddev: 250.66, min: 3, max: 2000, pop: 15, median: 10 },
            { itemType: 'Rifle Riven Mod', compatibility: 'AX-52', rerolled: true,
              avg: 204.69, stddev: 387.32, min: 2, max: 2069, pop: 6, median: 75 },
            { itemType: 'Rifle Riven Mod', compatibility: 'Acceltra', rerolled: false,
              avg: 41.75, stddev: 45.23, min: 5, max: 400, pop: 10, median: 35 }
        ]"#;
        let rows = parse_weekly_rivens(body).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].compatibility, None);
        assert_eq!(rows[1].compatibility.as_deref(), Some("AX-52"));
        assert!(rows[1].rerolled);
        assert_eq!(rows[1].median, 75.0);
        assert_eq!(rows[2].pop, 10);
    }

    #[test]
    fn weekly_rivens_rejects_garbage_loudly() {
        assert!(parse_weekly_rivens("not js at all").is_err());
        // Missing colon between key and value.
        assert!(parse_weekly_rivens("[{itemType 'x'}]").is_err());
        // Two values in an array without a separator.
        assert!(parse_weekly_rivens("[1 2]").is_err());
    }

    #[test]
    fn riven_stats_reduce_by_weapon_and_reroll_state_with_drop_counting() {
        let body = r#"[
            { itemType: 'Rifle Riven Mod', compatibility: 'Acceltra', rerolled: false,
              avg: 41.75, stddev: 45.23, min: 5, max: 400, pop: 10, median: 35 },
            { itemType: 'Rifle Riven Mod', compatibility: 'Acceltra', rerolled: true,
              avg: 266.72, stddev: 580.64, min: 5, max: 4600, pop: 12, median: 100 },
            { itemType: 'Rifle Riven Mod', compatibility: 'AX-52', rerolled: false,
              avg: 87.2, stddev: 300.77, min: 5, max: 3000, pop: 10, median: 30 },
            { itemType: 'Rifle Riven Mod', compatibility: 'NotARealWeapon', rerolled: false,
              avg: 1.0, stddev: 0.5, min: 1, max: 2, pop: 1, median: 1 },
            { itemType: 'Melee Riven Mod', compatibility: null, rerolled: false,
              avg: 52.85, stddev: 274.96, min: 2, max: 2300, pop: 8, median: 7 }
        ]"#;
        let (stats, unmatched, _) = fetch_riven_stats(&weekly_http(body), &weapons_by_name());
        assert_eq!(unmatched, 1, "only the unknown weapon counts");
        let acceltra = stats.get("acceltra").unwrap();
        assert_eq!(acceltra["name"], "Acceltra");
        assert_eq!(acceltra["unrolled"]["median"], 35.0);
        assert_eq!(acceltra["rolled"]["median"], 100.0);
        assert_eq!(acceltra["rolled"]["pop"], 12);
        assert_eq!(acceltra["unrolled"]["avg"], 41.75);
        let ax = stats.get("ax_52").unwrap();
        assert!(ax.get("rolled").is_none(), "no rolled rows for AX-52");
        assert_eq!(ax["unrolled"]["min"], 5.0);
        assert!(!stats.contains_key("ack_brunt"));
    }

    #[test]
    fn riven_stats_fold_consoles_under_platforms_without_disturbing_pc() {
        let pc = r#"[
            { itemType: 'Rifle Riven Mod', compatibility: 'Acceltra', rerolled: false,
              avg: 41.75, stddev: 45.23, min: 5, max: 400, pop: 10, median: 35 }
        ]"#;
        let ps4 = r#"[
            { itemType: 'Rifle Riven Mod', compatibility: 'Acceltra', rerolled: false,
              avg: 88.0, stddev: 60.0, min: 10, max: 500, pop: 30, median: 75 },
            { itemType: 'Rifle Riven Mod', compatibility: 'Ack & Brunt', rerolled: false,
              avg: 9.0, stddev: 2.0, min: 7, max: 14, pop: 3, median: 9 }
        ]"#;
        let mut r = HashMap::new();
        r.insert(DE_WEEKLY_RIVENS_URL.into(), serde_json::Value::String(pc.into()));
        r.insert(
            DE_WEEKLY_RIVEN_PLATFORMS[0].1.into(),
            serde_json::Value::String(ps4.into()),
        );
        let (stats, _, _) = fetch_riven_stats(&FixtureHttp { responses: r }, &weapons_by_name());

        let acceltra = stats.get("acceltra").unwrap();
        // PC keeps its place at the top level, untouched.
        assert_eq!(acceltra["unrolled"]["median"], 35.0);
        // The console rides along underneath — console riven prices diverge
        // sharply from PC's, which is the whole point of carrying them.
        assert_eq!(acceltra["platforms"]["ps4"]["unrolled"]["median"], 75.0);
        assert_eq!(acceltra["platforms"]["ps4"]["unrolled"]["pop"], 30);

        // A weapon only the console saw has no PC baseline to compare against,
        // so it contributes nothing rather than a lone console-only row.
        assert!(!stats.contains_key("ack_brunt"));
    }

    #[test]
    fn riven_stats_survive_one_platform_failing() {
        // A Switch outage must not cost us PC, which is what we actually price
        // against.
        let pc = r#"[
            { itemType: 'Rifle Riven Mod', compatibility: 'Acceltra', rerolled: false,
              avg: 41.75, stddev: 45.23, min: 5, max: 400, pop: 10, median: 35 }
        ]"#;
        let (stats, _, children) = fetch_riven_stats(&weekly_http(pc), &weapons_by_name());
        assert_eq!(children["pc"], RivenChildOutcome::Usable);
        assert_eq!(children["swi"], RivenChildOutcome::Unavailable);
        let acceltra = stats.get("acceltra").unwrap();
        assert_eq!(acceltra["unrolled"]["median"], 35.0);
        assert!(acceltra.get("platforms").is_none());
    }

    #[test]
    fn riven_stats_fetch_failure_is_empty_for_reconcile_to_fall_back() {
        let (stats, unmatched, children) = fetch_riven_stats(&FixtureHttp { responses: HashMap::new() }, &weapons_by_name());
        assert!(stats.is_empty());
        assert_eq!(unmatched, 0);
        assert_eq!(children["pc"], RivenChildOutcome::Unavailable);
    }

    #[test]
    fn failed_console_carries_only_onto_fresh_pc_weapons() {
        let prior = HashMap::from([
            ("kept".into(), serde_json::json!({"name":"Kept","unrolled":{"median":10},"platforms":{"swi":{"unrolled":{"median":20}}}})),
            ("gone".into(), serde_json::json!({"name":"Gone","platforms":{"swi":{"unrolled":{"median":30}}}})),
        ]);
        let mut fresh = HashMap::from([
            ("kept".into(), serde_json::json!({"name":"Kept","unrolled":{"median":11}})),
        ]);
        let failed = HashMap::from([
            ("pc".into(), RivenChildOutcome::Usable),
            ("swi".into(), RivenChildOutcome::Invalid),
        ]);
        carry_failed_riven_platforms(&mut fresh, Some(&prior), &failed);
        assert_eq!(fresh["kept"]["platforms"]["swi"]["unrolled"]["median"], 20);
        assert!(!fresh.contains_key("gone"), "console-only stale weapons stay gone");

        let mut cleared = fresh.clone();
        cleared.get_mut("kept").unwrap().as_object_mut().unwrap().remove("platforms");
        let cleared_outcomes = HashMap::from([
            ("pc".into(), RivenChildOutcome::Usable),
            ("swi".into(), RivenChildOutcome::AuthoritativeEmpty),
        ]);
        carry_failed_riven_platforms(&mut cleared, Some(&prior), &cleared_outcomes);
        assert!(cleared["kept"].get("platforms").is_none(), "valid empty Switch data clears prior");
    }

    fn set_catalog() -> HashMap<String, String> {
        let mut m = HashMap::new();
        for n in ["revenant prime", "baruuk prime", "nezha prime", "octavia prime", "oberon prime", "gauss prime"] {
            m.insert(format!("{n} set"), format!("{}_set", n.replace(' ', "_")));
        }
        m
    }

    #[test]
    fn pack_names_resolve_to_frames() {
        assert_eq!(frames_in_pack_name("M P V Revenant Baruuk Prime Dual Pack"), vec!["Revenant Prime", "Baruuk Prime"]);
        assert_eq!(frames_in_pack_name("Nezha & Octavia Prime Dual Pack"), vec!["Nezha Prime", "Octavia Prime"]);
        assert_eq!(frames_in_pack_name("M P V Oberon Prime Single Pack"), vec!["Oberon Prime"]);
        assert!(frames_in_pack_name("Last Chance Item C").is_empty());
        assert!(frames_in_pack_name("").is_empty());
    }

    #[test]
    fn resurgence_rotations_bound_each_window_by_the_previous_expiry() {
        let vt = serde_json::json!({
            "activation": "2026-08-06T18:00:00.000Z",
            "expiry": "2026-09-03T18:00:00.000Z",
            "initialStart": "2022-09-09T15:42:24.266Z",
            "inventory": [
                {"item": "M P V Revenant Prime Single Pack", "ducats": 6},
                {"item": "M P V Revenant Baruuk Prime Dual Pack", "ducats": 10}
            ],
            "schedule": [
                {"expiry": "2022-11-03T18:00:00.000Z", "item": "M P V Oberon Prime Single Pack"},
                {"expiry": "2022-12-01T19:00:00.000Z", "item": "Last Chance Item C"},
                {"expiry": "2023-01-05T19:00:00.000Z", "item": "Nezha & Octavia Prime Dual Pack"},
                {"expiry": "2023-02-02T19:00:00.000Z"}
            ]
        });
        let (rot, cur) = resurgence_rotations(&vt, &set_catalog());
        assert_eq!(rot.len(), 2, "unresolvable packs are skipped but still advance the window");
        assert_eq!(rot[0]["from"], "2022-09-09T15:42:24.266Z");
        assert_eq!(rot[0]["to"], "2022-11-03T18:00:00.000Z");
        assert_eq!(rot[0]["frames"], serde_json::json!(["oberon_prime_set"]));
        assert_eq!(rot[1]["from"], "2022-12-01T19:00:00.000Z", "the skipped rotation still bounds the next one");
        assert_eq!(rot[1]["frames"], serde_json::json!(["nezha_prime_set", "octavia_prime_set"]));
        let cur = cur.unwrap();
        assert_eq!(cur["from"], "2026-08-06T18:00:00.000Z");
        assert_eq!(cur["frames"], serde_json::json!(["revenant_prime_set", "baruuk_prime_set"]));
    }

    #[test]
    fn calendar_primes_come_from_the_wfstat_payload_and_need_a_wfm_set() {
        let raw = serde_json::json!([
            {"name": "Gauss Prime", "category": "Warframes", "releaseDate": "2024-01-17", "vaulted": true, "vaultDate": "2025-12-10", "estimatedVaultDate": "2025-12-10"},
            {"name": "Gauss Prime Helmet", "category": "Skins"},
            {"name": "Unknown Prime", "category": "Warframes", "releaseDate": "2020-01-01"},
            {"name": "Revenant Prime", "category": "Warframes", "releaseDate": "2022-08-30", "vaulted": false}
        ]);
        let http = FixtureHttp { responses: HashMap::new() }; // vaultTrader absent → warning, no rotations
        let cal = fetch_calendar(&http, Some(&raw), &set_catalog());
        let primes = cal["primes"].as_object().unwrap();
        assert_eq!(primes.len(), 2);
        assert_eq!(primes["gauss_prime_set"]["vault_date"], "2025-12-10");
        assert_eq!(primes["gauss_prime_set"]["vaulted"], true);
        assert_eq!(primes["revenant_prime_set"]["vaulted"], false);
        assert!(primes["revenant_prime_set"].get("vault_date").is_none());
        assert!(!cal.contains_key("resurgence"));
    }

    #[test]
    fn carry_forward_preserves_the_last_visits_stock() {
        let prior: HashMap<String, serde_json::Value> = serde_json::from_value(serde_json::json!({
            "activation": "2026-08-21T13:00:00.000Z",
            "expiry": "2026-08-23T13:00:00.000Z",
            "location": "Orcus Relay (Pluto)",
            "inventory": [{"item": "Primed Fury", "ducats": 350}],
            "inventory_for": "2026-08-21T13:00:00.000Z"
        }))
        .unwrap();
        // The next scrape, after he has left: new schedule, no stock.
        let mut fresh: HashMap<String, serde_json::Value> = serde_json::from_value(serde_json::json!({
            "activation": "2026-09-04T13:00:00.000Z",
            "expiry": "2026-09-06T13:00:00.000Z",
            "location": "Kronia Relay (Saturn)"
        }))
        .unwrap();

        carry_baro_inventory(&mut fresh, Some(&prior));

        // The NEW schedule wins; the OLD stock is kept and still labelled with
        // the visit it came from, so a consumer can see it is not current.
        assert_eq!(fresh.get("activation").unwrap(), "2026-09-04T13:00:00.000Z");
        assert_eq!(fresh.get("inventory").unwrap().as_array().unwrap().len(), 1);
        assert_eq!(fresh.get("inventory_for").unwrap(), "2026-08-21T13:00:00.000Z");
    }

    #[test]
    fn carry_forward_never_overwrites_a_live_capture() {
        let prior: HashMap<String, serde_json::Value> = serde_json::from_value(serde_json::json!({
            "inventory": [{"item": "Old Thing"}],
            "inventory_for": "2026-08-21T13:00:00.000Z"
        }))
        .unwrap();
        let mut fresh: HashMap<String, serde_json::Value> = serde_json::from_value(serde_json::json!({
            "activation": "2026-09-04T13:00:00.000Z",
            "inventory": [{"item": "New Thing"}],
            "inventory_for": "2026-09-04T13:00:00.000Z"
        }))
        .unwrap();

        carry_baro_inventory(&mut fresh, Some(&prior));

        assert_eq!(fresh.get("inventory").unwrap()[0].get("item").unwrap(), "New Thing");
        assert_eq!(fresh.get("inventory_for").unwrap(), "2026-09-04T13:00:00.000Z");
    }

    #[test]
    fn carry_forward_is_inert_without_usable_prior_data() {
        let mut fresh: HashMap<String, serde_json::Value> =
            serde_json::from_value(serde_json::json!({"activation": "x"})).unwrap();
        carry_baro_inventory(&mut fresh, None);
        assert!(!fresh.contains_key("inventory"));

        // A prior with a list but no `inventory_for` is pre-upgrade data; taking
        // it would leave stock that cannot be dated.
        let partial: HashMap<String, serde_json::Value> =
            serde_json::from_value(serde_json::json!({"inventory": [{"item": "x"}]})).unwrap();
        carry_baro_inventory(&mut fresh, Some(&partial));
        assert!(!fresh.contains_key("inventory"));

        // A totally failed fetch stays failed — carry-forward must not
        // resurrect a surface reconcile is about to treat as empty.
        let mut failed: HashMap<String, serde_json::Value> = HashMap::new();
        let full: HashMap<String, serde_json::Value> = serde_json::from_value(serde_json::json!({
            "inventory": [{"item": "x"}], "inventory_for": "t"
        }))
        .unwrap();
        carry_baro_inventory(&mut failed, Some(&full));
        assert!(failed.is_empty());
    }
    fn parent_http() -> FixtureHttp {
        // The two endpoints that still exist, both empty — isolates the
        // sentinel path.
        let mut r = HashMap::new();
        r.insert("https://api.warframestat.us/warframes/".into(), serde_json::json!([]));
        r.insert("https://api.warframestat.us/weapons/".into(), serde_json::json!([]));
        FixtureHttp { responses: r }
    }

    fn sentinel_catalog() -> HashMap<String, String> {
        let mut c = HashMap::new();
        c.insert("carrier prime set".into(), "carrier_prime_set".into());
        c.insert("carrier prime cerebrum".into(), "carrier_prime_cerebrum".into());
        c
    }

    #[test]
    fn sentinel_parents_come_from_the_bulk_item_payload() {
        // /sentinels/ 404s upstream since ~2026-07-31; these parents must still
        // resolve, out of the /items/ payload we already download.
        let items = serde_json::json!([
            {"name": "Excalibur", "category": "Warframes", "components": []},
            {"name": "Carrier Prime", "category": "Sentinels", "components": [
                {"uniqueName": "/Lotus/Types/Sentinels/CarrierPrime/Cerebrum", "name": "Cerebrum", "itemCount": 2}
            ]}
        ]);
        let (p2i, s2p, complete) =
            fetch_parent_data(&parent_http(), &sentinel_catalog(), Some(&items));

        assert!(complete, "both live endpoints answered and the payload was present");
        let info = p2i
            .get("/Lotus/Types/Sentinels/CarrierPrime/Cerebrum")
            .expect("sentinel component path resolved");
        assert_eq!(info.get("slug").unwrap(), "carrier_prime_cerebrum");
        // Category comes from the item's own field, not the fallback.
        assert_eq!(info.get("category").unwrap(), "Sentinels");

        let set = s2p.get("carrier_prime_set").expect("sentinel set built");
        assert_eq!(set.get("name").unwrap(), "Carrier Prime");
        assert_eq!(set.get("parts").unwrap().as_array().unwrap().len(), 1);
        assert_eq!(set["parts"][0]["quantity"], 2);
    }

    #[test]
    fn non_prime_sentinels_are_ignored_like_every_other_parent() {
        let items = serde_json::json!([
            {"name": "Carrier", "category": "Sentinels", "components": [
                {"uniqueName": "/Lotus/Types/Sentinels/Carrier/Cerebrum", "name": "Cerebrum"}
            ]}
        ]);
        let (p2i, s2p, _) = fetch_parent_data(&parent_http(), &sentinel_catalog(), Some(&items));
        assert!(p2i.is_empty() && s2p.is_empty());
    }

    #[test]
    fn a_missing_item_payload_marks_the_surface_incomplete() {
        // `complete` is what makes reconcile MERGE over the prior snapshot
        // instead of replacing it. Getting this wrong would silently delete
        // every sentinel prime the moment the bulk fetch failed.
        let (_, _, complete) = fetch_parent_data(&parent_http(), &sentinel_catalog(), None);
        assert!(!complete);
    }

    #[test]
    fn a_failed_live_endpoint_still_marks_the_surface_incomplete() {
        let empty = FixtureHttp { responses: HashMap::new() };
        let (_, _, complete) =
            fetch_parent_data(&empty, &sentinel_catalog(), Some(&serde_json::json!([])));
        assert!(!complete);
    }
}
