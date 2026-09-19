//! Passive per-item observation log.
//!
//! The refresh-schedule question - may statistics be refreshed every k sweeps
//! instead of every sweep? - cannot be answered from the published snapshot: it
//! keeps only the newest row per item, already carrying the values the sweep
//! applied on top of them. These logs keep, per sweep, what each attempted
//! item's statistics said and what its live book held, so a later replay can
//! apply an older statistics selection to a newer book and measure what changes.
//!
//! One JSONL file per sweep: a run header, one record per ATTEMPTED item
//! (including the ones the volume gate dropped, because hiding those is exactly
//! what a stale statistics cache would do), and a summary. The sweep streams
//! into `<name>.partial` and renames it when it completes, so a truncated log is
//! never mistaken for a finished one.
//!
//! Nothing here may fail a sweep. A log that cannot be written is a warning and
//! an absent log, never a lost snapshot.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

/// Bumped when a field's meaning changes. A replay refuses what it cannot read.
pub const FORMAT: u32 = 1;

/// Retention. Observations are only useful while they overlap a schedule being
/// tested, and the host's disk is small; the oldest logs go first.
///
/// The evaluation window is four weeks, so retention has to outlast it: at the
/// old 28 days the corpus lost its earliest days exactly as fast as it gained
/// new ones, and the window it existed to measure never closed. Production logs
/// measure ~1.55 MB per sweep at 12 sweeps a day, about 18.6 MB/day, so 56 days
/// is roughly a gibibyte of steady state. The size cap is the backstop for a
/// sweep that suddenly writes far more; the age rule is the working limit.
const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_AGE: Duration = Duration::from_secs(56 * 24 * 60 * 60);

/// What the sweep concluded about one item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// A CSV row this sweep.
    Kept,
    /// Statistics came back under the volume gate, so no book was fetched.
    BelowVolume,
    /// The statistics payload was empty; the item was skipped unread.
    NoStatistics,
}

/// What the live book held. Present only when the sweep fetched it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Book {
    pub live_buys: i64,
    pub live_sells: i64,
    pub buy_sell_ratio: f64,
    pub top_buy_price: f64,
    pub low_sell_price: f64,
    /// Before clamping. The CSV keeps only the clamped value, and the clamp
    /// depends on the statistics baseline a replay is trying to vary.
    pub low5_avg_unclamped: f64,
    pub score: f64,
}

/// One attempted item. The statistics figures are unrounded, so a replay can
/// recompute the clamp and the score the same way the sweep would have.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub slug: String,
    pub name: String,
    pub tags: Vec<String>,
    pub ducats: Option<i64>,
    pub outcome: Outcome,
    /// The tier the statistics picked. A replay that picks a different one
    /// cannot apply this book: the book was filtered to this tier.
    pub subtype: Option<String>,
    pub volume_48h: f64,
    pub median_now: f64,
    pub median_90d: f64,
    pub medians_7d: Vec<f64>,
    pub avg_price_48h: f64,
    pub donch_top_90d: f64,
    pub donch_bot_90d: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub book: Option<Book>,
}

/// What the sweep was, so a replay can scope and trust the rows that follow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub format: u32,
    pub version: String,
    pub platform: String,
    pub filter: String,
    pub exclude: String,
    pub min_volume: i64,
    pub items: usize,
    pub workers: usize,
    pub started_at: String,
}

/// The sweep's own totals, written last. A log without this record is an
/// incomplete sweep: its rows are real, but they are not a cross-section. The
/// sweep's stamp is the header's - one sweep is one observation time, and the
/// `sweep metrics:` line carries how long it took.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub scanned: usize,
    pub kept: usize,
    pub coercions: u64,
}

/// One JSONL line. `kind` keeps the file readable and lets a replay skip records
/// a newer writer added.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Record {
    Run(Run),
    Item(Item),
    Summary(Summary),
}

