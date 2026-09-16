//! Offline replay of a statistics-refresh schedule against observation logs.
//!
//! `scrape --observations-dir` records, per sweep, what each attempted item's
//! statistics said and - when the volume gate let the sweep fetch it - what the
//! live book held. `replay` answers the question those logs exist for: if
//! statistics were refreshed every k sweeps instead of every sweep, what would
//! the pipeline have seen, what would it have missed, and how many requests
//! would it have saved?
//!
//! The simulation is a POLICY, not an oracle. At each sweep it refreshes only
//! on evidence a production run could have had at that moment: the cached
//! statistics' age, whether the cached entry was ever usable, the catalog (an
//! item reappearing after an absence), and a response it actually fetched (an
//! empty book). The same sweep's fresh statistics score those decisions
//! afterwards - they never inform them. Triggers only fresh statistics could
//! reveal (a volume-gate crossing, a subtype change) are reported separately as
//! diagnostic bounds and never enter the headline numbers.
//!
//! A book-dependent result is UNKNOWN, not zero, whenever the policy would have
//! applied statistics the observed book was not filtered to: a stale inclusion
//! whose book the sweep never fetched, or a stale subtype that differs from the
//! one the logged book was narrowed to. Unknowns are counted, bounded, and kept
//! out of every "known" rate.
//!
//! Determinism is a contract - the report must be byte-identical across runs
//! over the same logs - so it carries no wall-clock stamp, no absolute path and
//! no unordered map.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use market_math::sell_priority::{self, PricedEntry};
use market_math::{clamp_low5, score};
use serde::{Deserialize, Serialize};

use crate::observations::{self, Outcome, Record};

/// Bumped when the report's meaning changes. A reader refuses what it does not
/// know; the observation `format` is refused independently.
pub const REPORT_FORMAT: u32 = 1;

/// Estimated decoded bytes per response, from the 2026-09-15 host measurement:
/// 785 MB decoded for 1 catalog + 3,840 statistics + 2,598 books, with order
/// books about 90% of the bytes. The logs carry no per-item byte counter, so
/// every byte figure here is an ESTIMATE scaled by these two constants, never a
/// measurement. The catalog request is under 0.1% of the total and is folded
/// into the statistics constant rather than given a fabricated size.
const EST_BYTES_PER_STATISTICS: f64 = 21_435.0;
const EST_BYTES_PER_BOOK: f64 = 285_150.0;

/// The candidate ranking is the production sell-priority score at its owner
/// limit: the logs carry no inventory, so `owned` is unbounded and
/// `units_today` collapses to `daily_sales`. That makes the ranking the
/// market's own sell-through capacity instead of an invented per-user holding.
const CANDIDATE_OWNED: f64 = f64::INFINITY;

/// Ranking depth the recall metrics are defined at. `top20` is the tighter
/// look the plan's gates also care about; `top100` is the headline.
const TOP_K: [usize; 2] = [20, 100];

/// One complete sweep, in start order.
#[derive(Debug, Clone)]
pub struct Sweep {
    /// File name only: the report is portable and deterministic, so it never
    /// carries the absolute directory it happened to be read from.
    pub file: String,
    pub started_at: DateTime<Utc>,
    pub run: observations::Run,
    /// Keyed by slug. A complete sweep attempts every catalog item exactly once.
    pub items: BTreeMap<String, observations::Item>,
}

/// A log the reader refused, with the reason. Empty in a healthy window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedLog {
    pub file: String,
    pub reason: String,
}

/// What the directory held and what was left out.
#[derive(Debug, Clone, Default)]
pub struct Loaded {
    pub sweeps: Vec<Sweep>,
    pub logs_seen: u64,
    pub skipped_incomplete: u64,
    pub skipped_out_of_range: u64,
    pub skipped_malformed: Vec<SkippedLog>,
}

/// A rate that must never be reported without its denominator - "0 false
/// exclusions out of 0 observations" and "0 out of 40,000" are different
/// claims.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RateCount {
    pub count: u64,
    pub total: u64,
    pub rate: f64,
}

/// Simulated versus fresh request counts for one interval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CallComparison {
    pub statistics_calls: u64,
    pub book_calls: u64,
    pub total_requests: u64,
    pub fresh_statistics_calls: u64,
    pub fresh_book_calls: u64,
    pub fresh_total_requests: u64,
    /// Fresh minus simulated; negative is possible when stale inclusions fetch
    /// books the fresh sweep skipped, and is reported rather than clamped.
    pub saving_requests: i64,
    pub saving_ratio: f64,
}

/// Book-dependent outcomes among stale-eligible item-sweeps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct UnknownBooks {
    pub known: u64,
    pub unknown: u64,
    pub coverage: f64,
    /// The fresh sweep found no book to apply (volume fell under the gate, or
    /// statistics came back empty), so the stale inclusion's book is unknown.
    pub unknown_no_fresh_book: u64,
    /// Fresh and cached statistics pick different subtypes; the logged book was
    /// already narrowed to the fresh one, so it cannot be applied to the cache.
    pub unknown_subtype_change: u64,
}

/// Ranking overlap at one depth, with both bounds an unknown can produce.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Overlap {
    pub reference: u64,
    /// Every unknown ranks below all known candidates.
    pub matched_optimistic: u64,
    pub recall_optimistic: f64,
    /// Every unknown ranks above all known candidates, so the known items that
    /// survive occupy only `k - unknowns` slots.
    pub matched_pessimistic: u64,
    pub recall_pessimistic: f64,
}

/// How long a fresh top-100 candidate stayed out of the simulated top-100.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DiscoveryDelay {
    pub reference_candidates: u64,
    pub missing_at_their_sweep: u64,
    pub observed: u64,
    pub censored: u64,
    pub p50_sweeps: f64,
    pub p95_sweeps: f64,
    pub max_sweeps: f64,
    pub p95_seconds: f64,
    /// Delay beyond one further sweep, plus every censored episode - the plan's
    /// "hidden beyond one additional sweep" count.
    pub beyond_one_sweep: u64,
    pub beyond_one_sweep_rate: f64,
}

/// Absolute error of the simulated clearing price on the fresh top-100.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PriceError {
    pub reference: u64,
    pub matched: u64,
    pub p50_absolute: f64,
    pub p95_absolute: f64,
    pub max_absolute: f64,
    pub within_tolerance: u64,
    pub within_tolerance_rate: f64,
    pub tolerance: String,
}

/// One liquidity bucket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct StratumReport {
    pub stratum: String,
    pub observations: u64,
    pub false_exclusions: u64,
    pub false_exclusion_rate: f64,
    pub unknown_books: u64,
    pub unknown_rate: f64,
}

/// Book-depth and volume-cutoff breakdowns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Strata {
    pub book_depth: Vec<StratumReport>,
    pub volume_cutoff: Vec<StratumReport>,
}

/// Triggers the simulated policy cannot see. Diagnostic only: including these
/// would price a policy production cannot run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Diagnostics {
    pub oracle_note: String,
    pub oracle_gate_crossings: u64,
    pub oracle_subtype_changes: u64,
    pub oracle_extra_refreshes: u64,
}

/// Everything one interval produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct IntervalReport {
    pub label: String,
    pub interval_seconds: u64,
    pub identity: bool,
    pub calls: CallComparison,
    pub estimated_decoded_bytes: f64,
    pub bytes_are_estimates: bool,
    /// Refresh count per reason. Scheduled refreshes are driven by age; the
    /// rest are forced by evidence the policy can observe.
    pub refresh_reasons: BTreeMap<String, u64>,
    pub forced_refreshes: u64,
    pub false_exclusions: RateCount,
    pub unknown_books: UnknownBooks,
    pub recall: Recall,
    pub price_error: PriceError,
    pub clamped_low5_error: PriceError,
    pub production_score_relative_error: PriceError,
    pub strata: Strata,
    pub diagnostics: Diagnostics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Recall {
    /// Share of the fresh top-100 candidates the policy still had a usable book
    /// for, regardless of where they ranked.
    pub eligibility: RateCount,
    #[serde(rename = "top20")]
    pub top20: Overlap,
    #[serde(rename = "top100")]
    pub top100: Overlap,
    pub discovery_delay: DiscoveryDelay,
}

