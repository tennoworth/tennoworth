//! Desktop "what to sell right now": join the latest inventory snapshot × the
//! market snapshot, rank by the shared sell-priority score, and return the top
//! sellables. Backs BOTH the tray menu and the post-scan notification (C6).
//!
//! Three moving parts:
//!   1. RESOLUTION. The snapshot stores DE item paths (`/Lotus/...`); the market
//!      is keyed by WFM slug. `resolve` mirrors the primary paths of
//!      market_domain::inventory's resolution: market.json's baked `path_to_info`
//!      (direct path→slug for prime parts/warframes), then the wfstat catalog
//!      (path→name, with Component/Blueprint trimming) → market's `catalog`
//!      (name→slug), then a de-camelled name guess. Relic refinement subtypes
//!      are NOT reconstructed - the snapshot doesn't carry per-instance subtype -
//!      so relics collapse to their base slug (a low-value edge for a top-5 tray).
//!   2. SCORING. `market_math::sell_priority` - the single source of truth shared
//!      with the SPA (parity-tested). We only rank; we never re-derive the score.
//!   3. DATA FLOOR. Both catalogs are bundled via `include_str!` from the
//!      committed `frontend/public/*.json`, so the tray works offline on a first
//!      run before any C4 refresh. The live market prefers the app-data cache
//!      (last known-good from the server) over the compile-time bundle.

use std::collections::HashMap;
#[cfg(test)]
use std::collections::BTreeMap;

#[cfg(test)]
use market_domain::inventory::{path_guess_candidates, slug_guess};
use market_math::sell_priority::PricedEntry;
#[cfg(test)]
use market_math::sell_priority;
use serde::{Deserialize, Deserializer};

#[cfg(test)]
use crate::persistence::Db;
#[cfg(test)]
use crate::services::market::MarketCache;

mod market;
mod quantities;
mod ranking;
mod resolution;
mod rewards;

pub use ranking::{build_notification, rank_sellables};

/// Compile-time floors - always present because `frontend/public/*.json` is
/// committed (unlike the gitignored `dist-desktop`, so `include_str!` never
/// breaks a fresh checkout). The market floor is only used when the app-data
/// cache is absent/corrupt; the catalog never changes at runtime.
pub(crate) const BUNDLED_MARKET: &str = include_str!("../../../../frontend/public/market.json");
const BUNDLED_CATALOG: &str = include_str!("../../../../frontend/public/wfstat-catalog.json");

/// One ranked sellable row - the shape the tray label and the notification both
/// read, and what `top_sellables` returns to the SPA. `price` is the clamped
/// clearing price; `score` is the usage-weighted prioritization score.
#[derive(serde::Serialize, Debug, Clone, PartialEq)]
pub struct SellableRow {
    pub name: String,
    pub slug: String,
    pub sellable_qty: i64,
    pub price: f64,
    pub score: f64,
}

/// The post-scan summary payload: how many items are worth listing and
/// their total realizable plat (Σ sellable_qty × clearing price).
#[derive(serde::Serialize, Debug, Clone, Copy, PartialEq)]
pub struct ScanNotification {
    pub count: usize,
    pub total_plat: i64,
}

