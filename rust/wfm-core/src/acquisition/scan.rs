//! Game process detection + cross-platform memory scanning.
//!
//! `scan_session(pid)` is implemented twice, gated by `#[cfg(target_os = …)]`:
//! Linux walks `/proc/<pid>/maps` and seek+reads `/proc/<pid>/mem`; Windows
//! walks regions with `VirtualQueryEx` and reads them with `ReadProcessMemory`.
//! Both feed the same regex aggregation and dominant-pair pick.

// `anyhow!` is only used by the Linux-gated `ptrace_open_error`; a bare
// import would be an unused_imports warning on the Windows leg.
#[cfg(target_os = "linux")]
use anyhow::anyhow;
use anyhow::{bail, Result};
use regex::bytes::Regex;

use crate::poison::{read_guard, write_guard};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, RwLock};
use std::time::{Duration, Instant};
use sysinfo::System;

/// The session secrets + build metadata scraped out of the running game.
///
/// Fields are session secrets while a play session is live - never print
/// `account_id` / `nonce`.
pub struct SessionInfo {
    pub account_id: String,
    pub nonce: String,
    pub build: Option<String>,
    pub ct: String,
    pub cred_hits: usize,
    pub distinct_creds: usize,
}

pub fn find_wf_pid() -> Option<u32> {
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    for (pid, process) in sys.processes() {
        if matches_warframe(process) {
            return Some(pid.as_u32());
        }
    }
    None
}

pub fn matches_warframe(p: &sysinfo::Process) -> bool {
    // /proc/<pid>/comm is capped at 15 chars on Linux, so "Warframe.x64.exe"
    // arrives as "Warframe.x64.ex". Match the un-ambiguous prefix instead.
    let name = p.name().to_string_lossy();
    if name.starts_with("Warframe.x64") || name == "Warframe.exe" {
        return true;
    }
    // Belt-and-braces: check the full exe path (Wine / Proton give a real
    // path; some setups have a different comm than the file name).
    if let Some(exe) = p.exe() {
        let s = exe.to_string_lossy();
        if s.contains("Warframe.x64.exe") || s.ends_with("/Warframe.exe") {
            return true;
        }
    }
    false
}

// Confirmed in May 2026 memory scan: this exact form appears in the URLs the
// game sends. ASCII [0-9] (not \d) so we don't need the regex crate's
// unicode-perl feature - saves ~150 KB on the binary.
pub const DEFAULT_CRED_PATTERN: &str = r"accountId=([0-9a-fA-F]{24})&nonce=([0-9]{6,})";
pub const DEFAULT_BUILD_PATTERN: &str = r#""BuildLabel":"([0-9.]+)/[A-Za-z0-9]+"#;
pub const DEFAULT_CT_PATTERN: &str = r"&ct=([A-Z]{2,4})\b";

/// A remote pattern longer than this is rejected unread. The real patterns are
/// well under 60 bytes; the cap exists so a corrupt or hostile definitions file
/// cannot hand the scanner something absurd to compile on every launch.
const MAX_PATTERN_LEN: usize = 512;

/// The three patterns a scan searches for, as one swappable set.
///
/// Compiled once per scan rather than once per chunk: `aggregate_match` runs on
/// every ~4 MB chunk (Linux) / every VirtualQuery region (Windows), which is
/// hundreds to low-thousands of calls on a multi-GB game process, and
/// `Regex::new()` was once re-run 3x on every single one.
pub struct ScanPatterns {
    cred: Regex,
    build: Regex,
    ct: Regex,
}

impl Default for ScanPatterns {
    #[allow(
        clippy::unwrap_used,
        reason = "compile-time constant patterns are exercised by the test suite"
    )]
    fn default() -> Self {
        // unwrap is honest here: these are compile-time constants that the test
        // suite compiles. A failure is a build-breaking typo, not a runtime path.
        ScanPatterns {
            cred: Regex::new(DEFAULT_CRED_PATTERN).unwrap(),
            build: Regex::new(DEFAULT_BUILD_PATTERN).unwrap(),
            ct: Regex::new(DEFAULT_CT_PATTERN).unwrap(),
        }
    }
}

/// The remote `definitions.json` shape. Every field is optional: a definitions
/// file that only fixes the credential pattern leaves the other two alone.
#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct ScanDefinitions {
    /// Bumped by us for humans reading the file; the app does not gate on it.
    #[serde(default)]
    pub version: Option<u32>,
    #[serde(default)]
    pub cred_pattern: Option<String>,
    #[serde(default)]
    pub build_pattern: Option<String>,
    #[serde(default)]
    pub ct_pattern: Option<String>,
}

/// Why a supplied pattern was refused. Surfaced so a bad definitions push is
/// diagnosable from the app's own log instead of looking like "scan broke".
#[derive(Debug, PartialEq)]
pub struct PatternRejection {
    pub field: &'static str,
    pub reason: String,
}

/// Capture-group indices that are guaranteed to participate in **every** match
/// of `pattern`, or `None` if the pattern cannot be parsed.
///
/// The `regex` crate exposes no per-group optionality, and `static_captures_len`
/// answers a different question - it returns `Some(3)` for `(a)(b)|(c)(d)`, where
/// group 1 is absent whenever the second branch matches. So the pattern is parsed
/// into its intermediate representation and the guarantee is computed
/// structurally:
///
/// - a capture contributes its own index plus whatever its child guarantees;
/// - concatenation guarantees the union of its parts;
/// - alternation guarantees only the **intersection**, because one branch that
///   omits a group makes that group optional;
/// - a repetition guarantees nothing when it can match zero times.
///
/// Deliberately conservative: it can refuse a pattern that in practice always
/// supplies the group, and it never accepts one that can omit it. Parsed with
/// `utf8(false)` to match `regex::bytes::Regex`, which is what the scan compiles.
fn guaranteed_captures(pattern: &str) -> Option<HashSet<usize>> {
    let hir = regex_syntax::ParserBuilder::new()
        .utf8(false)
        .build()
        .parse(pattern)
        .ok()?;
    Some(guaranteed(&hir))
}