/// The sweep's open log. The collector thread owns it, so no worker ever writes
/// a file and rows arrive in completion order rather than catalog order - every
/// row carries its slug and the header carries the sweep's stamp.
pub struct Log {
    directory: PathBuf,
    partial: PathBuf,
    complete: PathBuf,
    writer: Option<BufWriter<File>>,
    failed: bool,
}

impl Log {
    /// Open a log for a sweep in `directory`. Failures are reported once on
    /// stderr and leave an inert log: evidence that cannot be written must never
    /// cost the snapshot.
    pub fn start(directory: &Path, header: Run) -> Log {
        let name = format!("sweep-{}.jsonl", header.started_at.replace(':', "-"));
        let mut log = Log {
            directory: directory.to_path_buf(),
            partial: directory.join(format!("{name}.partial")),
            complete: directory.join(name),
            writer: None,
            failed: false,
        };
        // Before writing anything: a sweep that died mid-run never reached
        // `finish`, so cleanup cannot depend on this one completing.
        prune(directory, MAX_BYTES, MAX_AGE);
        match std::fs::create_dir_all(directory).and_then(|_| File::create(&log.partial)) {
            Ok(file) => {
                log.writer = Some(BufWriter::new(file));
                log.write(&Record::Run(header));
            }
            Err(e) => log.fail(format!("observation log {directory:?}: {e}")),
        }
        log
    }

    pub fn item(&mut self, item: Item) {
        self.write(&Record::Item(item));
    }

    /// Write the summary, flush, sync and rename into place, then apply
    /// retention. Called once, only when the sweep completed.
    pub fn finish(mut self, summary: Summary) {
        self.write(&Record::Summary(summary));
        let Some(writer) = self.writer.take() else {
            return;
        };
        let synced = writer
            .into_inner()
            .map_err(|e| e.into_error())
            .and_then(|file| file.sync_all());
        if let Err(e) = synced {
            self.fail(format!("observation log not finalized: {e}"));
            return;
        }
        if let Err(e) = std::fs::rename(&self.partial, &self.complete) {
            self.fail(format!("observation log not published: {e}"));
            return;
        }
        // This sweep's own file can be what pushes the directory over the cap.
        prune(&self.directory, MAX_BYTES, MAX_AGE);
    }

    fn write(&mut self, record: &Record) {
        let Some(writer) = self.writer.as_mut() else {
            return;
        };
        let line = match serde_json::to_vec(record) {
            Ok(line) => line,
            Err(e) => {
                self.fail(format!("observation log encode: {e}"));
                return;
            }
        };
        let written = writer
            .write_all(&line)
            .and_then(|()| writer.write_all(b"\n"));
        if let Err(e) = written {
            self.fail(format!("observation log write: {e}"));
        }
    }

    /// Give up on this log. The partial file stays behind: a sweep that died
    /// mid-run is worth reading, and deleting it would hide the evidence.
    fn fail(&mut self, message: String) {
        if !self.failed {
            self.failed = true;
            eprintln!("warning: {message}; the sweep continues without an observation log");
        }
        self.writer = None;
    }
}