#[derive(Deserialize)]
struct MarketEntry {
    #[serde(default)]
    vol: f64,
    #[serde(default)]
    low_sell: f64,
    #[serde(default)]
    avg: f64,
    #[serde(default)]
    median_now: f64,
    #[serde(default)]
    median_90d: f64,
    #[serde(default)]
    low5_avg: f64,
    #[serde(default)]
    ducats: Option<i64>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OverlayCatalogItem {
    pub name: String,
    pub slug: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OverlayMarketFacts {
    pub cached_platinum: Option<u32>,
    pub ducats: Option<u32>,
}

impl MarketEntry {
    fn priced(&self) -> PricedEntry {
        PricedEntry {
            vol: self.vol,
            low_sell: self.low_sell,
            avg: self.avg,
            median_now: self.median_now,
            median_90d: self.median_90d,
        }
    }
}

#[derive(Deserialize)]
struct PathInfo {
    #[serde(default)]
    name: String,
    #[serde(default)]
    slug: String,
}

#[derive(Deserialize)]
struct SetPart {
    #[serde(default)]
    slug: String,
    #[serde(default)]
    quantity: Option<u32>,
}

#[derive(Deserialize)]
struct SetEntry {
    #[serde(default)]
    name: String,
    #[serde(default)]
    parts: Vec<SetPart>,
}

#[derive(Deserialize)]
struct RelicRewardEntry {
    #[serde(default)]
    reward_name: String,
    #[serde(default)]
    reward_slug: String,
}

fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// The three maps the join needs from a market snapshot, ignoring the rest.
#[derive(Deserialize)]
pub struct MarketData {
    /// The snapshot's own stamp. Read only to order the app-data cache against
    /// the bundled floor; older cached files predate it, hence the default.
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    items: HashMap<String, MarketEntry>,
    /// name (lowercased) → WFM slug.
    #[serde(default, deserialize_with = "null_default")]
    catalog: HashMap<String, String>,
    /// DE path → {name, slug} - prime parts pre-baked by the scraper.
    #[serde(default, deserialize_with = "null_default")]
    path_to_info: HashMap<String, PathInfo>,
    #[serde(default, deserialize_with = "null_default")]
    usage: HashMap<String, serde_json::Value>,
    #[serde(default, deserialize_with = "null_default")]
    set_to_parts: HashMap<String, SetEntry>,
    #[serde(default, deserialize_with = "null_default")]
    relic_rewards: HashMap<String, Vec<RelicRewardEntry>>,
    #[serde(skip)]
    usage_parent_by_part: HashMap<String, Option<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use market_domain::inventory::path_name_guess;

    #[derive(Deserialize)]
    struct CompositionInventoryRow {
        path: String,
        name: String,
        slug: String,
        count: i64,
        xp: i64,
    }

    #[derive(Deserialize)]
    struct CompositionExpectedRow {
        slug: String,
        sellable_qty: i64,
    }

    #[derive(Deserialize)]
    struct CompositionExpected {
        shared_order: Vec<String>,
        tray: Vec<CompositionExpectedRow>,
    }

    #[derive(Deserialize)]
    struct CompositionFixture {
        reserve_copies: i64,
        inventory: Vec<CompositionInventoryRow>,
        market: serde_json::Value,
        protected: BTreeMap<String, u32>,
        legacy_reserves: BTreeMap<String, i64>,
        traded_after_scan: String,
        expected: CompositionExpected,
    }

    #[test]
    fn tray_composes_the_shared_scan_with_local_protection_and_trade_history() {
        let raw = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/tray-table-composition/cases.json"
        ));
        let fixture: CompositionFixture = serde_json::from_str(raw).unwrap();
        let mut market_json = fixture.market;
        let path_to_info: serde_json::Map<String, serde_json::Value> = fixture
            .inventory
            .iter()
            .map(|row| {
                (
                    row.path.clone(),
                    serde_json::json!({ "name": row.name, "slug": row.slug }),
                )
            })
            .collect();
        market_json["path_to_info"] = serde_json::Value::Object(path_to_info);
        let market: MarketData = serde_json::from_value(market_json).unwrap();
        let db = Db::open_in_memory().unwrap();
        let snapshot: Vec<crate::persistence::SnapshotItem> = fixture
            .inventory
            .iter()
            .map(|row| crate::persistence::SnapshotItem {
                slug: row.path.clone(),
                count: row.count,
                leveled: if row.xp > 0 { row.count } else { 0 },
            })
            .collect();
        let scan_id = db
            .insert_snapshot("memory", Some("1970-01-01T00:01:40Z"), None, &snapshot)
            .unwrap();
        db.set_setting("reserve-copies", &fixture.reserve_copies.to_string())
            .unwrap();
        for (slug, keep) in fixture.legacy_reserves {
            db.set_reserve(&slug, keep).unwrap();
        }
        crate::services::protection::ProtectionPlan {
            reserves: fixture.protected,
            goal: None,
        }
        .save(&db, &market)
        .unwrap();
        let boundary = crate::services::eelog::LogPosition {
            session: "composition".into(),
            start: 100,
            end: 100,
            observed_after: 100,
        };
        db.save_allowance(crate::services::allowance::Observation::scanned(
            "account".into(),
            scan_id,
            &serde_json::json!({"TradesRemaining": 8}),
            Some(boundary.clone()),
            Some(boundary.clone()),
            100,
            100,
        ))
        .unwrap();
        db.insert_trade(
            &crate::services::eelog::TradeEvent {
                partner: "Buyer".into(),
                kind: "sale".into(),
                plat: 50,
                log_stamp: Some("110".into()),
                items: vec![crate::services::eelog::TradeItem {
                    name: fixture.traded_after_scan,
                    qty: 1,
                    direction: "given".into(),
                }],
            },
            110,
            &crate::services::eelog::LogPosition {
                start: 101,
                end: 120,
                observed_after: 110,
                ..boundary
            },
        )
        .unwrap();

        let ranked = rank_sellables(&db, &market);
        assert_eq!(
            ranked.iter().map(|row| (&row.slug, row.sellable_qty)).collect::<Vec<_>>(),
            fixture.expected.tray.iter().map(|row| (&row.slug, row.sellable_qty)).collect::<Vec<_>>()
        );
        assert_eq!(
            ranked.iter().map(|row| row.slug.clone()).collect::<Vec<_>>(),
            fixture.expected.shared_order
        );
    }