/// The request totals over the whole window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Totals {
    pub catalog_calls: u64,
    pub statistics_calls: u64,
    pub book_calls: u64,
    pub total_requests: u64,
    pub estimated_decoded_bytes: f64,
}

/// The report written to `--out` and summarised on stdout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub format: u32,
    pub logs_seen: u64,
    pub logs_read: u64,
    pub skipped_incomplete: u64,
    pub skipped_out_of_range: u64,
    pub skipped_malformed: Vec<SkippedLog>,
    pub min_volume: i64,
    pub schedule: Vec<String>,
    pub sweeps: Vec<SweepRef>,
    pub fresh: Totals,
    pub intervals: Vec<IntervalReport>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SweepRef {
    pub file: String,
    pub started_at: String,
    pub items: u64,
    pub kept: u64,
}

/// The CLI's parsed inputs.
#[derive(Debug, Clone)]
pub struct ReplayOptions {
    pub observations: PathBuf,
    pub schedule: Vec<Duration>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------- reader ----

/// Read a directory of observation logs in start order.
///
/// Name order is start order because the writer stamps the sweep's start into
/// the file name. Unknown formats and undateable headers are refused outright;
/// a log without a `Summary` is an incomplete sweep and is skipped, because its
/// rows are evidence but not a cross-section. A complete log whose row count
/// disagrees with its header is skipped too - it cannot be trusted as a
/// complete sweep.
pub fn read_directory(
    directory: &Path,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
) -> Result<Loaded, String> {
    let entries = std::fs::read_dir(directory)
        .map_err(|e| format!("read observation directory {}: {e}", directory.display()))?;
    let mut files: Vec<String> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("read observation directory: {e}"))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with("sweep-") && name.ends_with(".jsonl") {
            files.push(name.to_string());
        }
    }
    files.sort();

    let mut loaded = Loaded::default();
    for file in files {
        loaded.logs_seen += 1;
        let text = std::fs::read_to_string(directory.join(&file))
            .map_err(|e| format!("read {file}: {e}"))?;
        let mut run: Option<observations::Run> = None;
        let mut summary: Option<observations::Summary> = None;
        let mut items: BTreeMap<String, observations::Item> = BTreeMap::new();
        let mut item_rows: usize = 0;
        for (index, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let record: Record = serde_json::from_str(line)
                .map_err(|e| format!("parse {file}:{}: {e}", index + 1))?;
            match record {
                Record::Run(header) => {
                    if header.format != observations::FORMAT {
                        return Err(format!(
                            "{file}: observation format {} is not supported (this replay reads {})",
                            header.format,
                            observations::FORMAT
                        ));
                    }
                    run = Some(header);
                }
                Record::Item(item) => {
                    item_rows += 1;
                    items.insert(item.slug.clone(), item);
                }
                Record::Summary(totals) => summary = Some(totals),
            }
        }
        let Some(run) = run else {
            return Err(format!("{file}: no run header; cannot date or trust it"));
        };
        let Some(summary) = summary else {
            loaded.skipped_incomplete += 1;
            continue;
        };
        if item_rows != run.items || summary.scanned != run.items || items.len() != item_rows {
            loaded.skipped_malformed.push(SkippedLog {
                file,
                reason: format!(
                    "{item_rows} item rows, {} unique slugs, summary.scanned {}, header items {}; \
                     a complete sweep is one record per catalog item",
                    items.len(),
                    summary.scanned,
                    run.items
                ),
            });
            continue;
        }
        let Some(started_at) = crate::clock::parse_stamp(&run.started_at) else {
            return Err(format!(
                "{file}: unparseable start stamp {:?}",
                run.started_at
            ));
        };
        if from.is_some_and(|bound| started_at < bound) || to.is_some_and(|bound| started_at > bound)
        {
            loaded.skipped_out_of_range += 1;
            continue;
        }
        loaded.sweeps.push(Sweep {
            file,
            started_at,
            run,
            items,
        });
    }
    Ok(loaded)
}

/// Refuse a window whose sweeps were not collected under the same policy: the
/// volume gate decides eligibility, so mixing gates would compare schedules
/// that saw different catalogs.
fn validate_compatible(sweeps: &[Sweep]) -> Result<(), String> {
    let Some(first) = sweeps.first() else {
        return Ok(());
    };
    for sweep in sweeps.iter().skip(1) {
        let same = sweep.run.format == first.run.format
            && sweep.run.platform == first.run.platform
            && sweep.run.filter == first.run.filter
            && sweep.run.exclude == first.run.exclude
            && sweep.run.min_volume == first.run.min_volume;
        if !same {
            return Err(format!(
                "{} was collected under a different configuration than {} \
                 (platform/filter/exclude/min_volume must match across a replay)",
                sweep.file, first.file
            ));
        }
    }
    Ok(())
}

// ------------------------------------------------------------ arithmetic ----

fn estimated_bytes(statistics_calls: u64, book_calls: u64) -> f64 {
    statistics_calls as f64 * EST_BYTES_PER_STATISTICS + book_calls as f64 * EST_BYTES_PER_BOOK
}

fn rate(count: u64, total: u64) -> f64 {
    if total == 0 {
        1.0
    } else {
        count as f64 / total as f64
    }
}

fn priced(item: &observations::Item, low_sell: f64) -> PricedEntry {
    PricedEntry {
        vol: item.volume_48h,
        low_sell,
        avg: item.avg_price_48h,
        median_now: item.median_now,
        median_90d: item.median_90d,
    }
}

/// The production sell-priority score, at the candidate owner limit.
fn candidate_score(item: &observations::Item, low_sell: f64) -> f64 {
    sell_priority::score_row(CANDIDATE_OWNED, &priced(item, low_sell)).sell_score
}

/// The production clearing price, on the statistics the policy would have held.
fn candidate_price(item: &observations::Item, low_sell: f64) -> f64 {
    sell_priority::clearing_price(&priced(item, low_sell))
}

/// The three book-dependent quantities a production run would emit for the same
/// book under the given statistics: clearing price, clamped lowest-five average
/// and the raw composite score. Statistics figures arrive as the sweep's stored
/// outputs of `series_stats`/`weighted_avg_48h`; the raw daily rows those
/// functions need are not logged, so they cannot be re-run here and are not
/// re-implemented.
#[derive(Debug, Clone, Copy)]
struct BookMath {
    clearing: f64,
    low5: f64,
    score: f64,
}

fn book_math(item: &observations::Item, book: &observations::Book) -> BookMath {
    BookMath {
        clearing: candidate_price(item, book.low_sell_price),
        low5: clamp_low5(book.low5_avg_unclamped, item.median_90d, book.low_sell_price),
        score: score(item.volume_48h, item.avg_price_48h, book.buy_sell_ratio),
    }
}

fn ranked(rows: &[(String, f64)]) -> Vec<String> {
    let mut rows = rows.to_vec();
    rows.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    rows.into_iter().map(|(slug, _)| slug).collect()
}

/// Nearest-rank percentile over an already-sorted slice. Empty input is 0.0.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let n = sorted.len();
    let rank = ((p / 100.0) * n as f64).ceil();
    let index = (rank as usize)
        .saturating_sub(1)
        .min(n.saturating_sub(1));
    sorted.get(index).copied().unwrap_or(0.0)
}

fn error_distribution(
    mut errors: Vec<f64>,
    reference_total: u64,
    within: u64,
    tolerance: &str,
) -> PriceError {
    errors.sort_by(f64::total_cmp);
    let matched = errors.len() as u64;
    PriceError {
        reference: reference_total,
        matched,
        p50_absolute: percentile(&errors, 50.0),
        p95_absolute: percentile(&errors, 95.0),
        max_absolute: errors.last().copied().unwrap_or(0.0),
        within_tolerance: within,
        within_tolerance_rate: rate(within, matched),
        tolerance: tolerance.to_string(),
    }
}

// ----------------------------------------------------------- simulation -----

/// Which forced or scheduled event triggered a statistics refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefreshReason {
    /// No cached entry at all: first sight of the item.
    Bootstrap,
    /// The cached entry is a `NoStatistics` outcome, which is not usable.
    UnusableCache,
    /// The item was absent from the previous sweep and has reappeared.
    Reappeared,
    /// The last book actually fetched was empty, contradicting the cache.
    EmptyBook,
    /// The cached entry's age reached the interval.
    Scheduled,
}

