use super::Http;
use std::collections::HashMap;

/// Fetch relic drop tables (Intact state only) from drops.warframestat.us.
/// Returns {} on any failure - the relic planner UI degrades gracefully.
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
        let tier = row
            .get("tier")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_lowercase();
        let name = row
            .get("relicName")
            .and_then(|n| n.as_str())
            .unwrap_or("")
            .to_lowercase();
        if tier.is_empty() || name.is_empty() {
            continue;
        }
        let relic_slug = format!("{tier}_{name}_relic");
        let mut rewards = Vec::new();
        for r in row
            .get("rewards")
            .and_then(|r| r.as_array())
            .unwrap_or(&vec![])
        {
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