    /// A unique empty dir per test, so a parallel run never shares a cache.
    fn temp_dir(tag: &str) -> std::path::PathBuf {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "tennoworth-sellables-{}-{}-{}",
            tag,
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // ---- cross-language ranking parity (Rust consumer side) ---------------
    // The TS canonical side lives in frontend/src/domain/sell-priority.parity.test.ts;
    // both rank the SAME fixture into `expected_order`. If this fails but the TS
    // passes (or vice versa), the two scorings have diverged.
    #[derive(Deserialize)]
    struct PMarket {
        vol: f64,
        low_sell: f64,
        avg: f64,
        median_now: f64,
        median_90d: f64,
    }
    #[derive(Deserialize)]
    struct PCase {
        slug: String,
        count: i64,
        reserve: i64,
        leveled: i64,
        market: PMarket,
    }
    #[derive(Deserialize)]
    struct PFixture {
        cases: Vec<PCase>,
        expected_order: Vec<String>,
    }

    #[test]
    fn ranking_matches_sell_priority_ts_on_shared_fixture() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/sell-priority/cases.json"
        );
        let raw = std::fs::read_to_string(path).expect("read the shared parity fixture");
        let fx: PFixture = serde_json::from_str(&raw).expect("parse the parity fixture");

        let mut ranked: Vec<(String, f64)> = fx
            .cases
            .iter()
            .filter_map(|c| {
                let sellable = sell_priority::sellable_qty(c.count, c.reserve, c.leveled);
                if sellable <= 0 {
                    return None;
                }
                let priced = PricedEntry {
                    vol: c.market.vol,
                    low_sell: c.market.low_sell,
                    avg: c.market.avg,
                    median_now: c.market.median_now,
                    median_90d: c.market.median_90d,
                };
                Some((
                    c.slug.clone(),
                    sell_priority::score_row(sellable as f64, &priced).sell_score,
                ))
            })
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then_with(|| a.0.cmp(&b.0)));
        let order: Vec<String> = ranked.into_iter().map(|(s, _)| s).collect();

        assert_eq!(
            order, fx.expected_order,
            "Rust ranking diverged from the golden order (and thus sell-priority.ts)"
        );
    }

    #[derive(Deserialize)]
    struct WeightedCase {
        id: String,
        count: i64,
        reserve: i64,
        leveled: i64,
        market: PMarket,
        #[serde(default)]
        usage_share: serde_json::Value,
        expected_base_score: f64,
        expected_weight: f64,
        expected_score: f64,
        expected_tier: String,
    }
    #[derive(Deserialize)]
    struct WeightedFixture {
        cases: Vec<WeightedCase>,
        expected_order: Vec<String>,
    }

    fn fixture_share(value: &serde_json::Value) -> Option<f64> {
        match value {
            serde_json::Value::Number(value) => value.as_f64(),
            serde_json::Value::String(value) if value == "NaN" => Some(f64::NAN),
            serde_json::Value::String(value) if value == "Infinity" => Some(f64::INFINITY),
            _ => None,
        }
    }

    #[test]
    fn weighted_score_matches_ts_on_shared_fixture() {
        let raw = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/usage-weighted-score/cases.json"
        ));
        let fixture: WeightedFixture = serde_json::from_str(raw).unwrap();
        let mut ranked = Vec::new();
        for test_case in fixture.cases {
            let sellable =
                sell_priority::sellable_qty(test_case.count, test_case.reserve, test_case.leveled);
            let priced = PricedEntry {
                vol: test_case.market.vol,
                low_sell: test_case.market.low_sell,
                avg: test_case.market.avg,
                median_now: test_case.market.median_now,
                median_90d: test_case.market.median_90d,
            };
            let share = fixture_share(&test_case.usage_share);
            let base = sell_priority::score_row(sellable as f64, &priced).sell_score;
            let weighted =
                sell_priority::score_row_weighted(sellable as f64, &priced, share).sell_score;
            let weight = sell_priority::liquidity_weight(share);
            let tier = sell_priority::usage_weight_tier(share);
            assert_eq!(base, test_case.expected_base_score, "{} base", test_case.id);
            assert_eq!(weight, test_case.expected_weight, "{} weight", test_case.id);
            assert_eq!(weighted, test_case.expected_score, "{} score", test_case.id);
            assert_eq!(tier, test_case.expected_tier, "{} tier", test_case.id);
            if sellable > 0 {
                ranked.push((test_case.id, weighted));
            }
        }
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then_with(|| a.0.cmp(&b.0)));
        assert_eq!(
            ranked.into_iter().map(|row| row.0).collect::<Vec<_>>(),
            fixture.expected_order
        );
    }

    #[derive(Deserialize)]
    struct ResolutionCase {
        slug: String,
        expected_source: Option<String>,
        expected_inherited: Option<bool>,
        expected_share: Option<f64>,
    }
    #[derive(Deserialize)]
    struct ResolutionFixture {
        cases: Vec<ResolutionCase>,
    }

    #[test]
    fn usage_resolution_matches_ts_on_shared_fixture() {
        let raw = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/usage-resolution/cases.json"
        ));
        let mut market: MarketData = serde_json::from_str(raw).unwrap();
        let fixture: ResolutionFixture = serde_json::from_str(raw).unwrap();
        market.build_usage_parent_index();
        assert_eq!(
            market.usage_parent_by_part.get("ambiguous_part"),
            Some(&None)
        );
        for test_case in fixture.cases {
            let hit = market.usage_resolution(&test_case.slug);
            assert_eq!(
                hit.as_ref().map(|row| row.0.clone()),
                test_case.expected_source,
                "{} source",
                test_case.slug
            );
            if let Some((_, share, inherited)) = hit {
                assert_eq!(
                    Some(inherited),
                    test_case.expected_inherited,
                    "{} inherited",
                    test_case.slug
                );
                assert_eq!(
                    Some(share),
                    test_case.expected_share,
                    "{} share",
                    test_case.slug
                );
            }
        }
    }

    #[test]
    fn load_keeps_current_items_when_auxiliary_surfaces_are_null() {
        let dir = std::env::temp_dir().join(format!(
            "tennoworth-sellables-null-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("market.json"),
            r#"{"items":{"current_only":{"vol":10,"low_sell":12}},"catalog":null,"path_to_info":null,"usage":null,"set_to_parts":null}"#,
        )
        .unwrap();

        let market = MarketData::load(&MarketCache::new(dir.clone()));
        assert!(market.items.contains_key("current_only"));
        assert!(market.catalog.is_empty());
        assert!(market.path_to_info.is_empty());
        assert!(market.usage.is_empty());
        assert!(market.set_to_parts.is_empty());
        assert!(market.usage_parent_by_part.is_empty());

        let _ = std::fs::remove_dir_all(dir);
    }

    /// The bundle ships with each release, so a cache written before an upstream
    /// rollback can be older than the bundle the user just installed. Native
    /// consumers must not stay pinned to it.
    #[test]
    fn load_prefers_the_bundle_over_a_cache_older_than_it() {
        let dir = temp_dir("older-cache");
        std::fs::write(
            dir.join("market.json"),
            r#"{"updated_at":"2020-01-01T00:00:00Z","items":{"rolled_back_only":{"vol":1,"low_sell":1}}}"#,
        )
        .unwrap();

        let market = MarketData::load(&MarketCache::new(dir.clone()));

        assert!(
            !market.items.contains_key("rolled_back_only"),
            "the pre-rollback cache must not win over a newer bundle"
        );
        assert!(
            market.items.contains_key("acceltra_prime_barrel"),
            "the bundle's own rows should be what we loaded"
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    /// The other direction still holds: a cache newer than the bundle wins, which
    /// is the whole reason the cache exists.
    #[test]
    fn load_prefers_a_cache_newer_than_the_bundle() {
        let dir = temp_dir("newer-cache");
        std::fs::write(
            dir.join("market.json"),
            r#"{"updated_at":"2999-01-01T00:00:00Z","items":{"live_only":{"vol":1,"low_sell":1}}}"#,
        )
        .unwrap();

        let market = MarketData::load(&MarketCache::new(dir.clone()));

        assert!(market.items.contains_key("live_only"));
        assert!(
            !market.items.contains_key("acceltra_prime_barrel"),
            "a newer cache replaces the bundle rather than merging with it"
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    /// A cache with no usable stamp cannot be placed in time, so it is not
    /// evidence that the bundle is newer - loading it keeps today's behaviour
    /// instead of silently discarding a perfectly good snapshot.
    #[test]
    fn load_keeps_a_cache_whose_stamp_is_missing_or_unreadable() {
        for (tag, body) in [
            ("no-stamp", r#"{"items":{"unstamped_only":{"vol":1,"low_sell":1}}}"#),
            (
                "bad-stamp",
                r#"{"updated_at":"whenever","items":{"unstamped_only":{"vol":1,"low_sell":1}}}"#,
            ),
        ] {
            let dir = temp_dir(tag);
            std::fs::write(dir.join("market.json"), body).unwrap();

            let market = MarketData::load(&MarketCache::new(dir.clone()));

            assert!(
                market.items.contains_key("unstamped_only"),
                "{tag}: an unorderable cache is still the best snapshot we hold"
            );
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    // ---- name guesses ------------------------------------------------------
    // tests/fixtures/name-guess/cases.json pins market_domain::inventory's
    // guesses, the only implementation since the browser resolver was retired.
    // A 2026-07 drift between two copies of these was a live, if narrow, bug.
    #[derive(Deserialize)]
    struct PathGuessCase {
        path: String,
        expected: Option<String>,
    }
    #[derive(Deserialize)]
    struct SlugGuessCase {
        name: String,
        expected: String,
    }
    #[derive(Deserialize)]
    struct GuessCandidatesCase {
        path: String,
        expected: Vec<String>,
    }
    #[derive(Deserialize)]
    struct NameGuessFixture {
        path_name_guess_cases: Vec<PathGuessCase>,
        slug_guess_cases: Vec<SlugGuessCase>,
        guess_candidates_cases: Vec<GuessCandidatesCase>,
    }

    #[test]
    fn name_guesses_match_the_shared_fixture() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/name-guess/cases.json"
        );
        let raw = std::fs::read_to_string(path).expect("read the shared name-guess fixture");
        let fx: NameGuessFixture =
            serde_json::from_str(&raw).expect("parse the name-guess fixture");

        for c in &fx.path_name_guess_cases {
            assert_eq!(
                path_name_guess(&c.path),
                c.expected,
                "path_name_guess({:?}) diverged from the name-guess fixture",
                c.path
            );
        }
        for c in &fx.slug_guess_cases {
            assert_eq!(
                slug_guess(&c.name),
                c.expected,
                "slug_guess({:?}) diverged from the name-guess fixture",
                c.name
            );
        }
        for c in &fx.guess_candidates_cases {
            assert_eq!(
                path_guess_candidates(&c.path),
                c.expected,
                "path_guess_candidates({:?}) diverged from the name-guess fixture",
                c.path
            );
        }
    }

    // ---- resolver (against the real bundled catalogs) ---------------------
    #[test]
    fn resolves_prime_part_via_path_to_info() {
        let m = MarketData::bundled();
        let (name, slug) = m
            .resolve("/Lotus/Types/Recipes/WarframeRecipes/AshPrimeBlueprint")
            .expect("ash prime bp resolves");
        assert_eq!(slug, "ash_prime_blueprint");
        assert_eq!(name, "Ash Prime Blueprint");
    }

    #[test]
    fn resolves_mod_via_wfstat_catalog_then_market_catalog() {
        let m = MarketData::bundled();
        let (name, slug) = m
            .resolve("/Lotus/Upgrades/Mods/Shotgun/DualStat/AcceleratedBlastMod")
            .expect("accelerated blast resolves");
        assert_eq!(slug, "accelerated_blast");
        assert_eq!(name, "Accelerated Blast");
        assert!(
            m.items.contains_key(&slug),
            "resolved slug is in the market"
        );
    }

    #[test]
    fn unresolvable_and_untradeable_paths_are_none_or_slugless() {
        let m = MarketData::bundled();
        // Pure garbage path → None.
        assert!(m.resolve("/Lotus/Nonsense/DoesNotExistXyzzy").is_none());
        // Orokin Cell resolves to a name but has no WFM market entry - resolve
        // returns a slug, but rank_sellables drops it (no `items` entry).
        let cell = m.resolve("/Lotus/Types/Items/MiscItems/OrokinCell");
        if let Some((_, slug)) = cell {
            assert!(!m.items.contains_key(&slug), "orokin cell is not tradeable");
        }
    }

    // ---- decamel / slug_guess helpers ------------------------------------
    #[test]
    fn decamel_inserts_spaces_like_the_ts_regex() {
        assert_eq!(path_name_guess("SagekPrimeBarrel").unwrap(), "Sagek Prime Barrel");
        assert_eq!(path_name_guess("AcceleratedBlast").unwrap(), "Accelerated Blast");
        assert_eq!(path_name_guess("Already Spaced").unwrap(), "Already Spaced");
    }

    #[test]
    fn slug_guess_matches_the_ts_normalisation() {
        assert_eq!(slug_guess("Ash Prime Blueprint"), "ash_prime_blueprint");
        assert_eq!(slug_guess("Secura Dual Cestra"), "secura_dual_cestra");
        assert_eq!(slug_guess("  Odd -- Name!! "), "odd_name");
    }

    // ---- build_notification ----------------------------------------------
    fn row(slug: &str, qty: i64, price: f64, score: f64) -> SellableRow {
        SellableRow {
            name: slug.into(),
            slug: slug.into(),
            sellable_qty: qty,
            price,
            score,
        }
    }

    #[test]
    fn notification_none_when_nothing_sellable() {
        assert_eq!(build_notification(&[]), None);
    }

    #[test]
    fn notification_counts_items_and_sums_realizable_plat() {
        let rows = vec![row("a", 3, 40.0, 60.0), row("b", 2, 10.5, 10.0)];
        // total = 3×40 + 2×10.5 = 141 → rounded 141.
        assert_eq!(
            build_notification(&rows),
            Some(ScanNotification {
                count: 2,
                total_plat: 141
            })
        );
    }
}
