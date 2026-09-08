use super::Http;
use std::collections::HashMap;

/// Fetch Baro Ki'Teer's schedule from warframestat. Returns {} on failure
/// or missing fields - the Baro card hides.
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
    let activation = data
        .get("activation")
        .and_then(|a| a.as_str())
        .unwrap_or("");
    let expiry = data.get("expiry").and_then(|e| e.as_str()).unwrap_or("");
    let location = data.get("location").and_then(|l| l.as_str()).unwrap_or("");
    if activation.is_empty() || expiry.is_empty() || location.is_empty() {
        return HashMap::new();
    }
    let mut out = HashMap::new();
    out.insert(
        "activation".into(),
        serde_json::Value::String(activation.into()),
    );
    out.insert("expiry".into(), serde_json::Value::String(expiry.into()));
    out.insert(
        "location".into(),
        serde_json::Value::String(location.into()),
    );

    // What he is actually selling - the part that decides whether any of this
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
/// WHOLE surface is empty, and Baro's is never empty - activation, expiry and
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