impl RefreshReason {
    fn label(self) -> &'static str {
        match self {
            RefreshReason::Bootstrap => "bootstrap",
            RefreshReason::UnusableCache => "unusable_cache",
            RefreshReason::Reappeared => "reappeared",
            RefreshReason::EmptyBook => "empty_book",
            RefreshReason::Scheduled => "scheduled",
        }
    }
}

struct Cached {
    item: observations::Item,
    refreshed_at: DateTime<Utc>,
    empty_book: bool,
}

#[derive(Debug, Default, Clone)]
struct StratumAcc {
    observations: u64,
    false_exclusions: u64,
    unknown_books: u64,
}

#[derive(Default)]
struct IntervalAcc {
    statistics_calls: u64,
    book_calls: u64,
    refresh_reasons: BTreeMap<String, u64>,
    false_exclusions: u64,
    fresh_eligible_observations: u64,
    known_books: u64,
    unknown_books: u64,
    unknown_no_book: u64,
    unknown_subtype: u64,
    oracle_gate_crossings: u64,
    oracle_subtype_changes: u64,
    oracle_extra_refreshes: u64,
    depth_strata: BTreeMap<String, StratumAcc>,
    cutoff_strata: BTreeMap<String, StratumAcc>,
    price_errors: Vec<f64>,
    low5_errors: Vec<f64>,
    score_relative_errors: Vec<f64>,
    price_within: u64,
    low5_within: u64,
    price_reference: u64,
    price_matched: u64,
    top_reference: BTreeMap<usize, u64>,
    top_optimistic: BTreeMap<usize, u64>,
    top_pessimistic: BTreeMap<usize, u64>,
    ref_top: BTreeMap<usize, Vec<BTreeSet<String>>>,
    sim_top: BTreeMap<usize, Vec<BTreeSet<String>>>,
    /// Every candidate with a known book at each sweep, regardless of rank.
    /// Eligibility recall reads this; ranking recall reads `sim_top`.
    sim_known: Vec<BTreeSet<String>>,
    /// Sweep start stamps, so a discovery delay is reported in real wall time
    /// rather than an assumed cadence.
    sweep_stamps: Vec<DateTime<Utc>>,
}

/// Fresh (every-sweep) totals over the window: one catalog request per sweep,
/// one statistics request per attempted item, one book request per kept item.
pub fn fresh_totals(sweeps: &[Sweep]) -> Totals {
    let catalog_calls = sweeps.len() as u64;
    let statistics_calls: u64 = sweeps.iter().map(|s| s.items.len() as u64).sum();
    let book_calls: u64 = sweeps
        .iter()
        .map(|s| s.items.values().filter(|i| i.book.is_some()).count() as u64)
        .sum();
    Totals {
        catalog_calls,
        statistics_calls,
        book_calls,
        total_requests: catalog_calls + statistics_calls + book_calls,
        estimated_decoded_bytes: estimated_bytes(statistics_calls, book_calls),
    }
}

/// Simulate every requested interval against the same logs.
pub fn simulate(sweeps: &[Sweep], schedule: &[Duration], min_volume: i64) -> Vec<IntervalReport> {
    let fresh = fresh_totals(sweeps);
    schedule
        .iter()
        .map(|interval| simulate_interval(sweeps, *interval, min_volume as f64, &fresh))
        .collect()
}

fn simulate_interval(
    sweeps: &[Sweep],
    interval: Duration,
    min_volume: f64,
    fresh: &Totals,
) -> IntervalReport {
    let interval_seconds = interval.as_secs();
    let mut acc = IntervalAcc::default();
    let mut cache: BTreeMap<String, Cached> = BTreeMap::new();
    let mut present_prev: BTreeSet<String> = BTreeSet::new();

    for sweep in sweeps {
        acc.sweep_stamps.push(sweep.started_at);
        let mut ref_rows: Vec<(String, f64)> = Vec::new();
        let mut sim_rows: Vec<(String, f64)> = Vec::new();
        let mut ref_math: BTreeMap<String, BookMath> = BTreeMap::new();
        let mut sim_math: BTreeMap<String, BookMath> = BTreeMap::new();
        let mut unknown_slugs: BTreeSet<String> = BTreeSet::new();

        for (slug, fresh_item) in &sweep.items {
            let had_cache = cache.contains_key(slug);
            // The pre-refresh entry drives the refresh decision only; every
            // comparison below reads the cache the policy actually holds after
            // any refresh, or a bootstrap would look like a subtype change.
            let prior_outcome = cache.get(slug).map(|c| c.item.outcome);
            let prior_empty = cache.get(slug).is_some_and(|c| c.empty_book);
            let aged_out = cache.get(slug).is_some_and(|c| {
                sweep.started_at.signed_duration_since(c.refreshed_at).num_seconds()
                    >= interval_seconds as i64
            });

            let reason = match prior_outcome {
                None => Some(RefreshReason::Bootstrap),
                Some(Outcome::NoStatistics) => Some(RefreshReason::UnusableCache),
                Some(_) if !present_prev.contains(slug) => Some(RefreshReason::Reappeared),
                Some(_) if prior_empty => Some(RefreshReason::EmptyBook),
                Some(_) if aged_out => Some(RefreshReason::Scheduled),
                Some(_) => None,
            };
            if let Some(reason) = reason {
                acc.statistics_calls += 1;
                *acc
                    .refresh_reasons
                    .entry(reason.label().to_string())
                    .or_insert(0) += 1;
                cache.insert(
                    slug.clone(),
                    Cached {
                        item: fresh_item.clone(),
                        refreshed_at: sweep.started_at,
                        empty_book: false,
                    },
                );
            }

            let cached_subtype = cache.get(slug).and_then(|c| c.item.subtype.clone());
            let cached_eligible = cache.get(slug).is_some_and(|c| {
                c.item.outcome != Outcome::NoStatistics && c.item.volume_48h >= min_volume
            });
            let fresh_eligible = fresh_item.outcome == Outcome::Kept;

            // An oracle-only trigger: only fresh statistics reveal it, so it is
            // a bound, never a simulated refresh.
            if reason.is_none() && had_cache {
                if cached_eligible != fresh_eligible {
                    acc.oracle_gate_crossings += 1;
                    acc.oracle_extra_refreshes += 1;
                }
                if fresh_eligible && cached_subtype != fresh_item.subtype {
                    acc.oracle_subtype_changes += 1;
                }
            }

            let known_here = if cached_eligible {
                // Eligibility comes from the cache, so this is a book the
                // policy would fetch whether or not the fresh sweep did.
                acc.book_calls += 1;
                let same_subtype = cached_subtype == fresh_item.subtype;
                let known = fresh_item.outcome == Outcome::Kept
                    && fresh_item.book.is_some()
                    && same_subtype;
                if known {
                    acc.known_books += 1;
                    if let (Some(c), Some(book)) = (cache.get(slug), fresh_item.book.as_ref()) {
                        sim_rows.push((slug.clone(), candidate_score(&c.item, book.low_sell_price)));
                        sim_math.insert(slug.clone(), book_math(&c.item, book));
                        // The response was actually fetched, so its emptiness is
                        // observable evidence for the next sweep's trigger.
                        if let Some(entry) = cache.get_mut(slug) {
                            entry.empty_book = book.live_buys == 0 && book.live_sells == 0;
                        }
                    }
                    true
                } else {
                    acc.unknown_books += 1;
                    unknown_slugs.insert(slug.clone());
                    if fresh_item.outcome != Outcome::Kept {
                        acc.unknown_no_book += 1;
                    } else {
                        acc.unknown_subtype += 1;
                    }
                    false
                }
            } else {
                if fresh_eligible {
                    acc.false_exclusions += 1;
                }
                false
            };

            if fresh_eligible {
                acc.fresh_eligible_observations += 1;
                if let Some(book) = fresh_item.book.as_ref() {
                    ref_rows.push((slug.clone(), candidate_score(fresh_item, book.low_sell_price)));
                    ref_math.insert(slug.clone(), book_math(fresh_item, book));
                    let depth = book.live_buys + book.live_sells;
                    let bucket = if depth < 5 {
                        "thin"
                    } else if depth <= 20 {
                        "medium"
                    } else {
                        "deep"
                    };
                    let entry = acc.depth_strata.entry(bucket.to_string()).or_default();
                    entry.observations += 1;
                    entry.false_exclusions += u64::from(!cached_eligible);
                    entry.unknown_books += u64::from(cached_eligible && !known_here);

                    let near = min_volume > 0.0 && fresh_item.volume_48h < 2.0 * min_volume;
                    let cutoff = if near { "near_cutoff" } else { "far_from_cutoff" };
                    let entry = acc.cutoff_strata.entry(cutoff.to_string()).or_default();
                    entry.observations += 1;
                    entry.false_exclusions += u64::from(!cached_eligible);
                    entry.unknown_books += u64::from(cached_eligible && !known_here);
                }
            }
        }

        let known_slugs: BTreeSet<String> = sim_math.keys().cloned().collect();
        accumulate_ranking(
            &mut acc,
            &ref_rows,
            &sim_rows,
            unknown_slugs.len() as u64,
            &known_slugs,
        );
        accumulate_errors(
            &mut acc,
            &ref_rows,
            &ref_math,
            &sim_math,
        );
        present_prev = sweep.items.keys().cloned().collect();
    }

    finish_interval(acc, interval_seconds, fresh)
}

