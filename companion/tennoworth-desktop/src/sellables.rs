//! Desktop "what to sell right now": join the latest inventory snapshot × the
//! market snapshot, rank by the shared sell-priority score, and return the top
//! sellables. Backs BOTH the tray menu and the post-scan notification (C6).
//!
//! Three moving parts:
//!   1. RESOLUTION. The snapshot stores DE item paths (`/Lotus/...`); the market
//!      is keyed by WFM slug. `resolve` mirrors the primary paths of
//!      prototype/src/lib/resolver.ts: market.json's baked `path_to_info`
//!      (direct path→slug for prime parts/warframes), then the wfstat catalog
//!      (path→name, with Component/Blueprint trimming) → market's `catalog`
//!      (name→slug), then a de-camelled name guess. Relic refinement subtypes
//!      are NOT reconstructed — the snapshot doesn't carry per-instance subtype —
//!      so relics collapse to their base slug (a low-value edge for a top-5 tray).
//!   2. SCORING. `market_math::sell_priority` — the single source of truth shared
//!      with the SPA (parity-tested). We only rank; we never re-derive the score.
//!   3. DATA FLOOR. Both catalogs are bundled via `include_str!` from the
//!      committed `prototype/public/*.json`, so the tray works offline on a first
//!      run before any C4 refresh. The live market prefers the app-data cache
//!      (last known-good from the server) over the compile-time bundle.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use market_math::sell_priority::{self, PricedEntry};
use serde::Deserialize;

use crate::db::Db;
use crate::market::MarketCache;

/// Compile-time floors — always present because `prototype/public/*.json` is
/// committed (unlike the gitignored `dist-desktop`, so `include_str!` never
/// breaks a fresh checkout). The market floor is only used when the app-data
/// cache is absent/corrupt; the catalog never changes at runtime.
const BUNDLED_MARKET: &str = include_str!("../../../prototype/public/market.json");
const BUNDLED_CATALOG: &str = include_str!("../../../prototype/public/wfstat-catalog.json");

/// One ranked sellable row — the shape the tray label and the notification both
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

/// The post-scan notification payload: how many items are worth listing and
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
}

#[derive(Deserialize)]
struct SetEntry {
    #[serde(default)]
    parts: Vec<SetPart>,
}

/// The three maps the join needs from a market snapshot, ignoring the rest.
#[derive(Deserialize)]
pub struct MarketData {
    #[serde(default)]
    items: HashMap<String, MarketEntry>,
    /// name (lowercased) → WFM slug.
    #[serde(default)]
    catalog: HashMap<String, String>,
    /// DE path → {name, slug} — prime parts pre-baked by the scraper.
    #[serde(default)]
    path_to_info: HashMap<String, PathInfo>,
    #[serde(default)]
    usage: HashMap<String, serde_json::Value>,
    #[serde(default)]
    set_to_parts: HashMap<String, SetEntry>,
    #[serde(skip)]
    usage_parent_by_part: HashMap<String, Option<String>>,
}

#[derive(Deserialize)]
struct SlimInfo {
    name: String,
}

/// Parse the bundled wfstat catalog once (DE path → display name). Static: the
/// catalog is compile-time data that never changes at runtime.
fn wfstat_catalog() -> &'static HashMap<String, SlimInfo> {
    static CATALOG: OnceLock<HashMap<String, SlimInfo>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        // Slim `[uniqueName, {name, category}]` pairs — same shape resolver.ts
        // reads. A parse failure yields an empty map (path_to_info still works).
        let pairs: Vec<(String, SlimInfo)> = serde_json::from_str(BUNDLED_CATALOG).unwrap_or_default();
        pairs.into_iter().collect()
    })
}

impl MarketData {
    /// Load the freshest market we hold: the app-data cache (last known-good from
    /// tennoworth.app) if present and parseable, else the compile-time bundle.
    pub fn load(cache: &MarketCache) -> MarketData {
        if let Some(body) = cache.cached() {
            if let Ok(mut m) = serde_json::from_str::<MarketData>(&body) {
                m.build_usage_parent_index();
                return m;
            }
        }
        Self::bundled()
    }

    fn bundled() -> MarketData {
        let mut market = serde_json::from_str(BUNDLED_MARKET).unwrap_or(MarketData {
            items: HashMap::new(),
            catalog: HashMap::new(),
            path_to_info: HashMap::new(),
            usage: HashMap::new(),
            set_to_parts: HashMap::new(),
            usage_parent_by_part: HashMap::new(),
        });
        market.build_usage_parent_index();
        market
    }