/// Delete the oldest logs until `directory` fits `max_bytes` and holds nothing
/// older than `max_age`. Incomplete logs count too - a sweep that keeps failing
/// still writes them - and both kinds age out.
fn prune(directory: &Path, max_bytes: u64, max_age: Duration) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut logs: Vec<(PathBuf, u64, SystemTime)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let is_log = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("sweep-"));
        if !is_log {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        logs.push((path, metadata.len(), modified));
    }
    // The name carries the sweep's start stamp, so name order is age order.
    logs.sort_by(|a, b| a.0.cmp(&b.0));
    let mut total: u64 = logs.iter().map(|(_, len, _)| len).sum();
    let now = SystemTime::now();
    for (path, len, modified) in logs {
        let expired = now
            .duration_since(modified)
            .is_ok_and(|age| age > max_age);
        if !expired && total <= max_bytes {
            continue;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(started_at: &str) -> Run {
        Run {
            format: FORMAT,
            version: "test".into(),
            platform: "pc".into(),
            filter: String::new(),
            exclude: String::new(),
            min_volume: 1,
            items: 1,
            workers: 2,
            started_at: started_at.into(),
        }
    }

    fn item(slug: &str, outcome: Outcome) -> Item {
        Item {
            slug: slug.into(),
            name: slug.into(),
            tags: vec![],
            ducats: None,
            outcome,
            subtype: None,
            volume_48h: 5.0,
            median_now: 20.0,
            median_90d: 21.0,
            medians_7d: vec![20.0, 21.0],
            avg_price_48h: 20.5,
            donch_top_90d: 30.0,
            donch_bot_90d: 10.0,
            book: None,
        }
    }

    fn records(path: &Path) -> Vec<Record> {
        std::fs::read_to_string(path)
            .expect("log readable")
            .lines()
            .map(|line| serde_json::from_str(line).expect("every line is a record"))
            .collect()
    }

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wfm-obs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_finished_sweep_writes_a_header_its_items_and_a_summary() {
        let dir = dir("finished");
        let mut log = Log::start(&dir, header("2026-09-15T22:07:20Z"));
        log.item(item("kept_item", Outcome::Kept));
        log.item(item("thin_item", Outcome::BelowVolume));
        log.finish(Summary {
            scanned: 2,
            kept: 1,
            coercions: 0,
        });

        let complete = dir.join("sweep-2026-09-15T22-07-20Z.jsonl");
        assert!(complete.exists(), "the finished log is renamed into place");
        assert!(
            !dir.join("sweep-2026-09-15T22-07-20Z.jsonl.partial").exists(),
            "the partial is gone once published"
        );
        let records = records(&complete);
        assert_eq!(records.len(), 4);
        assert!(matches!(&records[0], Record::Run(run) if run.workers == 2));
        assert!(
            matches!(&records[1], Record::Item(item) if item.slug == "kept_item" && item.outcome == Outcome::Kept)
        );
        assert!(
            matches!(&records[2], Record::Item(item) if item.slug == "thin_item" && item.outcome == Outcome::BelowVolume)
        );
        assert!(matches!(&records[3], Record::Summary(summary) if summary.kept == 1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unfinished_log_stays_a_partial_for_post_mortem() {
        let dir = dir("partial");
        let mut log = Log::start(&dir, header("2026-09-15T22:07:20Z"));
        log.item(item("kept_item", Outcome::Kept));
        drop(log);

        let partial = dir.join("sweep-2026-09-15T22-07-20Z.jsonl.partial");
        assert!(partial.exists(), "an aborted sweep leaves its rows behind");
        assert_eq!(records(&partial).len(), 2, "header plus the item written");
        assert!(records(&partial).iter().all(|r| !matches!(r, Record::Summary(_))));
        assert!(!dir.join("sweep-2026-09-15T22-07-20Z.jsonl").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unusable_directory_is_a_warning_not_a_failure() {
        let dir = dir("unusable");
        std::fs::create_dir_all(&dir).expect("parent");
        let blocked = dir.join("not-a-directory");
        std::fs::write(&blocked, b"file").expect("blocker");

        let mut log = Log::start(&blocked, header("2026-09-15T22:07:20Z"));
        log.item(item("kept_item", Outcome::Kept));
        log.finish(Summary {
            scanned: 1,
            kept: 1,
            coercions: 0,
        });
        assert_eq!(std::fs::read(&blocked).expect("blocker intact"), b"file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Age a file past the horizon without waiting for it.
    fn expire(path: &Path) {
        let old = SystemTime::now() - Duration::from_secs(MAX_AGE.as_secs() + 60);
        std::fs::File::options()
            .write(true)
            .open(path)
            .expect("open the seeded log")
            .set_times(std::fs::FileTimes::new().set_modified(old))
            .expect("set its mtime");
    }

    /// The bug this exists for: `finish` pruned the file it had just renamed
    /// instead of the directory, so `read_dir` failed silently and the cap was
    /// only a promise in the docs. The unit test above could not see it.
    #[test]
    fn finishing_a_sweep_prunes_its_directory() {
        let dir = dir("finish-prunes");
        std::fs::create_dir_all(&dir).expect("dir");
        let stale = dir.join("sweep-2026-01-01T00-00-00Z.jsonl");
        std::fs::write(&stale, b"old").expect("seed");
        expire(&stale);

        let mut log = Log::start(&dir, header("2026-09-15T22:07:20Z"));
        log.item(item("kept_item", Outcome::Kept));
        log.finish(Summary {
            scanned: 1,
            kept: 1,
            coercions: 0,
        });

        assert!(!stale.exists(), "the expired log goes when the sweep finishes");
        assert!(dir.join("sweep-2026-09-15T22-07-20Z.jsonl").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The log's format and retention bounds are restated in shell - the host
    /// readiness check reads both - so a one-sided bump would leave the box
    /// judging the corpus by numbers the pipeline no longer writes or prunes
    /// with. The shared fixture is the only thing that can see that drift.
    #[test]
    fn log_contract_matches_the_shared_fixture() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/observation-retention.json"
        ))
        .expect("observation contract fixture parses");
        assert_eq!(
            fixture["format"].as_u64(),
            Some(u64::from(FORMAT)),
            "FORMAT drifted from tests/fixtures/observation-retention.json"
        );
        assert_eq!(
            fixture["max_bytes"].as_u64(),
            Some(MAX_BYTES),
            "MAX_BYTES drifted from tests/fixtures/observation-retention.json"
        );
        let days = fixture["max_age_days"].as_u64().expect("max_age_days");
        assert_eq!(
            MAX_AGE,
            Duration::from_secs(days * 24 * 60 * 60),
            "MAX_AGE drifted from tests/fixtures/observation-retention.json"
        );
    }

    /// A sweep that dies never reaches `finish`, so cleanup cannot live only
    /// there or partials accumulate until the disk fills.
    #[test]
    fn starting_a_sweep_prunes_what_a_dead_one_left() {
        let dir = dir("start-prunes");
        std::fs::create_dir_all(&dir).expect("dir");
        let stale = dir.join("sweep-2026-01-01T00-00-00Z.jsonl.partial");
        std::fs::write(&stale, b"half a sweep").expect("seed");
        expire(&stale);

        let log = Log::start(&dir, header("2026-09-15T22:07:20Z"));
        drop(log);

        assert!(!stale.exists(), "the stale partial goes when the next sweep starts");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn retention_drops_the_oldest_logs_and_the_expired_ones() {
        let dir = dir("retention");
        std::fs::create_dir_all(&dir).expect("dir");
        for (name, body) in [
            ("sweep-2026-01-01T00-00-00Z.jsonl", "a".repeat(40)),
            ("sweep-2026-01-02T00-00-00Z.jsonl", "b".repeat(40)),
            ("sweep-2026-01-03T00-00-00Z.jsonl.partial", "c".repeat(40)),
            ("unrelated.jsonl", "d".repeat(40)),
        ] {
            std::fs::write(dir.join(name), body).expect("seed");
        }

        // 120 bytes of logs, a 60-byte cap: the oldest go first, and the
        // partial counts like any other log.
        prune(&dir, 60, Duration::from_secs(60 * 60));
        assert!(!dir.join("sweep-2026-01-01T00-00-00Z.jsonl").exists());
        assert!(!dir.join("sweep-2026-01-02T00-00-00Z.jsonl").exists());
        assert!(dir.join("sweep-2026-01-03T00-00-00Z.jsonl.partial").exists());
        assert!(
            dir.join("unrelated.jsonl").exists(),
            "only sweep logs are candidates"
        );

        // Everything is long past the age horizon now.
        prune(&dir, u64::MAX, Duration::ZERO);
        assert!(!dir.join("sweep-2026-01-02T00-00-00Z.jsonl").exists());
        assert!(!dir.join("sweep-2026-01-03T00-00-00Z.jsonl.partial").exists());
        assert!(dir.join("unrelated.jsonl").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