/// Overlap at each depth, plus the per-sweep sets the discovery delay needs.
fn accumulate_ranking(
    acc: &mut IntervalAcc,
    ref_rows: &[(String, f64)],
    sim_rows: &[(String, f64)],
    unknown_count: u64,
    known: &BTreeSet<String>,
) {
    let ref_ranked = ranked(ref_rows);
    let sim_ranked = ranked(sim_rows);
    acc.sim_known.push(known.clone());
    for k in TOP_K {
        let reference: BTreeSet<String> = ref_ranked.iter().take(k).cloned().collect();
        let optimistic: BTreeSet<String> = sim_ranked.iter().take(k).cloned().collect();
        let pessimistic_slots = k.saturating_sub(unknown_count as usize);
        let pessimistic: BTreeSet<String> =
            sim_ranked.iter().take(pessimistic_slots).cloned().collect();
        *acc.top_reference.entry(k).or_insert(0) += reference.len() as u64;
        *acc.top_optimistic.entry(k).or_insert(0) +=
            reference.intersection(&optimistic).count() as u64;
        *acc.top_pessimistic.entry(k).or_insert(0) +=
            reference.intersection(&pessimistic).count() as u64;
        acc.ref_top.entry(k).or_default().push(reference);
        acc.sim_top.entry(k).or_default().push(optimistic);
    }
}

/// Price, clamp and score error on the fresh top-100 candidates that still have
/// a known simulated result.
fn accumulate_errors(
    acc: &mut IntervalAcc,
    ref_rows: &[(String, f64)],
    ref_math: &BTreeMap<String, BookMath>,
    sim_math: &BTreeMap<String, BookMath>,
) {
    let top: Vec<String> = ranked(ref_rows).into_iter().take(100).collect();
    acc.price_reference += top.len() as u64;
    for slug in &top {
        let (Some(reference), Some(simulated)) = (ref_math.get(slug), sim_math.get(slug)) else {
            continue;
        };
        acc.price_matched += 1;
        let price_error = (simulated.clearing - reference.clearing).abs();
        let price_tolerance = 1.0f64.max(0.05 * reference.clearing.abs());
        if price_error <= price_tolerance {
            acc.price_within += 1;
        }
        acc.price_errors.push(price_error);

        let low5_error = (simulated.low5 - reference.low5).abs();
        let low5_tolerance = 1.0f64.max(0.05 * reference.low5.abs());
        if low5_error <= low5_tolerance {
            acc.low5_within += 1;
        }
        acc.low5_errors.push(low5_error);

        acc.score_relative_errors
            .push((simulated.score - reference.score).abs() / reference.score.abs().max(1.0));
    }
}

fn finish_interval(
    acc: IntervalAcc,
    interval_seconds: u64,
    fresh: &Totals,
) -> IntervalReport {
    let catalog_calls = fresh.catalog_calls;
    let total_requests = catalog_calls + acc.statistics_calls + acc.book_calls;
    let saving_requests = fresh.total_requests as i64 - total_requests as i64;
    let saving_ratio = if fresh.total_requests == 0 {
        0.0
    } else {
        saving_requests as f64 / fresh.total_requests as f64
    };
    let forced_refreshes = acc
        .refresh_reasons
        .iter()
        .filter(|(label, _)| label.as_str() != RefreshReason::Scheduled.label())
        .map(|(_, count)| *count)
        .sum();

    let calls = CallComparison {
        statistics_calls: acc.statistics_calls,
        book_calls: acc.book_calls,
        total_requests,
        fresh_statistics_calls: fresh.statistics_calls,
        fresh_book_calls: fresh.book_calls,
        fresh_total_requests: fresh.total_requests,
        saving_requests,
        saving_ratio,
    };

    let overlap = |k: usize| {
        let reference = acc.top_reference.get(&k).copied().unwrap_or(0);
        let matched_optimistic = acc.top_optimistic.get(&k).copied().unwrap_or(0);
        let matched_pessimistic = acc.top_pessimistic.get(&k).copied().unwrap_or(0);
        Overlap {
            reference,
            matched_optimistic,
            recall_optimistic: rate(matched_optimistic, reference),
            matched_pessimistic,
            recall_pessimistic: rate(matched_pessimistic, reference),
        }
    };

    let discipline = discovery_delay(&acc);
    let eligibility = eligibility_recall(&acc);
    let top20 = overlap(20);
    let top100 = overlap(100);
    let identity = acc.statistics_calls == fresh.statistics_calls
        && acc.book_calls == fresh.book_calls;

    IntervalReport {
        label: interval_label(interval_seconds),
        interval_seconds,
        identity,
        calls,
        estimated_decoded_bytes: estimated_bytes(acc.statistics_calls, acc.book_calls),
        bytes_are_estimates: true,
        refresh_reasons: acc.refresh_reasons,
        forced_refreshes,
        false_exclusions: RateCount {
            count: acc.false_exclusions,
            total: acc.fresh_eligible_observations,
            rate: rate(acc.false_exclusions, acc.fresh_eligible_observations),
        },
        unknown_books: UnknownBooks {
            known: acc.known_books,
            unknown: acc.unknown_books,
            coverage: rate(acc.known_books, acc.known_books + acc.unknown_books),
            unknown_no_fresh_book: acc.unknown_no_book,
            unknown_subtype_change: acc.unknown_subtype,
        },
        recall: Recall {
            eligibility,
            top20,
            top100,
            discovery_delay: discipline,
        },
        price_error: error_distribution(
            acc.price_errors,
            acc.price_reference,
            acc.price_within,
            "absolute error vs max(1p, 5% of reference)",
        ),
        clamped_low5_error: error_distribution(
            acc.low5_errors,
            acc.price_reference,
            acc.low5_within,
            "absolute error vs max(1p, 5% of reference)",
        ),
        production_score_relative_error: error_distribution(
            acc.score_relative_errors,
            acc.price_reference,
            0,
            "relative error |sim - ref| / max(|ref|, 1); diagnostic, no pass threshold",
        ),
        strata: Strata {
            book_depth: strata_rows(acc.depth_strata),
            volume_cutoff: strata_rows(acc.cutoff_strata),
        },
        diagnostics: Diagnostics {
            oracle_note: "diagnostic bound only - production cannot observe these \
                          triggers without fetching fresh statistics, so they are \
                          excluded from statistics_calls and every headline metric"
                .to_string(),
            oracle_gate_crossings: acc.oracle_gate_crossings,
            oracle_subtype_changes: acc.oracle_subtype_changes,
            oracle_extra_refreshes: acc.oracle_extra_refreshes,
        },
    }
}

/// Share of the fresh top-100 candidates the policy still held a usable book
/// for, regardless of where a stale score ranked them. This is deliberately not
/// derived from the ranking overlap: a known candidate pushed out of the top
/// list was seen, not missed.
fn eligibility_recall(acc: &IntervalAcc) -> RateCount {
    let mut count = 0u64;
    let mut total = 0u64;
    if let Some(reference_sets) = acc.ref_top.get(&100) {
        for (index, reference) in reference_sets.iter().enumerate() {
            total += reference.len() as u64;
            if let Some(known) = acc.sim_known.get(index) {
                count += reference.intersection(known).count() as u64;
            }
        }
    }
    RateCount {
        count,
        total,
        rate: rate(count, total),
    }
}