    fn build_usage_parent_index(&mut self) {
        let mut index: HashMap<String, Option<String>> = HashMap::new();
        for (parent, set) in &self.set_to_parts {
            for part in &set.parts {
                if part.slug.is_empty() { continue; }
                match index.get(&part.slug) {
                    None => { index.insert(part.slug.clone(), Some(parent.clone())); }
                    Some(Some(existing)) if existing != parent => { index.insert(part.slug.clone(), None); }
                    _ => {}
                }
            }
        }
        self.usage_parent_by_part = index;
    }

    fn usage_share(&self, slug: &str) -> Option<f64> {
        self.usage_resolution(slug).map(|(_, share, _)| share)
    }

    fn usage_resolution(&self, slug: &str) -> Option<(String, f64, bool)> {
        if let Some(direct) = self.usage.get(slug) {
            return valid_usage_share(direct).map(|share| (slug.to_string(), share, false));
        }
        let parent = self.usage_parent_by_part.get(slug)?.as_deref()?;
        let share = self.usage.get(parent).and_then(valid_usage_share)?;
        Some((parent.to_string(), share, true))
    }

    /// Resolve a DE item path to `(display name, WFM slug)`, or `None` when the
    /// path maps to nothing tradeable. Mirrors resolver.ts's non-relic paths.
    fn resolve(&self, path: &str) -> Option<(String, String)> {
        // 1. Pre-baked direct hit (prime parts / warframes / recipes).
        if let Some(d) = self.path_to_info.get(path) {
            if !d.slug.is_empty() {
                return Some((d.name.clone(), d.slug.clone()));
            }
        }

        // 2. wfstat catalog: path → name, retrying with the Component/Blueprint
        //    suffix trimmed (the same fallback resolver.ts uses).
        let cat = wfstat_catalog();
        let mut info = cat.get(path);
        if info.is_none() {
            for suffix in ["Component", "Blueprint"] {
                if let Some(trimmed) = path.strip_suffix(suffix) {
                    if let Some(hit) = cat.get(trimmed) {
                        info = Some(hit);
                        break;
                    }
                }
            }
        }

        // 3. No catalog entry: guess a display name from the path basename and
        //    accept it ONLY on an exact market.catalog hit (strict, like the TS —
        //    a bad guess can never fabricate an item).
        let Some(info) = info else {
            let guess = path_name_guess(path)?;
            let slug = self.catalog.get(&guess.to_lowercase())?;
            return Some((guess, slug.clone()));
        };

        // 4. name → slug via the market catalog, else a slug guess (which may not
        //    exist in `items`; the caller drops it then).
        let slug = self
            .catalog
            .get(&info.name.to_lowercase())
            .cloned()
            .unwrap_or_else(|| slug_guess(&info.name));
        Some((info.name.clone(), slug))
    }
}

fn valid_usage_share(value: &serde_json::Value) -> Option<f64> {
    let row = value.as_object()?;
    if row.get("name").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).is_none()
        || row.get("category").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).is_none()
        || row.get("year").and_then(|v| v.as_u64()).filter(|year| *year > 0).is_none()
        || row.get("peak_mr").and_then(|v| v.as_f64()).filter(|v| v.is_finite() && *v >= 0.0).is_none()
    {
        return None;
    }
    let by_mr = row.get("by_mr").and_then(|v| v.as_array()).filter(|values| !values.is_empty())?;
    if !by_mr.iter().all(|value| value.as_f64().is_some_and(|v| v.is_finite() && v >= 0.0)) {
        return None;
    }
    row.get("share").and_then(|v| v.as_f64()).filter(|v| v.is_finite() && *v >= 0.0)
}

/// De-camel a path basename into a display-name guess, trimming Blueprint /
/// Component first — ".../SagekPrimeBarrelBlueprint" → "Sagek Prime Barrel".
/// Mirrors `pathNameGuess` in resolver.ts.
fn path_name_guess(path: &str) -> Option<String> {
    let mut base = path.rsplit('/').next().unwrap_or("");
    for suffix in ["Blueprint", "Component"] {
        if let Some(trimmed) = base.strip_suffix(suffix) {
            base = trimmed;
        }
    }
    if base.is_empty() {
        return None;
    }
    Some(decamel(base))
}