fn guaranteed(hir: &regex_syntax::hir::Hir) -> HashSet<usize> {
    use regex_syntax::hir::HirKind;
    match hir.kind() {
        HirKind::Capture(capture) => {
            let mut set = guaranteed(&capture.sub);
            set.insert(capture.index as usize);
            set
        }
        HirKind::Concat(parts) => parts.iter().flat_map(guaranteed).collect(),
        HirKind::Alternation(branches) => {
            let mut branches = branches.iter();
            let Some(first) = branches.next() else {
                return HashSet::new();
            };
            let mut acc = guaranteed(first);
            for branch in branches {
                let other = guaranteed(branch);
                acc.retain(|index| other.contains(index));
            }
            acc
        }
        HirKind::Repetition(repetition) => {
            if repetition.min == 0 {
                HashSet::new()
            } else {
                guaranteed(&repetition.sub)
            }
        }
        HirKind::Empty | HirKind::Literal(_) | HirKind::Class(_) | HirKind::Look(_) => HashSet::new(),
    }
}

/// Compile a definitions file into a usable pattern set.
///
/// Each pattern is validated INDEPENDENTLY and falls back to the compiled-in
/// default on any problem, so one bad entry cannot disable scanning wholesale -
/// the point of shipping this is to fix a broken scan without a release, and a
/// remote file that can brick the scanner would defeat that.
///
/// Validation is three checks. The arity one rejects a pattern that cannot fit
/// the contract at all, but it is NOT a safety guarantee: it counts capture
/// groups, and a group that exists in the pattern can still be absent from a
/// given match - a top-level alternation or an optional group is enough. The
/// match loop therefore reads the required groups as Options and skips matches
/// that cannot yield a value, so no accepted pattern can abort a scan.
///
/// The length cap bounds what we agree to compile; it does not bound the work a
/// scan can do. A single search is linear, but `captures_iter` is documented as
/// `O(m * n^2)` in the worst case because each search may rescan the haystack -
/// the crate's own example is `.*[^A-Z]|[A-Z]` over a long run of uppercase.
/// Both the pattern and the haystack are untrusted here, so a definition can be
/// quadratic by construction; the scan's own work budget is what contains that.
pub fn patterns_from_definitions(defs: &ScanDefinitions) -> (ScanPatterns, Vec<PatternRejection>) {    let mut rejections = Vec::new();
    let default = ScanPatterns::default();

    fn build_one(
        field: &'static str,
        supplied: Option<&String>,
        groups: usize,
        fallback: Regex,
        rejections: &mut Vec<PatternRejection>,
    ) -> Regex {
        let Some(raw) = supplied else { return fallback };
        if raw.is_empty() {
            return fallback;
        }
        if raw.len() > MAX_PATTERN_LEN {
            rejections.push(PatternRejection {
                field,
                reason: format!("{} bytes exceeds the {MAX_PATTERN_LEN}-byte cap", raw.len()),
            });
            return fallback;
        }
        let compiled = match Regex::new(raw) {
            Ok(re) => re,
            Err(e) => {
                rejections.push(PatternRejection {
                    field,
                    reason: format!("does not compile: {e}"),
                });
                return fallback;
            }
        };
        // captures_len() counts the implicit whole-match group, so a pattern
        // with N capture groups reports N + 1.
        let have = compiled.captures_len().saturating_sub(1);
        if have < groups {
            rejections.push(PatternRejection {
                field,
                reason: format!("needs {groups} capture group(s), has {have}"),
            });
            return fallback;
        }
        // Counting groups is not the same as proving they participate. A group
        // inside a top-level alternation branch, or under an optional quantifier,
        // is absent from some matches - and this is remote input, so the pattern
        // is refused rather than relied on.
        let required: HashSet<usize> = (1..=groups).collect();
        if !guaranteed_captures(raw).is_some_and(|captures| required.is_subset(&captures)) {
            rejections.push(PatternRejection {
                field,
                reason: format!(
                    "capture group(s) 1..{groups} are not guaranteed to participate in every \
                     match; a top-level alternation or an optional group makes them optional. \
                     Put a rotated alternative inside the capture, as `(a|b)`, not around it"
                ),
            });
            return fallback;
        }
        compiled
    }

    let cred = build_one(
        "cred_pattern",
        defs.cred_pattern.as_ref(),
        2,
        default.cred,
        &mut rejections,
    );
    let build = build_one(
        "build_pattern",
        defs.build_pattern.as_ref(),
        1,
        default.build,
        &mut rejections,
    );
    let ct = build_one(
        "ct_pattern",
        defs.ct_pattern.as_ref(),
        1,
        default.ct,
        &mut rejections,
    );

    (ScanPatterns { cred, build, ct }, rejections)
}

/// The pattern set every scan uses, swappable at runtime by the shell once it
/// has fetched `definitions.json`.
///
/// A process global rather than a `scan_session` parameter so the fetch stays a
/// shell concern: wfm-core owns no network policy, and every existing caller
/// keeps its signature. Each scan takes ONE snapshot up front, so a definitions
/// swap landing mid-scan cannot change the patterns underneath a run in
/// progress.
static INSTALLED: LazyLock<RwLock<Arc<ScanPatterns>>> =
    LazyLock::new(|| RwLock::new(Arc::new(ScanPatterns::default())));

/// Replace the pattern set for subsequent scans. Applying an all-default set
/// is the documented way to revert to compiled-in behaviour.
pub fn install_patterns(patterns: ScanPatterns) {
    *write_guard(&INSTALLED) = Arc::new(patterns);
}

/// The set a scan should use, snapshotted for the duration of that scan.
pub fn current_patterns() -> Arc<ScanPatterns> {
    Arc::clone(&read_guard(&INSTALLED))
}

/// Resource ceiling for one scan.
///
/// The scanner walks the game's entire address space while running patterns that
/// come from a remote definitions file, so two things need a ceiling: how many
/// matches we are willing to examine, and how much of what they captured we are
/// willing to keep.
///
/// The match ceiling is the one that bounds CPU. Iterating matches in the
/// `regex` crate is `O(m * n^2)` in the worst case - each search can rescan the
/// haystack - so the number of searches is the quadratic factor, and a pattern
/// like `.*[^A-Z]|[A-Z]` over a long run of uppercase reaches a large match count
/// very quickly. The retention ceilings bound memory, which a match count alone
/// does not: a single captured value is arbitrarily long.
///
/// Exceeding any ceiling fails the scan. It deliberately does not return what was
/// collected first, because a partial credential set looks exactly like a
/// complete one to everything downstream.
#[derive(Debug, Clone)]
pub struct ScanBudget {
    /// Wall-clock ceiling for one scan, measured from [`ScanBudget::started`].
    pub max_elapsed: Duration,
    /// When the scan began.
    pub started: Instant,
    /// Total matches examined across every pattern.
    pub max_matches: usize,
    /// Longest single captured value that may be retained.
    pub max_capture_len: usize,
    /// Most distinct keys retained per accumulator.
    pub max_entries: usize,
    /// Most total bytes of retained captured values.
    pub max_bytes: usize,
}