fn strata_rows(acc: BTreeMap<String, StratumAcc>) -> Vec<StratumReport> {    acc.into_iter()
        .map(|(stratum, a)| StratumReport {
            stratum,
            observations: a.observations,
            false_exclusions: a.false_exclusions,
            false_exclusion_rate: rate(a.false_exclusions, a.observations),
            unknown_books: a.unknown_books,
            unknown_rate: rate(a.unknown_books, a.observations),
        })
        .collect()
}

/// For every fresh top-100 candidate absent from the simulated top-100 at its
/// own sweep, how many further sweeps until it appears - and how long that is
/// in wall time, which is what a 2-hourly pipeline actually experiences. A
/// candidate that never reappears in the window is censored, not zero-delay.
fn discovery_delay(acc: &IntervalAcc) -> DiscoveryDelay {
    let empty = Vec::new();
    let ref_top = acc.ref_top.get(&100).unwrap_or(&empty);
    let sim_top = acc.sim_top.get(&100).unwrap_or(&empty);
    let mut sweeps_delays: Vec<f64> = Vec::new();
    let mut second_delays: Vec<f64> = Vec::new();
    let mut censored = 0u64;
    let mut missing = 0u64;
    let mut reference_candidates = 0u64;
    let mut beyond = 0u64;

    for (index, reference) in ref_top.iter().enumerate() {
        reference_candidates += reference.len() as u64;
        for slug in reference {
            if sim_top.get(index).is_some_and(|set| set.contains(slug)) {
                continue;
            }
            missing += 1;
            let mut found: Option<usize> = None;
            for (offset, future) in sim_top.iter().enumerate().skip(index + 1) {
                if future.contains(slug) {
                    found = Some(offset - index);
                    break;
                }
            }
            match found {
                Some(delay) => {
                    sweeps_delays.push(delay as f64);
                    second_delays.push(wall_seconds(acc, index, delay));
                    if delay > 1 {
                        beyond += 1;
                    }
                }
                None => {
                    censored += 1;
                    beyond += 1;
                }
            }
        }
    }

    sweeps_delays.sort_by(f64::total_cmp);
    second_delays.sort_by(f64::total_cmp);
    let observed = sweeps_delays.len() as u64;
    DiscoveryDelay {
        reference_candidates,
        missing_at_their_sweep: missing,
        observed,
        censored,
        p50_sweeps: percentile(&sweeps_delays, 50.0),
        p95_sweeps: percentile(&sweeps_delays, 95.0),
        max_sweeps: sweeps_delays.last().copied().unwrap_or(0.0),
        p95_seconds: percentile(&second_delays, 95.0),
        beyond_one_sweep: beyond,
        beyond_one_sweep_rate: rate(beyond, reference_candidates),
    }
}

/// Wall seconds between two sweeps `delay` apart, read from the sweep stamps
/// rather than assumed to be a fixed cadence.
fn wall_seconds(acc: &IntervalAcc, index: usize, delay: usize) -> f64 {
    let stamps = acc.sweep_stamps.get(index).zip(
        acc.sweep_stamps
            .get(index + delay),
    );
    match stamps {
        Some((start, end)) => end.signed_duration_since(*start).num_seconds() as f64,
        None => 0.0,
    }
}

// ------------------------------------------------------------------ output ---

fn interval_label(seconds: u64) -> String {
    for (size, suffix) in [(86_400u64, "d"), (3_600, "h"), (60, "m")] {
        let count = seconds / size;
        if count > 0 && count * size == seconds {
            return format!("{count}{suffix}");
        }
    }
    format!("{seconds}s")
}

/// Parse a comma-separated schedule such as `4h,6h,12h,24h`.
pub fn parse_schedule(spec: &str) -> Result<Vec<Duration>, String> {
    let mut out: Vec<Duration> = Vec::new();
    for token in spec.split(',') {
        let token = token.trim();
        if token.is_empty() {
            return Err(format!("empty interval in --schedule {spec:?}"));
        }
        let interval = parse_interval(token)?;
        if !out.contains(&interval) {
            out.push(interval);
        }
    }
    if out.is_empty() {
        return Err("--schedule is empty".to_string());
    }
    Ok(out)
}

fn parse_interval(token: &str) -> Result<Duration, String> {
    let digits: String = token.chars().take_while(char::is_ascii_digit).collect();
    let unit: String = token.chars().skip_while(char::is_ascii_digit).collect();
    let value: u64 = digits
        .parse()
        .map_err(|_| format!("invalid interval {token:?}"))?;
    let multiplier: u64 = match unit.as_str() {
        "" | "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        other => return Err(format!("unknown interval unit {other:?} in {token:?}")),
    };
    if value == 0 {
        return Err(format!("interval {token:?} must be greater than zero"));
    }
    let seconds = value
        .checked_mul(multiplier)
        .ok_or_else(|| format!("interval {token:?} overflows"))?;
    Ok(Duration::from_secs(seconds))
}

/// Parse a `--from`/`--to` bound: a bare date, or a full `...Z` stamp. A bare
/// `--to` date covers the whole day, which is what a human means by "through
/// the 15th".
pub fn parse_bound(text: &str, end_of_day: bool) -> Result<DateTime<Utc>, String> {
    if let Some(stamp) = crate::clock::parse_stamp(text) {
        return Ok(stamp);
    }
    let date = NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .map_err(|_| format!("invalid date {text:?} (expected YYYY-MM-DD or YYYY-MM-DDTHH:MM:SSZ)"))?;
    let (hour, minute, second) = if end_of_day { (23, 59, 59) } else { (0, 0, 0) };
    let naive = date
        .and_hms_opt(hour, minute, second)
        .ok_or_else(|| format!("invalid date {text:?}"))?;
    Ok(Utc.from_utc_datetime(&naive))
}

/// Read the logs and produce the report.
pub fn run(options: &ReplayOptions) -> Result<Report, String> {
    let loaded = read_directory(&options.observations, options.from, options.to)?;
    if loaded.sweeps.is_empty() {
        return Err(format!(
            "no complete observation logs in {} ({} seen, {} incomplete, {} out of range, {} malformed)",
            options.observations.display(),
            loaded.logs_seen,
            loaded.skipped_incomplete,
            loaded.skipped_out_of_range,
            loaded.skipped_malformed.len()
        ));
    }
    validate_compatible(&loaded.sweeps)?;
    let min_volume = loaded.sweeps.first().map(|s| s.run.min_volume).unwrap_or(0);
    let fresh = fresh_totals(&loaded.sweeps);
    let intervals = options
        .schedule
        .iter()
        .map(|interval| simulate_interval(&loaded.sweeps, *interval, min_volume as f64, &fresh))
        .collect();
    let sweeps = loaded
        .sweeps
        .iter()
        .map(|sweep| SweepRef {
            file: sweep.file.clone(),
            started_at: crate::clock::iso_z(sweep.started_at),
            items: sweep.items.len() as u64,
            kept: sweep
                .items
                .values()
                .filter(|i| i.book.is_some())
                .count() as u64,
        })
        .collect();
    Ok(Report {
        format: REPORT_FORMAT,
        logs_seen: loaded.logs_seen,
        logs_read: loaded.sweeps.len() as u64,
        skipped_incomplete: loaded.skipped_incomplete,
        skipped_out_of_range: loaded.skipped_out_of_range,
        skipped_malformed: loaded.skipped_malformed,
        min_volume,
        schedule: options
            .schedule
            .iter()
            .map(|d| interval_label(d.as_secs()))
            .collect(),
        sweeps,
        fresh,
        intervals,
        notes: vec![
            "decoded bytes are ESTIMATES from the documented per-request constants; \
             the observation logs carry no per-item byte counter"
                .to_string(),
            "candidates are ranked with the production sell-priority score at its owner \
             limit (owned = inf => units_today = daily_sales); the logs carry no inventory"
                .to_string(),
            "oracle_* is a diagnostic bound only and is excluded from every headline number"
                .to_string(),
            "series_stats and weighted_avg_48h outputs are consumed as the sweep stored \
             them; the raw daily rows they need are not logged, so they are not re-run"
                .to_string(),
            "cache age is measured from each sweep's start stamp; the logs carry no \
             per-item fetch time, so a real cache entry is at most one sweep younger and \
             the simulated schedule refreshes at least as often as production would"
                .to_string(),
            "discovery delay reads the optimistic known top-100; the pessimistic ranking \
             overlap is the conservative bound for the same candidates"
                .to_string(),
        ],
    })
}