/// Insert a space between a lowercase/digit and an uppercase letter, matching
/// resolver.ts's `/([a-z0-9])([A-Z])/g` → `$1 $2`.
fn decamel(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 4);
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 && c.is_ascii_uppercase() {
            let prev = chars[i - 1];
            if prev.is_ascii_lowercase() || prev.is_ascii_digit() {
                out.push(' ');
            }
        }
        out.push(c);
    }
    out
}

/// `slugGuess` from resolver.ts: strip non-alphanumerics (keep spaces), trim,
/// lowercase, collapse whitespace to underscores.
fn slug_guess(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == ' ' { c } else { ' ' })
        .collect::<String>();
    // Collapse runs of whitespace to single underscores; trims ends implicitly
    // (split_whitespace drops leading/trailing/empty tokens).
    cleaned
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("_")
        .to_lowercase()
}

/// Read the user's global "keep N copies" reserve from settings (the SPA's
/// `reserve-copies`), clamped ≥ 0. A missing/garbage value is 0 (no reserve),
/// matching the SPA's parse-or-zero.
fn reserve_copies(db: &Db) -> i64 {
    db.get_setting("reserve-copies")
        .ok()
        .flatten()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .filter(|&n| n >= 0)
        .unwrap_or(0)
}

/// Rank every sellable item in the latest snapshot, highest prioritization score first.
/// Items resolving to the same slug (rare) are aggregated. Rows with no market
/// entry, nothing left after the reserve, or a zero score are dropped. Sorted
/// by score desc, then slug asc for a deterministic order (the SPA uses
/// insertion order on ties; ties are irrelevant for a top-N tray).
pub fn rank_sellables(db: &Db, market: &MarketData) -> Vec<SellableRow> {
    let items = db.latest_snapshot_items().unwrap_or_default();
    let reserve = reserve_copies(db);

    // Aggregate by resolved slug so two DE paths that map to one WFM item don't
    // produce duplicate rows.
    let mut by_slug: BTreeMap<String, (String, i64, i64)> = BTreeMap::new();
    for it in items {
        let Some((name, slug)) = market.resolve(&it.slug) else {
            continue;
        };
        let entry = by_slug.entry(slug).or_insert((name, 0, 0));
        entry.1 += it.count;
        entry.2 += it.leveled;
    }

    let mut rows: Vec<SellableRow> = Vec::new();
    for (slug, (name, count, leveled)) in by_slug {
        let Some(entry) = market.items.get(&slug) else {
            continue;
        };
        let sellable = sell_priority::sellable_qty(count, reserve, leveled);
        if sellable <= 0 {
            continue;
        }
        let priced = entry.priced();
        let score = sell_priority::score_row_weighted(
            sellable as f64,
            &priced,
            market.usage_share(&slug),
        );
        if score.sell_score <= 0.0 {
            continue;
        }
        rows.push(SellableRow {
            name,
            slug,
            sellable_qty: sellable,
            price: sell_priority::clearing_price(&priced),
            score: score.sell_score,
        });
    }

    rows.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.slug.cmp(&b.slug))
    });
    rows
}