impl Default for ScanBudget {
    fn default() -> Self {
        Self {
            max_elapsed: Duration::from_secs(120),
            started: Instant::now(),
            max_matches: 20_000,
            max_capture_len: 4 * 1024,
            max_entries: 4_096,
            max_bytes: 1024 * 1024,
        }
    }
}

fn aggregate_match(
    haystack: &[u8],
    pats: &ScanPatterns,
    counts: &mut PatternCounts,
    budget: &ScanBudget,
) -> Result<()> {
    // Checked before each pattern's iteration, not only per yielded match: a
    // hostile pattern is often slow precisely because it matches little, so a
    // per-match clock would never see it. This bounds a *sequence* of slow
    // searches; a single search cannot be interrupted from outside the regex
    // engine - that needs the lower-level automata API.
    counts.charge_time(budget)?;
    for cap in pats.cred.captures_iter(haystack) {
        counts.charge_match(budget)?;
        // A definitions file is remote input. The install-time arity check can
        // only count capture groups, so a group that exists in the pattern may
        // still be absent from a particular match - a top-level alternation or
        // an optional group is enough. Indexing a group that did not
        // participate panics, and release builds abort, so the required groups
        // are read as Options and an unusable match is counted instead.
        let (Some(aid), Some(nonce)) = (cap.get(1), cap.get(2)) else {
            counts.cred_matches_without_groups += 1;
            continue;
        };
        // Charged on the raw match, before the conversion below allocates: the
        // cap exists to bound allocation, not merely retention.
        counts.charge_value(aid.as_bytes().len(), budget)?;
        counts.charge_value(nonce.as_bytes().len(), budget)?;
        let aid = String::from_utf8_lossy(aid.as_bytes()).to_ascii_lowercase();
        let nonce = String::from_utf8_lossy(nonce.as_bytes()).into_owned();
        if !counts.creds.contains_key(&(aid.clone(), nonce.clone())) {
            counts.charge_entry(counts.creds.len(), budget)?;
        }
        *counts.creds.entry((aid, nonce)).or_insert(0) += 1;
    }
    counts.charge_time(budget)?;
    for cap in pats.build.captures_iter(haystack) {
        counts.charge_match(budget)?;
        let Some(m) = cap.get(1) else { continue };
        counts.charge_value(m.as_bytes().len(), budget)?;
        let value = String::from_utf8_lossy(m.as_bytes()).into_owned();
        if !counts.builds.contains_key(&value) {
            counts.charge_entry(counts.builds.len(), budget)?;
        }
        *counts.builds.entry(value).or_insert(0) += 1;
    }
    counts.charge_time(budget)?;
    for cap in pats.ct.captures_iter(haystack) {
        counts.charge_match(budget)?;
        let Some(m) = cap.get(1) else { continue };
        counts.charge_value(m.as_bytes().len(), budget)?;
        let value = String::from_utf8_lossy(m.as_bytes()).into_owned();
        if !counts.cts.contains_key(&value) {
            counts.charge_entry(counts.cts.len(), budget)?;
        }
        *counts.cts.entry(value).or_insert(0) += 1;
    }
    Ok(())
}

/// Message for a scan that spent more than its budget allows.
fn budget_exceeded(what: &str, limit: u64) -> String {
    format!(
        "Scan budget exceeded: {what} reached {limit}. The scan was stopped rather than \
         returning a partial result, because a partial credential set is indistinguishable \
         from a complete one. A definitions override is the likely cause - resetting it \
         restores the built-in patterns."
    )
}

#[derive(Default)]
struct PatternCounts {
    creds: HashMap<(String, String), usize>,
    builds: HashMap<String, usize>,
    cts: HashMap<String, usize>,
    /// Cred matches that could not yield a value because their required groups
    /// did not participate. Non-zero means the installed pattern is unusable,
    /// which is worth telling the user instead of a bare "nothing found".
    cred_matches_without_groups: usize,
    /// Matches examined across every pattern in this scan.
    matches: usize,
    /// Total bytes of retained captured values.
    bytes: usize,
}

impl PatternCounts {
    /// Check the clock on its own, so the time ceiling holds even when a pattern
    /// yields no match at all - which is the shape a deliberately slow pattern
    /// takes. Callers check this before each pattern's iteration.
    fn charge_time(&self, budget: &ScanBudget) -> Result<()> {
        if budget.started.elapsed() >= budget.max_elapsed {
            bail!(
                "{}",
                budget_exceeded("elapsed seconds", budget.max_elapsed.as_secs())
            );
        }
        Ok(())
    }

    /// Charge one examined match. Iterating matches is the quadratic factor, so
    /// this is what stops a hostile pattern.
    fn charge_match(&mut self, budget: &ScanBudget) -> Result<()> {
        self.matches += 1;
        if self.matches > budget.max_matches {
            bail!("{}", budget_exceeded("matches examined", budget.max_matches as u64));
        }
        self.charge_time(budget)
    }

    /// Charge `len` bytes of a captured value, before it is converted. A match
    /// count does not bound memory, because a single capture can be arbitrarily
    /// long; charging the raw length keeps the cap ahead of the allocation.
    fn charge_value(&mut self, len: usize, budget: &ScanBudget) -> Result<()> {
        if len > budget.max_capture_len {
            bail!(
                "{}",
                budget_exceeded("capture bytes", budget.max_capture_len as u64)
            );
        }
        self.bytes += len;
        if self.bytes > budget.max_bytes {
            bail!("{}", budget_exceeded("retained bytes", budget.max_bytes as u64));
        }
        Ok(())
    }