/// A short human-readable summary for stdout.
pub fn summary(report: &Report) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "replay: read {} of {} logs ({} incomplete, {} out of range, {} malformed), \
         {} sweeps, min_volume {}",
        report.logs_read,
        report.logs_seen,
        report.skipped_incomplete,
        report.skipped_out_of_range,
        report.skipped_malformed.len(),
        report.sweeps.len(),
        report.min_volume,
    );
    let _ = writeln!(
        out,
        "fresh: catalog {} + statistics {} + books {} = {} requests, ~{:.1} MB decoded (estimate)",
        report.fresh.catalog_calls,
        report.fresh.statistics_calls,
        report.fresh.book_calls,
        report.fresh.total_requests,
        report.fresh.estimated_decoded_bytes / (1024.0 * 1024.0),
    );
    for interval in &report.intervals {
        let _ = writeln!(
            out,
            "{:>4}: statistics {}/{} books {}/{} total {}/{} saving {} ({:+.1}%) | \
             false excl {}/{} ({:.3}%) | unknown {}/{} coverage {:.3} | \
             top100 recall {:.4}/{:.4} | price p95 {:.2} within {:.4} | delay p95 {} sweeps {} censored",
            interval.label,
            interval.calls.statistics_calls,
            interval.calls.fresh_statistics_calls,
            interval.calls.book_calls,
            interval.calls.fresh_book_calls,
            interval.calls.total_requests,
            interval.calls.fresh_total_requests,
            interval.calls.saving_requests,
            interval.calls.saving_ratio * 100.0,
            interval.false_exclusions.count,
            interval.false_exclusions.total,
            interval.false_exclusions.rate * 100.0,
            interval.unknown_books.unknown,
            interval.unknown_books.known + interval.unknown_books.unknown,
            interval.unknown_books.coverage,
            interval.recall.top100.recall_optimistic,
            interval.recall.top100.recall_pessimistic,
            interval.price_error.p95_absolute,
            interval.price_error.within_tolerance_rate,
            interval.recall.discovery_delay.p95_sweeps,
            interval.recall.discovery_delay.censored,
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observations::{Book, Run, Summary};

    fn stamp(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 15, hour, 0, 0).unwrap()
    }

    fn book(buys: i64, sells: i64, low: f64, low5: f64) -> Book {
        Book {
            live_buys: buys,
            live_sells: sells,
            buy_sell_ratio: if sells > 0 {
                buys as f64 / sells as f64
            } else {
                buys as f64 * 10.0
            },
            top_buy_price: if buys > 0 { low * 0.8 } else { 0.0 },
            low_sell_price: low,
            low5_avg_unclamped: low5,
            score: 0.0,
        }
    }

    fn item(
        slug: &str,
        outcome: Outcome,
        volume: f64,
        subtype: Option<&str>,
        book: Option<Book>,
    ) -> observations::Item {
        observations::Item {
            slug: slug.to_string(),
            name: slug.to_string(),
            tags: vec![],
            ducats: None,
            outcome,
            subtype: subtype.map(str::to_string),
            volume_48h: volume,
            median_now: 30.0,
            median_90d: 31.0,
            medians_7d: vec![30.0, 31.0],
            avg_price_48h: volume * 1.5,
            donch_top_90d: 40.0,
            donch_bot_90d: 20.0,
            book,
        }
    }

    fn kept(slug: &str, volume: f64, subtype: Option<&str>, book: Book) -> observations::Item {
        item(slug, Outcome::Kept, volume, subtype, Some(book))
    }

    fn sweep(hour: u32, items: Vec<observations::Item>) -> Sweep {
        let items: BTreeMap<String, observations::Item> =
            items.into_iter().map(|i| (i.slug.clone(), i)).collect();
        Sweep {
            file: format!("sweep-2026-09-15T{hour:02}-00-00Z.jsonl"),
            started_at: stamp(hour),
            run: Run {
                format: observations::FORMAT,
                version: "test".to_string(),
                platform: "pc".to_string(),
                filter: "prime".to_string(),
                exclude: "set".to_string(),
                min_volume: 5,
                items: items.len(),
                workers: 2,
                started_at: crate::clock::iso_z(stamp(hour)),
            },
            items,
        }
    }

    fn simulate_one(sweeps: &[Sweep], interval: Duration) -> IntervalReport {
        let fresh = fresh_totals(sweeps);
        simulate_interval(sweeps, interval, 5.0, &fresh)
    }

    fn hours(n: u64) -> Duration {
        Duration::from_secs(n * 3_600)
    }

    /// The baseline the whole exercise rests on: at or below the sweep cadence
    /// every sweep refreshes, so the simulation IS the fresh run.
    #[test]
    fn identity_schedule_reproduces_the_fresh_result_exactly() {
        let sweeps = vec![
            sweep(
                0,
                vec![
                    kept("alpha", 20.0, None, book(3, 4, 30.0, 29.0)),
                    item("beta", Outcome::BelowVolume, 1.0, None, None),
                    kept("gamma", 50.0, Some("intact"), book(10, 12, 5.0, 4.5)),
                ],
            ),
            sweep(
                2,
                vec![
                    kept("alpha", 21.0, None, book(3, 4, 31.0, 30.0)),
                    item("beta", Outcome::BelowVolume, 1.0, None, None),
                    kept("gamma", 52.0, Some("intact"), book(10, 12, 6.0, 5.5)),
                ],
            ),
            sweep(
                4,
                vec![
                    kept("alpha", 22.0, None, book(3, 4, 32.0, 31.0)),
                    item("beta", Outcome::BelowVolume, 1.0, None, None),
                    kept("gamma", 54.0, Some("intact"), book(10, 12, 7.0, 6.5)),
                ],
            ),
        ];
        let fresh = fresh_totals(&sweeps);
        for interval in [hours(1), hours(2)] {
            let report = simulate_one(&sweeps, interval);
            assert!(report.identity, "{interval:?} should be the identity");
            assert_eq!(report.calls.statistics_calls, fresh.statistics_calls);
            assert_eq!(report.calls.book_calls, fresh.book_calls);
            assert_eq!(report.calls.total_requests, fresh.total_requests);
            assert_eq!(report.calls.saving_requests, 0);
            assert_eq!(report.false_exclusions.count, 0);
            assert_eq!(report.unknown_books.unknown, 0);
            assert_eq!(report.unknown_books.coverage, 1.0);
            assert_eq!(report.recall.top100.recall_optimistic, 1.0);
            assert_eq!(report.recall.top100.recall_pessimistic, 1.0);
            assert_eq!(report.recall.eligibility.rate, 1.0);
            assert_eq!(report.price_error.max_absolute, 0.0);
            assert_eq!(report.clamped_low5_error.max_absolute, 0.0);
            assert_eq!(report.production_score_relative_error.max_absolute, 0.0);
            assert_eq!(report.recall.discovery_delay.missing_at_their_sweep, 0);
        }
    }

    /// Eligibility recall is a different claim from ranking recall: a candidate
    /// whose book is still usable has not been missed, even if a stale score
    /// pushed it out of the simulated top list.
    #[test]
    fn eligibility_recall_is_separate_from_ranking_recall() {
        // 101 candidates so the top-100 cap bites. `flip` is valuable only in
        // the cache - it collapses in the fresh sweep - so at the second sweep
        // its stale score takes a top-100 slot from the weakest fresh
        // candidate, which still has a usable book of its own.
        let mut s0_items = Vec::new();
        let mut s1_items = Vec::new();
        for i in 0..100 {
            let slug = format!("item{i:03}");
            let vol = 20.0 + i as f64;
            let price = 10.0 + i as f64;
            s0_items.push(kept(&slug, vol, None, book(4, 4, price, price)));
            s1_items.push(kept(&slug, vol, None, book(4, 4, price, price)));
        }
        s0_items.push(kept("flip", 1000.0, None, book(4, 4, 500.0, 500.0)));
        s1_items.push(kept("flip", 5.0, None, book(4, 4, 1.0, 1.0)));
        let sweeps = vec![sweep(0, s0_items), sweep(2, s1_items)];
        let report = simulate_one(&sweeps, hours(24));

        assert_eq!(report.recall.top100.reference, 200);
        assert_eq!(
            report.recall.top100.matched_optimistic, 199,
            "one fresh top-100 candidate is ranked out by a stale score"
        );
        assert_eq!(report.recall.eligibility.total, 200);
        assert_eq!(
            report.recall.eligibility.count, 200,
            "every fresh top-100 candidate still had a usable book"
        );
        assert!(
            report.recall.eligibility.rate > report.recall.top100.recall_optimistic,
            "eligibility and ranking must not be the same number"
        );
    }

    /// Fresh-data leakage is the failure that invalidates the measurement: the
    /// item is worthless in the cache, becomes valuable, and the simulation must
    /// stay blind to it until the schedule catches up - even though the fresh
    /// row for that same sweep says otherwise.
    #[test]
    fn a_below_gate_item_is_not_rescued_by_fresh_statistics() {
        let sweeps = vec![
            sweep(0, vec![item("sleeper", Outcome::BelowVolume, 1.0, None, None)]),
            sweep(1, vec![kept("sleeper", 1000.0, None, book(5, 5, 100.0, 99.0))]),
            sweep(2, vec![kept("sleeper", 1000.0, None, book(5, 5, 100.0, 99.0))]),
        ];
        let report = simulate_one(&sweeps, hours(24));

        assert_eq!(report.calls.statistics_calls, 1, "only the bootstrap");
        assert_eq!(report.calls.book_calls, 0, "the policy never believed it eligible");
        assert_eq!(report.false_exclusions.count, 2, "both fresh-eligible sweeps missed");
        assert_eq!(report.false_exclusions.total, 2);
        assert_eq!(report.unknown_books.unknown, 0, "never a stale inclusion");
        assert_eq!(report.recall.top100.reference, 2);
        assert_eq!(report.recall.top100.matched_optimistic, 0);
        assert_eq!(report.recall.discovery_delay.censored, 2);
        // The oracle sees the crossing; the headline must not.
        assert_eq!(report.diagnostics.oracle_gate_crossings, 2);
        assert_eq!(report.diagnostics.oracle_extra_refreshes, 2);
        assert!(
            report.calls.statistics_calls < 1 + report.diagnostics.oracle_extra_refreshes,
            "oracle refreshes are not simulated"
        );
    }

    /// An OBSERVABLE event - here the cached state itself being unusable - does
    /// reveal the item early, and that forced call is charged to the schedule.
    #[test]
    fn an_observable_cached_state_reveals_the_item_early() {
        let sweeps = vec![
            sweep(0, vec![item("riser", Outcome::NoStatistics, 0.0, None, None)]),
            sweep(1, vec![kept("riser", 100.0, None, book(4, 6, 40.0, 39.0))]),
        ];
        let report = simulate_one(&sweeps, hours(24));

        assert_eq!(report.refresh_reasons.get("unusable_cache"), Some(&1));
        assert_eq!(report.calls.statistics_calls, 2, "the forced call is charged");
        assert_eq!(
            report.forced_refreshes, 2,
            "first sight plus the unusable cache entry"
        );
        assert_eq!(report.calls.book_calls, 1);
        assert_eq!(report.false_exclusions.count, 0);
        assert_eq!(report.unknown_books.unknown, 0);
        assert_eq!(report.unknown_books.coverage, 1.0);
    }

    /// A response the pipeline actually fetched is observable evidence: an empty
    /// book next to a non-empty cache forces the next sweep to re-read the stats.
    #[test]
    fn an_empty_fetched_book_forces_a_refresh() {
        let sweeps = vec![
            sweep(0, vec![kept("flicker", 50.0, None, book(0, 0, 0.0, 0.0))]),
            sweep(1, vec![kept("flicker", 50.0, None, book(0, 0, 0.0, 0.0))]),
        ];
        let report = simulate_one(&sweeps, hours(24));

        assert_eq!(report.refresh_reasons.get("empty_book"), Some(&1));
        assert_eq!(report.calls.statistics_calls, 2);
        assert_eq!(
            report.forced_refreshes, 2,
            "first sight plus the empty book the sweep actually fetched"
        );
    }

    /// Catalog churn is observable too: an item that left and returned is not
    /// trusted to the cache it held before the absence.
    #[test]
    fn a_catalog_reappearance_forces_a_refresh() {
        let sweeps = vec![
            sweep(
                0,
                vec![
                    kept("blip", 30.0, None, book(3, 3, 20.0, 19.0)),
                    kept("other", 10.0, None, book(2, 2, 5.0, 4.0)),
                ],
            ),
            sweep(1, vec![kept("other", 10.0, None, book(2, 2, 5.0, 4.0))]),
            sweep(
                2,
                vec![
                    kept("blip", 30.0, None, book(3, 3, 20.0, 19.0)),
                    kept("other", 10.0, None, book(2, 2, 5.0, 4.0)),
                ],
            ),
        ];
        let report = simulate_one(&sweeps, hours(24));

        assert_eq!(report.refresh_reasons.get("reappeared"), Some(&1));
        assert_eq!(
            report.calls.statistics_calls, 3,
            "two first sights, then the reappearance; `other` is never re-read"
        );
        assert_eq!(report.forced_refreshes, 3);
    }

    /// Subtype-selected books cannot be applied to a different subtype. The
    /// stale inclusion is UNKNOWN - not a zero, not a miss.
    #[test]
    fn a_subtype_change_makes_the_book_unknown() {
        let sweeps = vec![
            sweep(0, vec![kept("relic", 100.0, Some("intact"), book(5, 5, 10.0, 9.0))]),
            sweep(1, vec![kept("relic", 100.0, Some("radiant"), book(5, 5, 50.0, 49.0))]),
        ];
        let report = simulate_one(&sweeps, hours(24));

        assert_eq!(report.calls.book_calls, 2, "the stale inclusion still cost a book");
        assert_eq!(report.unknown_books.unknown, 1);
        assert_eq!(report.unknown_books.known, 1, "the bootstrap sweep was known");
        assert_eq!(report.unknown_books.unknown_subtype_change, 1);
        assert_eq!(report.unknown_books.unknown_no_fresh_book, 0);
        assert_eq!(report.unknown_books.coverage, 0.5);
        assert_eq!(report.recall.top100.reference, 2);
        assert_eq!(
            report.recall.top100.matched_optimistic, 1,
            "only the bootstrap sweep's book is usable"
        );
        assert_eq!(report.recall.eligibility.rate, 0.5);
    }

    /// The stale-positive case the plan calls out: the cache says eligible, the
    /// fresh sweep found no book. The result is unknown, and the policy still
    /// paid for the book request it could not evaluate.
    #[test]
    fn a_stale_positive_without_a_book_is_unknown_not_zero() {
        let sweeps = vec![
            sweep(0, vec![kept("ghost", 100.0, None, book(2, 2, 10.0, 9.0))]),
            sweep(1, vec![item("ghost", Outcome::BelowVolume, 1.0, None, None)]),
        ];
        let report = simulate_one(&sweeps, hours(24));

        assert_eq!(report.calls.book_calls, 2, "one evaluated, one unevaluable");
        assert_eq!(report.unknown_books.unknown, 1);
        assert_eq!(report.unknown_books.unknown_no_fresh_book, 1);
        assert_eq!(report.unknown_books.unknown_subtype_change, 0);
        assert_eq!(report.unknown_books.known, 1, "the bootstrap sweep was known");
        assert_eq!(report.unknown_books.coverage, 0.5);
        assert_eq!(report.false_exclusions.count, 0, "the fresh sweep agreed it was out");
    }

    /// Every forced call is part of the simulated statistics count - otherwise
    /// the saving would be a fiction.
    #[test]
    fn forced_refreshes_are_counted_against_the_savings() {
        let sweeps = vec![
            sweep(0, vec![item("riser", Outcome::NoStatistics, 0.0, None, None)]),
            sweep(1, vec![kept("riser", 100.0, None, book(4, 6, 40.0, 39.0))]),
            sweep(2, vec![kept("riser", 100.0, None, book(4, 6, 41.0, 40.0))]),
        ];
        let report = simulate_one(&sweeps, hours(24));
        let refresh_total: u64 = report.refresh_reasons.values().sum();
        assert_eq!(refresh_total, report.calls.statistics_calls);
        assert_eq!(
            report.forced_refreshes, 2,
            "first sight and the unusable cache entry, both charged"
        );
    }

    /// Two runs over the same logs must serialise identically - the report is
    /// evidence, so a diff has to mean something changed.
    #[test]
    fn running_twice_is_byte_identical() {
        let sweeps = vec![
            sweep(
                0,
                vec![
                    kept("alpha", 20.0, None, book(3, 4, 30.0, 29.0)),
                    item("beta", Outcome::BelowVolume, 1.0, None, None),
                    kept("gamma", 50.0, Some("intact"), book(10, 12, 5.0, 4.5)),
                ],
            ),
            sweep(
                2,
                vec![
                    kept("alpha", 21.0, Some("intact"), book(3, 4, 31.0, 30.0)),
                    kept("beta", 40.0, None, book(2, 2, 12.0, 11.0)),
                    kept("gamma", 50.0, Some("radiant"), book(10, 12, 5.0, 4.5)),
                ],
            ),
        ];
        let first = simulate(&sweeps, &[hours(4), hours(6)], 5);
        let second = simulate(&sweeps, &[hours(4), hours(6)], 5);
        let a = serde_json::to_string_pretty(&first).unwrap();
        let b = serde_json::to_string_pretty(&second).unwrap();
        assert_eq!(a, b);
    }

    // ---- reader -----------------------------------------------------------

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "wfm-replay-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_log(
        dir: &Path,
        started_at: &str,
        items: Vec<observations::Item>,
        with_summary: bool,
        format: u32,
    ) -> String {
        let file = format!("sweep-{}.jsonl", started_at.replace(':', "-"));
        let count = items.len();
        let kept = items.iter().filter(|i| i.book.is_some()).count();
        let run = Run {
            format,
            version: "test".to_string(),
            platform: "pc".to_string(),
            filter: "prime".to_string(),
            exclude: "set".to_string(),
            min_volume: 5,
            items: count,
            workers: 2,
            started_at: started_at.to_string(),
        };
        let mut lines = vec![serde_json::to_string(&Record::Run(run)).unwrap()];
        for item in items {
            lines.push(serde_json::to_string(&Record::Item(item)).unwrap());
        }
        if with_summary {
            lines.push(
                serde_json::to_string(&Record::Summary(Summary {
                    scanned: count,
                    kept,
                    coercions: 0,
                }))
                .unwrap(),
            );
        }
        std::fs::write(dir.join(&file), lines.join("\n") + "\n").unwrap();
        file
    }

    #[test]
    fn an_incomplete_log_is_skipped() {
        let dir = temp_dir("incomplete");
        write_log(
            &dir,
            "2026-09-15T00:00:00Z",
            vec![kept("alpha", 20.0, None, book(3, 4, 30.0, 29.0))],
            true,
            observations::FORMAT,
        );
        write_log(
            &dir,
            "2026-09-15T02:00:00Z",
            vec![kept("alpha", 20.0, None, book(3, 4, 30.0, 29.0))],
            false,
            observations::FORMAT,
        );

        let loaded = read_directory(&dir, None, None).unwrap();
        assert_eq!(loaded.logs_seen, 2);
        assert_eq!(loaded.sweeps.len(), 1);
        assert_eq!(loaded.skipped_incomplete, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unknown_format_is_refused() {
        let dir = temp_dir("format");
        write_log(
            &dir,
            "2026-09-15T00:00:00Z",
            vec![kept("alpha", 20.0, None, book(3, 4, 30.0, 29.0))],
            true,
            observations::FORMAT + 7,
        );

        let error = read_directory(&dir, None, None).unwrap_err();
        assert!(error.contains("format"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn out_of_range_logs_are_excluded_from_the_window() {
        let dir = temp_dir("range");
        write_log(
            &dir,
            "2026-09-15T00:00:00Z",
            vec![kept("alpha", 20.0, None, book(3, 4, 30.0, 29.0))],
            true,
            observations::FORMAT,
        );
        write_log(
            &dir,
            "2026-09-16T00:00:00Z",
            vec![kept("alpha", 20.0, None, book(3, 4, 30.0, 29.0))],
            true,
            observations::FORMAT,
        );

        let from = parse_bound("2026-09-15", false).unwrap();
        let to = parse_bound("2026-09-15", true).unwrap();
        let loaded = read_directory(&dir, Some(from), Some(to)).unwrap();
        assert_eq!(loaded.sweeps.len(), 1);
        assert_eq!(loaded.skipped_out_of_range, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_row_count_mismatch_is_skipped_as_malformed() {
        let dir = temp_dir("malformed");
        let file = write_log(
            &dir,
            "2026-09-15T00:00:00Z",
            vec![kept("alpha", 20.0, None, book(3, 4, 30.0, 29.0))],
            true,
            observations::FORMAT,
        );
        // Claim three items while carrying one: not a cross-section.
        let text = std::fs::read_to_string(dir.join(&file)).unwrap();
        let patched = text.replacen("\"items\":1", "\"items\":3", 1);
        std::fs::write(dir.join(&file), patched).unwrap();

        let loaded = read_directory(&dir, None, None).unwrap();
        assert_eq!(loaded.sweeps.len(), 0);
        assert_eq!(loaded.skipped_malformed.len(), 1);
        assert!(loaded.skipped_malformed.first().unwrap().reason.contains("3"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicate_slugs_are_skipped_as_malformed() {
        let dir = temp_dir("duplicate");
        write_log(
            &dir,
            "2026-09-15T00:00:00Z",
            vec![
                kept("alpha", 20.0, None, book(3, 4, 30.0, 29.0)),
                kept("alpha", 21.0, None, book(3, 4, 30.0, 29.0)),
            ],
            true,
            observations::FORMAT,
        );

        let loaded = read_directory(&dir, None, None).unwrap();
        assert_eq!(loaded.sweeps.len(), 0);
        assert_eq!(loaded.skipped_malformed.len(), 1);
        assert!(
            loaded
                .skipped_malformed
                .first()
                .unwrap()
                .reason
                .contains("unique slugs"),
            "{:?}",
            loaded.skipped_malformed
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_run_header_is_refused() {
        let dir = temp_dir("norun");
        std::fs::write(
            dir.join("sweep-2026-09-15T00-00-00Z.jsonl"),
            format!(
                "{}\n{}\n",
                serde_json::to_string(&Record::Item(kept(
                    "alpha",
                    20.0,
                    None,
                    book(3, 4, 30.0, 29.0)
                )))
                .unwrap(),
                serde_json::to_string(&Record::Summary(Summary {
                    scanned: 1,
                    kept: 1,
                    coercions: 0
                }))
                .unwrap()
            ),
        )
        .unwrap();
        let error = read_directory(&dir, None, None).unwrap_err();
        assert!(error.contains("run header"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mismatched_configurations_are_refused() {
        let mut first = sweep(0, vec![kept("alpha", 20.0, None, book(3, 4, 30.0, 29.0))]);
        let mut second = sweep(2, vec![kept("alpha", 20.0, None, book(3, 4, 30.0, 29.0))]);
        second.run.min_volume = 1;
        first.run.min_volume = 5;
        let error = validate_compatible(&[first, second]).unwrap_err();
        assert!(error.contains("different configuration"), "{error}");
    }

    #[test]
    fn schedule_parsing_accepts_the_documented_forms() {
        assert_eq!(
            parse_schedule("4h,6h,12h,24h").unwrap(),
            vec![hours(4), hours(6), hours(12), hours(24)]
        );
        assert_eq!(parse_schedule("90m").unwrap(), vec![Duration::from_secs(5_400)]);
        assert_eq!(parse_schedule("1d").unwrap(), vec![Duration::from_secs(86_400)]);
        assert_eq!(parse_schedule("4h,240m").unwrap().len(), 1, "deduplicated");
        assert!(parse_schedule("4x").is_err());
        assert!(parse_schedule("0h").is_err());
        assert!(parse_schedule("").is_err());
    }

    #[test]
    fn an_empty_window_is_an_error_not_an_empty_pass() {
        let dir = temp_dir("empty");
        let options = ReplayOptions {
            observations: dir.clone(),
            schedule: vec![hours(4)],
            from: None,
            to: None,
        };
        let error = run(&options).unwrap_err();
        assert!(error.contains("no complete observation logs"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