/// The notification payload for a completed scan, or `None` when nothing is
/// sellable (→ the caller fires no notification). Total is the realizable plat
/// across the WHOLE sellable set, rounded — "N items worth ~Xp to sell".
pub fn build_notification(sellables: &[SellableRow]) -> Option<ScanNotification> {
    if sellables.is_empty() {
        return None;
    }
    let total: f64 = sellables
        .iter()
        .map(|r| r.price * r.sellable_qty as f64)
        .sum();
    Some(ScanNotification {
        count: sellables.len(),
        total_plat: total.round() as i64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- cross-language ranking parity (Rust consumer side) ---------------
    // The TS canonical side lives in prototype/src/lib/sell-priority.parity.test.ts;
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
                Some((c.slug.clone(), sell_priority::score_row(sellable as f64, &priced).sell_score))
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
            let sellable = sell_priority::sellable_qty(
                test_case.count, test_case.reserve, test_case.leveled,
            );
            let priced = PricedEntry {
                vol: test_case.market.vol,
                low_sell: test_case.market.low_sell,
                avg: test_case.market.avg,
                median_now: test_case.market.median_now,
                median_90d: test_case.market.median_90d,
            };
            let share = fixture_share(&test_case.usage_share);
            let base = sell_priority::score_row(sellable as f64, &priced).sell_score;
            let weighted = sell_priority::score_row_weighted(sellable as f64, &priced, share).sell_score;
            let weight = sell_priority::liquidity_weight(share);
            let tier = sell_priority::usage_weight_tier(share);
            assert_eq!(base, test_case.expected_base_score, "{} base", test_case.id);
            assert_eq!(weight, test_case.expected_weight, "{} weight", test_case.id);
            assert_eq!(weighted, test_case.expected_score, "{} score", test_case.id);
            assert_eq!(tier, test_case.expected_tier, "{} tier", test_case.id);
            if sellable > 0 { ranked.push((test_case.id, weighted)); }
        }
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then_with(|| a.0.cmp(&b.0)));
        assert_eq!(ranked.into_iter().map(|row| row.0).collect::<Vec<_>>(), fixture.expected_order);
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
        assert_eq!(market.usage_parent_by_part.get("ambiguous_part"), Some(&None));
        for test_case in fixture.cases {
            let hit = market.usage_resolution(&test_case.slug);
            assert_eq!(hit.as_ref().map(|row| row.0.clone()), test_case.expected_source, "{} source", test_case.slug);
            if let Some((_, share, inherited)) = hit {
                assert_eq!(Some(inherited), test_case.expected_inherited, "{} inherited", test_case.slug);
                assert_eq!(Some(share), test_case.expected_share, "{} share", test_case.slug);
            }
        }
    }

    // ---- name-guess parity (Rust consumer side) ----------------------------
    // The TS canonical side lives in prototype/src/lib/resolver.parity.test.ts;
    // both check the SAME fixture (tests/fixtures/name-guess/cases.json). If
    // this fails but the TS passes (or vice versa), path_name_guess/slug_guess
    // have diverged from resolver.ts's pathNameGuess/slugGuess.
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
    struct NameGuessFixture {
        path_name_guess_cases: Vec<PathGuessCase>,
        slug_guess_cases: Vec<SlugGuessCase>,
    }

    #[test]
    fn name_guess_matches_resolver_ts_on_shared_fixture() {
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
                "path_name_guess({:?}) diverged from resolver.ts's pathNameGuess",
                c.path
            );
        }
        for c in &fx.slug_guess_cases {
            assert_eq!(
                slug_guess(&c.name),
                c.expected,
                "slug_guess({:?}) diverged from resolver.ts's slugGuess",
                c.name
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
        assert!(m.items.contains_key(&slug), "resolved slug is in the market");
    }

    #[test]
    fn unresolvable_and_untradeable_paths_are_none_or_slugless() {
        let m = MarketData::bundled();
        // Pure garbage path → None.
        assert!(m.resolve("/Lotus/Nonsense/DoesNotExistXyzzy").is_none());
        // Orokin Cell resolves to a name but has no WFM market entry — resolve
        // returns a slug, but rank_sellables drops it (no `items` entry).
        let cell = m.resolve("/Lotus/Types/Items/MiscItems/OrokinCell");
        if let Some((_, slug)) = cell {
            assert!(!m.items.contains_key(&slug), "orokin cell is not tradeable");
        }
    }

    // ---- decamel / slug_guess helpers ------------------------------------
    #[test]
    fn decamel_inserts_spaces_like_the_ts_regex() {
        assert_eq!(decamel("SagekPrimeBarrel"), "Sagek Prime Barrel");
        assert_eq!(decamel("AcceleratedBlast"), "Accelerated Blast");
        assert_eq!(decamel("Already Spaced"), "Already Spaced");
    }

    #[test]
    fn slug_guess_matches_the_ts_normalisation() {
        assert_eq!(slug_guess("Ash Prime Blueprint"), "ash_prime_blueprint");
        assert_eq!(slug_guess("Secura Dual Cestra"), "secura_dual_cestra");
        assert_eq!(slug_guess("  Odd -- Name!! "), "odd_name");
    }

    // ---- build_notification ----------------------------------------------
    fn row(slug: &str, qty: i64, price: f64, score: f64) -> SellableRow {
        SellableRow { name: slug.into(), slug: slug.into(), sellable_qty: qty, price, score }
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
            Some(ScanNotification { count: 2, total_plat: 141 })
        );
    }
}