    /// Refuse a new distinct key once the accumulator is full.
    fn charge_entry(&self, held: usize, budget: &ScanBudget) -> Result<()> {
        if held >= budget.max_entries {
            bail!("{}", budget_exceeded("distinct entries", budget.max_entries as u64));
        }
        Ok(())
    }
}

/// The shape DE produces for a session account id.
fn is_account_id(value: &str) -> bool {
    value.len() == 24 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A session nonce is a run of decimal digits.
fn is_nonce(value: &str) -> bool {
    (6..=32).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_digit())
}

/// A build label is a dotted numeric version, e.g. `38.1.2`.
fn is_build_label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 16
        && !value.starts_with('.')
        && !value.ends_with('.')
        && value.bytes().all(|b| b.is_ascii_digit() || b == b'.')
}

/// A platform tag is the short uppercase token DE sends as `ct`.
fn is_platform_tag(value: &str) -> bool {
    (2..=4).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_uppercase())
}

fn pick_dominant(counts: PatternCounts) -> Result<SessionInfo> {
    let total_distinct = counts.creds.len();
    let unusable = counts.cred_matches_without_groups;
    let ((aid, nonce), hits) = match counts.creds.into_iter().max_by_key(|(_, v)| *v) {
        Some(pair) => pair,
        None => bail!("{}", no_creds_message(unusable)),
    };
    // A captured value is remote input: a definitions file can loosen a pattern
    // until it captures anything at all. None of these values stays local. The
    // account id and nonce become request parameters, the build becomes the
    // request's User-Agent and `appVersion` and is recorded in snapshot
    // metadata, and the platform tag becomes a request parameter - so a value
    // with a shape DE never produces is refused rather than forwarded, and a
    // build label carrying control characters cannot reach a header.
    if !is_account_id(&aid) || !is_nonce(&nonce) {
        bail!(
            "The captured accountId/nonce pair does not have the shape DE produces: \
             a 24-digit hexadecimal account id and a numeric nonce are required, and \
             this pair was not used. A definitions override is the likely cause - \
             resetting it restores the built-in patterns."
        );
    }
    let build = counts
        .builds
        .into_iter()
        .filter(|(k, _)| is_build_label(k))
        .max_by_key(|(_, v)| *v)
        .map(|(k, _)| k);
    let ct = counts
        .cts
        .into_iter()
        .filter(|(k, _)| is_platform_tag(k))
        .max_by_key(|(_, v)| *v)
        .map(|(k, _)| k)
        .unwrap_or_else(|| "STM".to_string());
    Ok(SessionInfo {
        account_id: aid,
        nonce,
        build,
        ct,
        cred_hits: hits,
        distinct_creds: total_distinct,
    })
}

/// Why a scan produced no credentials.
///
/// A pattern that matched but could not yield a value is a different problem
/// from a pattern that never matched, and the user can only fix the first one
/// by correcting the definitions file - so say which happened.
fn no_creds_message(cred_matches_without_groups: usize) -> String {
    let mut msg = String::from(
        "No accountId/nonce pair found in WF memory.\n\
         Make sure you're past the login screen and a recent network\n\
         call has fired (opening the trade or profile screen is reliable).",
    );
    if cred_matches_without_groups > 0 {
        msg.push_str(&format!(
            "\n\nThe configured cred pattern matched {cred_matches_without_groups} time(s), \
             but capture groups 1 and 2 did not both participate, so no value can be read. \
             A top-level alternation or an optional group breaks the pattern; it must capture \
             the account id and the nonce as groups 1 and 2."
        ));
    }
    msg
}

// ---- Linux ---------------------------------------------------------------

#[cfg(target_os = "linux")]
pub fn scan_session(pid: u32) -> Result<SessionInfo> {
    use anyhow::Context;
    use std::fs::File;
    use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};

    // One snapshot for the whole scan - a definitions swap landing mid-run
    // must not change the patterns underneath it.
    let pats = current_patterns();

    let maps_path = format!("/proc/{pid}/maps");
    let mem_path = format!("/proc/{pid}/mem");

    let maps_file = File::open(&maps_path)
        .with_context(|| format!("cannot open {maps_path} - does PID {pid} exist?"))?;
    let mut mem_file = File::open(&mem_path).map_err(|e| ptrace_open_error(&mem_path, pid, e))?;

    let mut counts = PatternCounts::default();
    let budget = ScanBudget::default();
    const CHUNK: usize = 4 * 1024 * 1024;
    let overlap = 96;
    // Scratch buffer reused across every chunk of every region - `hay[0..tail_len]`
    // holds the small overlap carried from the previous chunk (0 bytes at the
    // start of a new region) and reads land right after it, so a pattern
    // straddling a chunk boundary still matches without a fresh allocation
    // and copy on every iteration (a multi-GB process is thousands of
    // iterations; this used to allocate+copy ~4 MB on every one of them).
    let mut hay = vec![0u8; overlap + CHUNK];

    let skip_substrings = ["[vvar]", "[vsyscall]", "[vdso]", "/dev/", "/SYSV"];

    for line in BufReader::new(maps_file).lines() {
        let line = line?;
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 {
            continue;
        }
        let addr_range = parts.first().copied().unwrap_or("");
        let perms = parts.get(1).copied().unwrap_or("");
        let path = parts.get(5).copied().unwrap_or("");
        if !perms.contains('r') {
            continue;
        }
        if skip_substrings.iter().any(|s| path.contains(s)) {
            continue;
        }
        let (start_s, end_s) = match addr_range.split_once('-') {
            Some(p) => p,
            None => continue,
        };
        let start: u64 = u64::from_str_radix(start_s, 16)?;
        let end: u64 = u64::from_str_radix(end_s, 16)?;
        let mut offset = start;
        let mut tail_len = 0usize;
        while offset < end {
            let want = std::cmp::min(CHUNK as u64, end - offset) as usize;
            if mem_file.seek(SeekFrom::Start(offset)).is_err() {
                break;
            }
            #[allow(
                clippy::indexing_slicing,
                reason = "buffer sizing keeps tail_len + want within hay"
            )]
            let n = match mem_file.read(&mut hay[tail_len..tail_len + want]) {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };
            let total = tail_len + n;
            #[allow(
                clippy::indexing_slicing,
                reason = "total cannot exceed the initialized buffer prefix"
            )]
            aggregate_match(&hay[..total], &pats, &mut counts, &budget)?;
            let keep = std::cmp::min(overlap, n);
            hay.copy_within(total - keep..total, 0);
            tail_len = keep;
            offset += n as u64;
        }
    }

    pick_dominant(counts)
}

