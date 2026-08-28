//! Render — CSV rows → item entries → full snapshot.
//!
//! Pure transform with injected clock. No network I/O; no dependency on
//! the fetch stage. The fetch-dependent surfaces (`path_to_info`,
//! `set_to_parts`, etc.) are passed in by the caller so unit tests can
//! feed frozen inputs.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::clock;
use crate::csvin;
use market_math::{clamp_avg, clamp_low5};

#[derive(Debug, Clone, Serialize, Default)]
pub struct CatalogItemMeta {
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ducats: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_rank: Option<i64>,
    pub subtypes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ItemEntry {
    pub avg: f64,
    pub low_sell: i64,
    pub low5_avg: f64,
    pub top_buy: i64,
    pub vol: i64,
    pub ratio: f64,
    pub buys: i64,
    pub sells: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ducats: Option<i64>,
    pub median_now: f64,
    pub median_90d: f64,
    pub medians_7d: Vec<f64>,
    pub donch_top_90d: i64,
    pub donch_bot_90d: i64,
}



/// Render a single CSV row into an [`ItemEntry`].
pub fn render_item(row: &csvin::CsvRow, meta: &CatalogItemMeta) -> ItemEntry {
    let avg_raw = parse_f64_or(&row.avg_price_48h, 0.0);
    let med90 = parse_f64_or(&row.median_90d, 0.0);
    let ls = parse_i64_or(&row.low_sell_price, 0);
    let low5_raw = parse_f64_or(&row.low5_avg, 0.0);

    ItemEntry {
        avg: clamp_avg(avg_raw, med90),
        low_sell: ls,
        low5_avg: clamp_low5(low5_raw, med90, ls as f64),
        top_buy: parse_i64_or(&row.top_buy_price, 0),
        vol: parse_i64_or(&row.volume_48h, 0),
        ratio: parse_f64_or(&row.buy_sell_ratio, 0.0),
        buys: parse_i64_or(&row.live_buys, 0),
        sells: parse_i64_or(&row.live_sells, 0),
        tags: meta.tags.clone(),
        ducats: meta.ducats,
        median_now: {
            let raw = row.median_now.as_str();
            // old CSVs lack median_now — fall back to median_90d
            let val = if raw.is_empty() { row.median_90d.as_str() } else { raw };
            parse_f64_or(val, 0.0)
        },
        median_90d: med90,
        medians_7d: csvin::parse_medians_7d(&row.medians_7d),
        donch_top_90d: parse_i64_or(&row.donch_top_90d, 0),
        donch_bot_90d: parse_i64_or(&row.donch_bot_90d, 0),
    }
}

/// Render all rows into a slug→[`ItemEntry`] map.
pub fn render_items(
    rows: &[csvin::CsvRow],
    meta_by_slug: &HashMap<String, CatalogItemMeta>,
) -> HashMap<String, ItemEntry> {
    let mut items = HashMap::new();
    for r in rows {
        let slug = r.url_name.clone();
        if slug.is_empty() {
            continue;
        }
        let meta = meta_by_slug.get(&slug).cloned().unwrap_or_default();
        items.insert(slug, render_item(r, &meta));
    }
    items
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub updated_at: String,
    pub platform: String,
    pub item_count: usize,
    pub catalog_count: usize,
    pub source: String,
    pub catalog: HashMap<String, String>,
    pub items: HashMap<String, ItemEntry>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub path_to_info: HashMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub set_to_parts: HashMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub relic_rewards: HashMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub vault_status: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub baro: HashMap<String, serde_json::Value>,
    /// `weapons: {slug: {name, disposition, group, riven_type, req_mr}}` +
    /// `changes: [{slug, name, from, to, seen_at}]` (rolling 90 d) — see
    /// `fetch::fetch_rivens`.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub rivens: HashMap<String, serde_json::Value>,
    /// `primes: {set_slug: {name, released, vaulted, vault_date, …}}` +
    /// `resurgence: [{from, to, pack, frames}]` + `resurgence_current` — see
    /// `fetch::fetch_calendar`.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub calendar: HashMap<String, serde_json::Value>,
    /// `{slug: {name, unrolled?, rolled?}}` — DE's weekly riven price bands
    /// per weapon × reroll-state — see `fetch::fetch_riven_stats`.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub riven_stats: HashMap<String, serde_json::Value>,
    /// `{slug: {build_price, build_time, rush_price, ingredients[]}}` keyed by
    /// the item the recipe PRODUCES — see `de_extract::recipes_from_export`.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub recipes: HashMap<String, serde_json::Value>,
    /// `{slug: {name, category, year, share, peak_mr, by_mr[]}}` — DE's annual
    /// usage telemetry, keyed by the PARENT's slug. See
    /// `de_extract::usage_from_export`.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub usage: HashMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "UsageHistorySurface::is_empty")]
    pub usage_history: UsageHistorySurface,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub surface_fetched_at: HashMap<String, String>,
    /// Observation provenance is separate from data freshness: an unchanged
    /// hash check is fresh evidence about old data, while an outage is not.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub surface_provenance: HashMap<String, SurfaceProvenance>,
    #[serde(default, skip_serializing_if = "EventRewardsSurface::is_empty")]
    pub event_rewards: EventRewardsSurface,
    /// Digital Extremes provenance + the worldState-only surfaces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub de: Option<DeSurface>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct UsageHistorySurface {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub years: Vec<u16>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub by_year: std::collections::BTreeMap<u16, HashMap<String, serde_json::Value>>,
}

impl UsageHistorySurface {
    pub fn is_empty(&self) -> bool { self.years.is_empty() || self.by_year.is_empty() }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct EventRewardsSurface {
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub goals: std::collections::BTreeMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub events: std::collections::BTreeMap<String, serde_json::Value>,
}

impl EventRewardsSurface {
    pub fn is_empty(&self) -> bool { self.goals.is_empty() && self.events.is_empty() }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SurfaceProvenance {
    pub disposition: crate::reconcile::Disposition,
    pub attempted_at: String,
    pub data_fetched_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// What one cycle took from DE, and what only DE can give.
///
/// `hashes` is provenance AND mechanism: next cycle compares it against a
/// freshly fetched 490-byte index and refetches only what moved. It is carried
/// forward verbatim when DE is unreachable, so an outage costs one skipped
/// cycle rather than a full re-download on recovery.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct DeSurface {
    /// Export manifest basename → content hash, as published this cycle.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub hashes: std::collections::BTreeMap<String, String>,
    /// Manifests whose hash moved this cycle — a patch-day signal in itself.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed: Vec<String>,
    /// Whether worldState answered with an object. This does not describe
    /// individual children, and Baro may still be fresh from warframestat.
    pub world_ok: bool,
    /// Child timestamps do not inherit their parent's freshness. A successful
    /// PC weekly-riven fetch must not make a failed Switch child look current.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub child_fetched_at: std::collections::BTreeMap<String, String>,
    /// Announced Prime Vault rotations — the real thing, not the estimate.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vault_rotation: Vec<serde_json::Value>,
    /// Darvo's daily deal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deals: Vec<serde_json::Value>,
    /// `{slug: ducats}` for exactly the slugs DE's recipe tree set.
    ///
    /// This is PROVENANCE, and it is load-bearing. Ducats are an override on a
    /// catalogue rebuilt from warframe.market every cycle, so when the recipes
    /// manifest fails the override has to be re-applied from the prior
    /// snapshot — and without knowing which values were DE's, that carry would
    /// also stamp stale WFM values over fresh, legitimately-corrected ones.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub ducats: std::collections::BTreeMap<String, i64>,
    /// Only slugs whose disposition was matched to DE. This prevents an
    /// outage from treating every value in WFM's mirror as first-party.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub dispositions: std::collections::BTreeMap<String, f64>,
}

/// Assemble a [`Snapshot`] from rendered items + catalog + all surfaces.
///
/// The `now` parameter is the injected clock (see [`clock`]); every
/// timestamp in the snapshot — including per-surface stamps — flows
/// through it.
#[allow(clippy::too_many_arguments)]
pub fn assemble_snapshot(
    now: DateTime<Utc>,
    catalog: HashMap<String, String>,
    items: HashMap<String, ItemEntry>,
    path_to_info: HashMap<String, serde_json::Value>,
    set_to_parts: HashMap<String, serde_json::Value>,
    relic_rewards: HashMap<String, serde_json::Value>,
    vault_status: HashMap<String, String>,
    baro: HashMap<String, serde_json::Value>,
    rivens: HashMap<String, serde_json::Value>,
    calendar: HashMap<String, serde_json::Value>,
    riven_stats: HashMap<String, serde_json::Value>,
    recipes: HashMap<String, serde_json::Value>,
    usage: HashMap<String, serde_json::Value>,
    usage_history: UsageHistorySurface,
    surface_fetched_at: HashMap<String, String>,
) -> Snapshot {
    Snapshot {
        updated_at: clock::iso_z(now),
        platform: "pc".to_string(),
        item_count: items.len(),
        catalog_count: catalog.len(),
        source: "bootstrap from wfm_results.csv + /v2/items".to_string(),
        // Set by the caller after assembly — DE's surfaces are built from
        // manifests that may legitimately be absent this cycle.
        de: None,
        catalog,
        items,
        path_to_info,
        set_to_parts,
        relic_rewards,
        vault_status,
        baro,
        rivens,
        calendar,
        riven_stats,
        recipes,
        usage,
        usage_history,
        surface_fetched_at,
        surface_provenance: HashMap::new(),
        event_rewards: EventRewardsSurface::default(),
    }
}

fn parse_f64_or(s: &str, default: f64) -> f64 {
    let s = s.trim();
    if s.is_empty() {
        return default;
    }
    s.parse().unwrap_or(default)
}

fn parse_i64_or(s: &str, default: i64) -> i64 {
    let s = s.trim();
    if s.is_empty() {
        return default;
    }
    // The scraper writes some integer columns float-formatted ("15.0");
    // Python does int(float(s)) — truncation toward zero. Falling back to a
    // bare i64 parse silently zeroed every such value (caught by the first
    // real-data shadow run: 2857 Donchian diffs).
    s.parse().or_else(|_| s.parse::<f64>().map(|f| f.trunc() as i64)).unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use csvin::CsvRow;

    fn test_meta() -> CatalogItemMeta {
        CatalogItemMeta {
            tags: vec!["mod".to_string()],
            ducats: Some(0),
            max_rank: Some(10),
            subtypes: vec![],
        }
    }

    fn test_row() -> CsvRow {
        CsvRow {
            url_name: "primed_continuity".into(),
            name: "Primed Continuity".into(),
            live_buys: "12".into(),
            live_sells: "18".into(),
            buy_sell_ratio: "0.67".into(),
            top_buy_price: "35".into(),
            low_sell_price: "42".into(),
            low5_avg: "42.6".into(),
            volume_48h: "384".into(),
            avg_price_48h: "43.2".into(),
            median_now: "37".into(),
            median_90d: "33".into(),
            medians_7d: "[33, 36, 42]".into(),
            donch_top_90d: "45".into(),
            donch_bot_90d: "15".into(),
            ..Default::default()
        }
    }

    #[test]
    fn render_item_clamps_and_populates_all_fields() {
        let entry = render_item(&test_row(), &test_meta());
        assert_eq!(entry.avg, 43.2); // within 3x of median_90d(33) → unchanged
        assert_eq!(entry.low_sell, 42);
        assert_eq!(entry.low5_avg, 42.6); // within 3x of 33 → unchanged
        assert_eq!(entry.top_buy, 35);
        assert_eq!(entry.vol, 384);
        assert_eq!(entry.ratio, 0.67);
        assert_eq!(entry.buys, 12);
        assert_eq!(entry.sells, 18);
        assert_eq!(entry.tags, vec!["mod"]);
        assert_eq!(entry.ducats, Some(0));
        assert_eq!(entry.median_now, 37.0);
        assert_eq!(entry.median_90d, 33.0);
        assert_eq!(entry.medians_7d, vec![33.0, 36.0, 42.0]);
        assert_eq!(entry.donch_top_90d, 45);
        assert_eq!(entry.donch_bot_90d, 15);
    }

    #[test]
    fn render_item_clamp_avg_triggers_on_outlier() {
        let mut row = test_row();
        row.avg_price_48h = "500".into();
        row.median_90d = "9".into();
        let entry = render_item(&row, &test_meta());
        assert_eq!(entry.avg, 9.0); // clamped to median
    }

    #[test]
    fn render_item_clamp_low5_triggers_on_cliff_book() {
        let mut row = test_row();
        row.low5_avg = "35.2".into();
        row.low_sell_price = "5".into();
        row.median_90d = "5".into();
        let entry = render_item(&row, &test_meta());
        assert_eq!(entry.low5_avg, 5.0); // clamped to median
    }

    #[test]
    fn render_item_median_now_falls_back_to_median_90d() {
        let mut row = test_row();
        row.median_now = "".into(); // old CSV
        row.median_90d = "33".into();
        let entry = render_item(&row, &test_meta());
        assert_eq!(entry.median_now, 33.0);
    }

    #[test]
    fn render_item_empty_fields_default_to_zero() {
        let row = CsvRow {
            url_name: "empty_item".into(),
            ..Default::default()
        };
        let entry = render_item(&row, &CatalogItemMeta::default());
        assert_eq!(entry.avg, 0.0);
        assert_eq!(entry.low_sell, 0);
        assert_eq!(entry.low5_avg, 0.0);
        assert_eq!(entry.top_buy, 0);
        assert_eq!(entry.vol, 0);
        assert_eq!(entry.ratio, 0.0);
        assert_eq!(entry.buys, 0);
        assert_eq!(entry.sells, 0);
        assert_eq!(entry.median_now, 0.0);
        assert_eq!(entry.median_90d, 0.0);
        assert!(entry.medians_7d.is_empty());
        assert_eq!(entry.donch_top_90d, 0);
        assert_eq!(entry.donch_bot_90d, 0);
        assert!(entry.tags.is_empty());
        assert_eq!(entry.ducats, None);
    }

    #[test]
    fn render_items_multiple_rows() {
        let rows = vec![
            CsvRow { url_name: "slug_a".into(), low_sell_price: "10".into(), median_90d: "10".into(), ..Default::default() },
            CsvRow { url_name: "slug_b".into(), low_sell_price: "20".into(), median_90d: "20".into(), ..Default::default() },
        ];
        let meta = HashMap::new();
        let items = render_items(&rows, &meta);
        assert_eq!(items.len(), 2);
        assert_eq!(items.get("slug_a").unwrap().low_sell, 10);
        assert_eq!(items.get("slug_b").unwrap().low_sell, 20);
    }

    #[test]
    fn render_items_skips_empty_slug() {
        let rows = vec![
            CsvRow { url_name: "".into(), ..Default::default() },
            CsvRow { url_name: "ok".into(), low_sell_price: "5".into(), median_90d: "5".into(), ..Default::default() },
        ];
        let items = render_items(&rows, &HashMap::new());
        assert_eq!(items.len(), 1);
    }
}
