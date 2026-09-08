use super::Http;
use crate::clock;
use chrono::DateTime;
use chrono::Utc;
use std::collections::HashMap;

const VAULT_SOON_DAYS: i64 = 60;

/// Fetch prime vault status from WFCD warframe-items sources.
/// Returns `(vault_status, complete)` - `complete` false when any source
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
            let vaulted = parent
                .get("vaulted")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
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
            for comp in parent
                .get("components")
                .and_then(|c| c.as_array())
                .unwrap_or(&vec![])
            {
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

pub const WFSTAT_VAULT_TRADER_URL: &str = "https://api.warframestat.us/pc/vaultTrader/";

/// The price-shock calendar the hold/sell advisor reasons over, as one
/// `calendar` surface in market.json:
///
/// - `primes: {set_slug: {name, released, vaulted, vault_date,
///   est_vault_date}}` - per prime set, from warframestat's item catalog
///   (WFCD warframe-items carries `releaseDate` / `vaultDate` /
///   `estimatedVaultDate` / `vaulted`; the same payload the build already
///   downloads for the resolver, so no extra request).
/// - `resurgence: [{from, to, frames: [set_slug]}]` - every Prime Resurgence
///   rotation warframestat knows (its `vaultTrader.schedule` is the full
///   history since 2022, one entry per rotation with the pack name and its
///   expiry; a rotation runs from the previous expiry to its own), plus
///   `resurgence_current` for the one running now.
///
/// Why: a set's price has three predictable shocks - Prime Access release
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
            let Some(name) = it.get("name").and_then(|n| n.as_str()) else {
                continue;
            };
            if !name.ends_with(" Prime") {
                continue;
            }
            let Some(released) = it.get("releaseDate").and_then(|d| d.as_str()) else {
                continue;
            };
            let Some(slug) = catalog.get(&format!("{} set", name.to_lowercase())) else {
                unmatched += 1;
                continue;
            };
            let mut row = serde_json::Map::new();
            row.insert("name".into(), serde_json::Value::String(name.into()));
            row.insert(
                "released".into(),
                serde_json::Value::String(released.into()),
            );
            row.insert(
                "vaulted".into(),
                serde_json::Value::Bool(
                    it.get("vaulted").and_then(|v| v.as_bool()).unwrap_or(false),
                ),
            );
            for (src, dst) in [
                ("vaultDate", "vault_date"),
                ("estimatedVaultDate", "est_vault_date"),
            ] {
                if let Some(d) = it.get(src).and_then(|d| d.as_str()) {
                    row.insert(dst.into(), serde_json::Value::String(d.into()));
                }
            }
            primes.insert(slug.clone(), serde_json::Value::Object(row));
        }
    }
    if unmatched > 0 {
        eprintln!(
            "  calendar: {unmatched} primes with release dates have no WFM set in the catalog"
        );
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
    let Some(base) = s.strip_suffix(" Prime") else {
        return vec![];
    };
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
    let mut prev_expiry: Option<String> = vt
        .get("initialStart")
        .and_then(|s| s.as_str())
        .map(String::from);
    if let Some(sched) = vt.get("schedule").and_then(|s| s.as_array()) {
        for entry in sched {
            let Some(expiry) = entry.get("expiry").and_then(|e| e.as_str()) else {
                continue;
            };
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