// Turn a /proc/<pid>/mem open failure into actionable guidance. Permission
// denied is the common case (no permission to ptrace the game); anything else
// usually means the PID exited between lookup and read.
//
// The remedy depends on HOW the app is running, which is why this branches:
//
//   AppImage - `setcap` is useless here. The
//     runtime mounts the payload on a fresh nosuid FUSE mount per launch, and
//     the kernel ignores file capabilities on nosuid mounts; even if it did
//     not, `current_exe()` is a /tmp/.mount_* path that ceases to exist when
//     the app closes, so the grant could not outlive one run. The honest fix
//     is to relax Yama.
//
//   A local or extracted binary - the per-binary capability is still the
//     tightest grant available, so keep it.
#[cfg(target_os = "linux")]
fn ptrace_open_error(mem_path: &str, pid: u32, e: std::io::Error) -> anyhow::Error {
    if e.kind() != std::io::ErrorKind::PermissionDenied {
        return anyhow!(
            "cannot open {mem_path}: {e}\n\
             PID {pid} may have exited - restart Warframe past the title screen and retry."
        );
    }
    // Set by the AppImage runtime to the path of the .AppImage itself - the
    // same signal update.rs uses to decide whether self-update can work.
    let appimage = std::env::var_os("APPIMAGE").and_then(|p| p.to_str().map(str::to_owned));
    let scope = std::fs::read_to_string("/proc/sys/kernel/yama/ptrace_scope")
        .ok()
        .map(|s| s.trim().to_owned());

    let mut msg = match &appimage {
        Some(_) => format!(
            "Permission denied reading {mem_path} - reading the game's memory needs \
             permission to ptrace it.\n\
             `setcap` does not work for an AppImage: it runs from a temporary mount that \
             ignores file capabilities, and the path changes every launch.\n\
             Allow same-user ptrace instead:\n  \
             sudo sysctl kernel.yama.ptrace_scope=0\n\
             To keep it across reboots:\n  \
             echo 'kernel.yama.ptrace_scope=0' | sudo tee /etc/sysctl.d/10-tennoworth.conf\n\
             Launching the app itself with sudo is not the alternative: it is a \
             networked GUI that holds your WFM credentials."
        ),
        None => {
            let bin = std::env::current_exe()
                .ok()
                .and_then(|p| p.to_str().map(str::to_owned))
                .unwrap_or_else(|| "tennoworth-desktop".to_string());
            format!(
                "Permission denied reading {mem_path} - reading the game's memory needs CAP_SYS_PTRACE.\n\
                 Grant it once (no sudo needed afterwards):\n  \
                 sudo setcap cap_sys_ptrace=eip \"{bin}\"\n  \
                 {bin}\n\
                 Note: re-installing or rebuilding the binary clears the capability - re-run setcap after an upgrade.\n\
                 Launching the app itself with sudo is not the alternative: it is a \
                 networked GUI that holds your WFM credentials."
            )
        }
    };

    // Whether any of this is needed is decided by kernel.yama.ptrace_scope,
    // NOT by Proton-vs-native (a myth this message used to leave standing: at
    // scope 1 the game is a child of Steam, not of us, so a non-descendant
    // tracer is refused however the game was launched). Name the scope we
    // actually found so the user can tell "expected" from "misconfigured".
    match scope.as_deref() {
        // Yama makes 3 a one-way door: the sysctl write is rejected for the
        // rest of the uptime, so telling anyone to lower it now is a dead end.
        // The only route is config plus a reboot.
        Some("3") => msg.push_str(
            "\n\nkernel.yama.ptrace_scope is 3 (ptrace disabled). This cannot be lowered \
             while the machine is running - the sysctl write is refused once it reaches 3.\n\
             Set it for the next boot and reboot:\n  \
             echo 'kernel.yama.ptrace_scope=0' | sudo tee /etc/sysctl.d/10-tennoworth.conf",
        ),
        Some("0") => msg.push_str(
            "\n\nkernel.yama.ptrace_scope is 0, so this normally would not be needed -\n\
             the game may be running as a different user (a separate Steam or\n\
             Flatpak account), which same-user ptrace does not cover.",
        ),
        Some(s) => msg.push_str(&format!(
            "\n\nkernel.yama.ptrace_scope is {s}: only a process's own descendants\n\
             may read its memory, and the game is a child of Steam, not of us.\n\
             That is the usual desktop default, so this step is expected here -\n\
             it is not caused by Proton, and a native launch behaves the same."
        )),
        None => {}
    }
    anyhow!(msg)
}

// ---- Windows -------------------------------------------------------------

#[cfg(target_os = "windows")]
pub fn scan_session(pid: u32) -> Result<SessionInfo> {
    use anyhow::Context;
    use windows::Win32::Foundation::{CloseHandle, BOOL, HANDLE};
    use windows::Win32::System::Memory::{
        VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };

    use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;

    // One snapshot for the whole scan - see the Linux leg.
    let pats = current_patterns();

    unsafe {
        let handle: HANDLE = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, BOOL(0), pid)
            .context("OpenProcess failed - not running as same user, or pid is wrong")?;

        let mut counts = PatternCounts::default();
    let budget = ScanBudget::default();
        let mut addr: usize = 0;
        let mut mbi = MEMORY_BASIC_INFORMATION::default();
        let mbi_size = std::mem::size_of::<MEMORY_BASIC_INFORMATION>();

        // Regions are read in fixed CHUNKs into one reused buffer, carrying a
        // small overlap from the previous chunk so a pattern straddling a
        // chunk boundary still matches - the same scheme as the Linux leg.
        // This replaces a per-region `vec![0u8; RegionSize]` that both risked
        // an OOM abort on a huge region AND, once capped at 64 MB, silently
        // SKIPPED anything larger - a 64-bit game's heaps routinely exceed
        // that, so a token living in one was simply never seen.
        const CHUNK: usize = 4 * 1024 * 1024;
        let overlap = 96;
        let mut hay = vec![0u8; overlap + CHUNK];

        loop {
            let q = VirtualQueryEx(handle, Some(addr as *const _), &mut mbi, mbi_size);
            if q == 0 {
                break;
            }
            let base = mbi.BaseAddress as usize;
            let next = base.wrapping_add(mbi.RegionSize);
            let readable =
                mbi.State == MEM_COMMIT && (mbi.Protect.0 & (PAGE_NOACCESS.0 | PAGE_GUARD.0)) == 0;
            // A zero-sized or wrapping region would make `next` never advance;
            // it is skipped and the walk bails below.
            if readable && next > base {
                let end = next;
                let mut offset = base;
                let mut tail_len = 0usize;
                while offset < end {
                    let want = std::cmp::min(CHUNK, end - offset);
                    let mut read_n: usize = 0;
                    #[allow(
                        clippy::indexing_slicing,
                        reason = "tail_len is bounded by overlap and the buffer reserves overlap + CHUNK"
                    )]
                    let ok = ReadProcessMemory(
                        handle,
                        offset as *const _,
                        hay[tail_len..].as_mut_ptr() as *mut _,
                        want,
                        Some(&mut read_n),
                    );
                    // A short or failed read ends THIS region (a guard page or
                    // decommit mid-region), never the walk - skip-don't-fail.
                    if ok.is_err() || read_n == 0 {
                        break;
                    }
                    let total = tail_len + read_n;
                    #[allow(
                        clippy::indexing_slicing,
                        reason = "ReadProcessMemory returns at most want bytes, so total fits the buffer"
                    )]
                    aggregate_match(&hay[..total], &pats, &mut counts, &budget)?;
                    let keep = std::cmp::min(overlap, read_n);
                    hay.copy_within(total - keep..total, 0);
                    tail_len = keep;
                    offset += read_n;
                }
            }
            addr = next;
            // No forward progress (zero-sized region or wraparound) - bail out
            // of the walk rather than re-querying the same address forever.
            if addr <= base {
                break;
            }
        }

        let _ = CloseHandle(handle);
        pick_dominant(counts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defs(cred: Option<&str>, build: Option<&str>, ct: Option<&str>) -> ScanDefinitions {
        ScanDefinitions {
            version: Some(1),
            cred_pattern: cred.map(str::to_string),
            build_pattern: build.map(str::to_string),
            ct_pattern: ct.map(str::to_string),
        }
    }

    /// The scan reads groups 1 and 2 as Options and skips a match that cannot
    /// supply both; this mirrors that so a pattern the scan would decline to
    /// read does not panic here either.
    fn creds_found(p: &ScanPatterns, hay: &[u8]) -> Vec<(String, String)> {
        p.cred
            .captures_iter(hay)
            .filter_map(|c| {
                Some((
                    String::from_utf8_lossy(c.get(1)?.as_bytes()).into_owned(),
                    String::from_utf8_lossy(c.get(2)?.as_bytes()).into_owned(),
                ))
            })
            .collect()
    }

    const SAMPLE: &[u8] =
        b"GET /x?accountId=0123456789abcdef01234567&nonce=123456 HTTP/1.1 &ct=STM ";

    #[test]
    fn defaults_match_the_live_url_shape() {
        let p = ScanPatterns::default();
        assert_eq!(
            creds_found(&p, SAMPLE),
            vec![("0123456789abcdef01234567".to_string(), "123456".to_string())]
        );
        assert!(p.ct.is_match(SAMPLE));
    }

    #[test]
    fn an_empty_definitions_file_changes_nothing() {
        let (p, rej) = patterns_from_definitions(&ScanDefinitions::default());
        assert!(rej.is_empty());
        assert_eq!(creds_found(&p, SAMPLE).len(), 1);
    }

    #[test]
    fn a_valid_override_is_applied() {
        // DE rotates the parameter names - the exact scenario this exists for.
        let (p, rej) = patterns_from_definitions(&defs(
            Some(r"acct=([0-9a-f]{24})&n=([0-9]{6,})"),
            None,
            None,
        ));
        assert!(rej.is_empty(), "{rej:?}");
        assert_eq!(
            creds_found(&p, b"acct=0123456789abcdef01234567&n=999888 "),
            vec![("0123456789abcdef01234567".to_string(), "999888".to_string())]
        );
        // The untouched patterns still work.
        assert!(p.ct.is_match(SAMPLE));
    }

    #[test]
    fn a_pattern_that_does_not_compile_falls_back() {
        let (p, rej) = patterns_from_definitions(&defs(Some(r"([unclosed"), None, None));
        assert_eq!(rej.len(), 1);
        assert_eq!(rej[0].field, "cred_pattern");
        assert!(
            rej[0].reason.contains("does not compile"),
            "{}",
            rej[0].reason
        );
        // Fell back, so scanning still works rather than dying.
        assert_eq!(creds_found(&p, SAMPLE).len(), 1);
    }

    #[test]
    fn too_few_capture_groups_is_refused() {
        // Compiles fine, but the match loop indexes cap[2] - accepting this
        // would panic mid-scan on the user's machine.
        let (p, rej) =
            patterns_from_definitions(&defs(Some(r"accountId=([0-9a-f]{24})"), None, None));
        assert_eq!(rej.len(), 1);
        assert!(
            rej[0].reason.contains("needs 2 capture group"),
            "{}",
            rej[0].reason
        );
        assert_eq!(creds_found(&p, SAMPLE).len(), 1);

        // The single-group patterns are held to their own arity.
        let (_, rej_ct) = patterns_from_definitions(&defs(None, None, Some(r"&ct=[A-Z]+")));
        assert_eq!(rej_ct.len(), 1);
        assert!(rej_ct[0].reason.contains("needs 1 capture group"));
    }

    #[test]
    fn an_over_long_pattern_is_refused_without_compiling() {
        let huge = format!("({})", "a|".repeat(400));
        assert!(huge.len() > MAX_PATTERN_LEN);
        let (p, rej) = patterns_from_definitions(&defs(None, Some(&huge), None));
        assert_eq!(rej.len(), 1);
        assert!(rej[0].reason.contains("exceeds"), "{}", rej[0].reason);
        assert!(p.build.is_match(br#""BuildLabel":"38.1.2/ABCdef"#));
    }

    #[test]
    fn one_bad_entry_cannot_disable_the_others() {
        // The whole point: a bad push must not brick scanning.
        let (p, rej) = patterns_from_definitions(&defs(
            Some(r"([unclosed"),
            Some(r#""BuildLabel":"([0-9.]+)"#),
            Some(r"&ct=([A-Z]{2,4})"),
        ));
        assert_eq!(rej.len(), 1, "only the broken one is refused: {rej:?}");
        assert_eq!(
            creds_found(&p, SAMPLE).len(),
            1,
            "cred fell back and still works"
        );
        assert!(p.ct.is_match(SAMPLE), "the valid overrides applied");
    }

    #[test]
    fn an_empty_string_means_use_the_default_not_match_everything() {
        let (p, rej) = patterns_from_definitions(&defs(Some(""), None, None));
        assert!(rej.is_empty());
        assert_eq!(creds_found(&p, SAMPLE).len(), 1);
    }

    #[test]
    fn installed_patterns_round_trip() {
        // Snapshot, swap, restore - the global is process-wide, so leaving it
        // modified would leak into whatever test runs next.
        let before = current_patterns();
        let (p, _) =
            patterns_from_definitions(&defs(Some(r"z=([0-9a-f]{24})&q=([0-9]{6,})"), None, None));
        install_patterns(p);
        assert_eq!(
            creds_found(&current_patterns(), b"z=0123456789abcdef01234567&q=424242 ").len(),
            1
        );
        install_patterns(ScanPatterns {
            cred: before.cred.clone(),
            build: before.build.clone(),
            ct: before.ct.clone(),
        });
        assert_eq!(creds_found(&current_patterns(), SAMPLE).len(), 1);
    }

    #[test]
    fn definitions_parse_from_the_wire_shape() {
        let d: ScanDefinitions = serde_json::from_str(
            r#"{"version":2,"cred_pattern":"acct=([0-9a-f]{24})&n=([0-9]{6,})"}"#,
        )
        .unwrap();
        assert_eq!(d.version, Some(2));
        assert!(d.build_pattern.is_none(), "absent fields must not error");
        let (_, rej) = patterns_from_definitions(&d);
        assert!(rej.is_empty());
    }

    #[test]
    fn an_alternation_pattern_is_refused_and_cannot_abort_a_scan() {
        // A "DE rotated the parameter name" push written as a top-level
        // alternation. Only one side's groups participate in any given match, so
        // the other side's `cap[1]`/`cap[2]` are absent.
        let hostile =
            r"accountId=([0-9a-fA-F]{24})&nonce=([0-9]{6,})|acct=([0-9a-fA-F]{24})&n=([0-9]{6,})";

        // Layer one: refused at install, falling back to the compiled-in pattern.
        let (p, rej) = patterns_from_definitions(&defs(Some(hostile), None, None));
        assert_eq!(rej.len(), 1, "the pattern must be refused: {rej:?}");
        assert_eq!(creds_found(&p, SAMPLE).len(), 1, "fell back to the default");

        // Layer two: if such a pattern reaches the match loop anyway - a caller
        // that bypassed validation, or a shape this analysis does not foresee -
        // the loop must still not abort. Constructed directly, on purpose.
        let hostile_patterns = ScanPatterns {
            cred: Regex::new(hostile).expect("the pattern compiles"),
            build: p.build.clone(),
            ct: p.ct.clone(),
        };
        let mut counts = PatternCounts::default();
        aggregate_match(
            b"acct=0123456789abcdef01234567&n=123456 ",
            &hostile_patterns,
            &mut counts,
            &ScanBudget::default(),
        )
        .expect("within budget");

        // It must survive, and must not fabricate a credential from the groups
        // that were absent.
        assert!(
            counts.creds.is_empty(),
            "a match with absent required groups must not yield a credential: {:?}",
            counts.creds
        );
    }

    #[test]
    fn a_substituted_pattern_that_never_yields_creds_stays_scannable() {
        // Whatever the pattern does, the other patterns and the fallback path
        // must keep working - the definitions file is remote input.
        let (p, _) = patterns_from_definitions(&defs(
            Some(r"(?:z=([0-9a-f]{24}))?(?:&q=([0-9]{6,}))?"),
            None,
            None,
        ));
        let mut counts = PatternCounts::default();
        aggregate_match(b"nothing to see here ", &p, &mut counts, &ScanBudget::default())
            .expect("within budget");
        assert!(counts.creds.is_empty());
        assert!(p.ct.is_match(SAMPLE), "untouched patterns still work");
    }

    #[test]
    fn too_many_matches_fail_the_scan_instead_of_returning_partial_data() {
        // The match ceiling is what bounds the quadratic iteration: a pattern
        // that matches constantly must stop the scan, not quietly return the
        // matches it happened to reach first.
        let (p, _) = patterns_from_definitions(&ScanDefinitions::default());
        let mut counts = PatternCounts::default();
        let hay = b"&ct=STM ".repeat(64);
        let budget = ScanBudget {
            max_matches: 8,
            ..ScanBudget::default()
        };
        let err = aggregate_match(&hay, &p, &mut counts, &budget)
            .expect_err("the match ceiling must stop the scan");
        assert!(format!("{err:#}").contains("budget"), "{err:#}");
    }

    #[test]
    fn an_overlong_capture_fails_the_scan() {
        // A captured value is arbitrarily long, so a match count alone is not a
        // memory bound.
        let (p, _) = patterns_from_definitions(&ScanDefinitions::default());
        let mut counts = PatternCounts::default();
        let budget = ScanBudget {
            max_capture_len: 4,
            ..ScanBudget::default()
        };
        let err = aggregate_match(SAMPLE, &p, &mut counts, &budget)
            .expect_err("an over-long capture must stop the scan");
        assert!(format!("{err:#}").contains("budget"), "{err:#}");
    }

    #[test]
    fn too_many_distinct_entries_fail_the_scan() {
        let (p, _) = patterns_from_definitions(&ScanDefinitions::default());
        let mut counts = PatternCounts::default();
        let mut hay: Vec<u8> = Vec::new();
        for i in 0..16u32 {
            hay.extend_from_slice(
                format!("accountId={:024x}&nonce={:06} ", i, 100_000 + i).as_bytes(),
            );
        }
        let budget = ScanBudget {
            max_entries: 4,
            ..ScanBudget::default()
        };
        let err = aggregate_match(&hay, &p, &mut counts, &budget)
            .expect_err("the entry ceiling must stop the scan");
        assert!(format!("{err:#}").contains("budget"), "{err:#}");
    }

    #[test]
    fn an_exhausted_time_budget_fails_the_scan() {
        let (p, _) = patterns_from_definitions(&ScanDefinitions::default());
        let mut counts = PatternCounts::default();
        let budget = ScanBudget {
            max_elapsed: Duration::ZERO,
            ..ScanBudget::default()
        };
        let err = aggregate_match(SAMPLE, &p, &mut counts, &budget)
            .expect_err("an exhausted time budget must stop the scan");
        assert!(format!("{err:#}").contains("budget"), "{err:#}");
    }

    #[test]
    fn a_captured_value_that_is_not_an_account_id_is_rejected() {
        // A definitions file can loosen a pattern until it captures anything,
        // and these values do not stay local: the captured build becomes the
        // request's User-Agent and `appVersion`, and the platform tag becomes a
        // request parameter. A shape DE never produces must not be forwarded.
        let mut counts = PatternCounts::default();
        counts.creds.insert(
            (
                "not-an-account-id\r\nX-Injected: 1".to_string(),
                "123456".to_string(),
            ),
            1,
        );
        let Err(err) = pick_dominant(counts) else {
            panic!("a malformed capture must be refused");
        };
        assert!(format!("{err:#}").contains("accountId"), "{err:#}");
    }

    #[test]
    fn metadata_that_is_not_the_expected_shape_is_dropped() {
        let mut counts = PatternCounts::default();
        counts.creds.insert(
            ("0123456789abcdef01234567".to_string(), "123456".to_string()),
            1,
        );
        counts
            .builds
            .insert("evil\r\nUser-Agent: injected".to_string(), 1);
        counts.cts.insert("toolongtag".to_string(), 1);
        let info = pick_dominant(counts).expect("the credentials themselves are valid");
        assert_eq!(info.build, None, "a build label must look like a version");
        assert_eq!(info.ct, "STM", "an implausible platform tag falls back");
    }

    #[test]
    fn valid_metadata_survives_validation() {
        // The guard must not reject what DE actually produces.
        let mut counts = PatternCounts::default();
        counts.creds.insert(
            ("0123456789abcdef01234567".to_string(), "123456".to_string()),
            3,
        );
        counts.builds.insert("38.1.2".to_string(), 2);
        counts.cts.insert("STM".to_string(), 2);
        let info = pick_dominant(counts).expect("valid input");
        assert_eq!(info.build.as_deref(), Some("38.1.2"));
        assert_eq!(info.ct, "STM");
    }

    #[test]
    fn an_exhausted_time_budget_fails_even_when_nothing_matches() {
        // The clock must not depend on a match being yielded. A pattern that is
        // slow precisely because it finds nothing is the shape a hostile
        // definition takes, and checking only per yielded match would never see
        // it.
        let (p, _) = patterns_from_definitions(&ScanDefinitions::default());
        let mut counts = PatternCounts::default();
        let budget = ScanBudget {
            max_elapsed: Duration::ZERO,
            ..ScanBudget::default()
        };
        let hay = b"nothing here matches the default patterns at all ".repeat(4);
        let err = aggregate_match(&hay, &p, &mut counts, &budget)
            .expect_err("the clock must be checked even with no matches");
        assert!(format!("{err:#}").contains("budget"), "{err:#}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_ptrace_guidance_never_advises_running_the_app_as_root() {
        // The app is a networked GUI holding WFM credentials. Suggesting it be
        // launched under sudo trades one memory-read permission for root over
        // the whole session, including whatever the webview renders.
        let err = ptrace_open_error(
            "/proc/4242/mem",
            4242,
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
        );
        let msg = format!("{err:#}");
        let bin = std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(str::to_owned))
            .unwrap_or_else(|| "tennoworth-desktop".to_string());
        assert!(
            !msg.contains(&format!("sudo {bin}")),
            "the guidance must not suggest launching the app itself as root: {msg}"
        );
        // The narrow route is the whole point of the message, so it must stay.
        assert!(
            msg.contains("sudo setcap cap_sys_ptrace=eip"),
            "the per-binary capability route must survive: {msg}"
        );
    }

    #[test]
    fn a_top_level_alternation_is_refused_at_install() {
        // Groups 1 and 2 exist in the pattern, but only one branch supplies them,
        // so a match against the other branch leaves them absent. Counting capture
        // groups is not the same as proving they participate.
        let (p, rej) = patterns_from_definitions(&defs(
            Some(
                r"accountId=([0-9a-fA-F]{24})&nonce=([0-9]{6,})|acct=([0-9a-fA-F]{24})&n=([0-9]{6,})",
            ),
            None,
            None,
        ));
        assert_eq!(rej.len(), 1, "the pattern must be refused: {rej:?}");
        assert!(rej[0].reason.contains("guaranteed"), "{}", rej[0].reason);
        assert_eq!(creds_found(&p, SAMPLE).len(), 1, "fell back to the default");
    }

    #[test]
    fn an_optional_group_is_refused_at_install() {
        for raw in [
            r"accountId=([0-9a-fA-F]{24})?&nonce=([0-9]{6,})",
            r"(?:accountId=([0-9a-fA-F]{24}))?&nonce=([0-9]{6,})",
        ] {
            let (_, rej) = patterns_from_definitions(&defs(Some(raw), None, None));
            assert_eq!(rej.len(), 1, "{raw} must be refused: {rej:?}");
        }
    }

    #[test]
    fn patterns_whose_groups_always_participate_are_accepted() {
        // The guard must not refuse the correct way to write a rotated name: a
        // rotation belongs in a non-capturing group, where both alternatives sit
        // inside the same capture and it participates either way.
        for raw in [
            r"acct=([0-9a-f]{24})&n=([0-9]{6,})",
            r"(?:accountId|acct)=([0-9a-fA-F]{24})&(?:nonce|n)=([0-9]{6,})",
            r"acct=([0-9a-f]{0,24})&n=([0-9]{6,})",
        ] {
            let (_, rej) = patterns_from_definitions(&defs(Some(raw), None, None));
            assert!(rej.is_empty(), "{raw} must be accepted: {rej:?}");
        }
    }
}
