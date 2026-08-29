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
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};
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

/// Compile a definitions file into a usable pattern set.
///
/// Each pattern is validated INDEPENDENTLY and falls back to the compiled-in
/// default on any problem, so one bad entry cannot disable scanning wholesale -
/// the point of shipping this is to fix a broken scan without a release, and a
/// remote file that can brick the scanner would defeat that.
///
/// Validation is three checks, and the arity one is not cosmetic: the match
/// loop indexes `cap[1]` and `cap[2]`, so a pattern with too few capture groups
/// would panic mid-scan on the user's machine.
///
/// ReDoS is not among the risks - the `regex` crate has no backtracking and is
/// linear in input size - but the length cap still bounds what we agree to
/// compile.
pub fn patterns_from_definitions(defs: &ScanDefinitions) -> (ScanPatterns, Vec<PatternRejection>) {
    let mut rejections = Vec::new();
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
                rejections.push(PatternRejection { field, reason: format!("does not compile: {e}") });
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
        compiled
    }

    let cred = build_one("cred_pattern", defs.cred_pattern.as_ref(), 2, default.cred, &mut rejections);
    let build = build_one("build_pattern", defs.build_pattern.as_ref(), 1, default.build, &mut rejections);
    let ct = build_one("ct_pattern", defs.ct_pattern.as_ref(), 1, default.ct, &mut rejections);

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

fn aggregate_match(haystack: &[u8], pats: &ScanPatterns, counts: &mut PatternCounts) {
    for cap in pats.cred.captures_iter(haystack) {
        let aid = String::from_utf8_lossy(&cap[1]).to_ascii_lowercase();
        let nonce = String::from_utf8_lossy(&cap[2]).into_owned();
        *counts.creds.entry((aid, nonce)).or_insert(0) += 1;
    }
    for cap in pats.build.captures_iter(haystack) {
        *counts
            .builds
            .entry(String::from_utf8_lossy(&cap[1]).into_owned())
            .or_insert(0) += 1;
    }
    for cap in pats.ct.captures_iter(haystack) {
        *counts
            .cts
            .entry(String::from_utf8_lossy(&cap[1]).into_owned())
            .or_insert(0) += 1;
    }
}

#[derive(Default)]
struct PatternCounts {
    creds: HashMap<(String, String), usize>,
    builds: HashMap<String, usize>,
    cts: HashMap<String, usize>,
}

fn pick_dominant(counts: PatternCounts) -> Result<SessionInfo> {
    let total_distinct = counts.creds.len();
    let ((aid, nonce), hits) = match counts.creds.into_iter().max_by_key(|(_, v)| *v) {
        Some(pair) => pair,
        None => bail!(
            "No accountId/nonce pair found in WF memory.\n\
             Make sure you're past the login screen and a recent network\n\
             call has fired (opening the trade or profile screen is reliable)."
        ),
    };
    let build = counts
        .builds
        .into_iter()
        .max_by_key(|(_, v)| *v)
        .map(|(k, _)| k);
    let ct = counts
        .cts
        .into_iter()
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
    let mut mem_file =
        File::open(&mem_path).map_err(|e| ptrace_open_error(&mem_path, pid, e))?;

    let mut counts = PatternCounts::default();
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
        let addr_range = parts[0];
        let perms = parts[1];
        let path = if parts.len() >= 6 { parts[5] } else { "" };
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
            let n = match mem_file.read(&mut hay[tail_len..tail_len + want]) {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };
            let total = tail_len + n;
            aggregate_match(&hay[..total], &pats, &mut counts);
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
//   AppImage (the only Linux channel we ship) - `setcap` is useless here. The
//     runtime mounts the payload on a fresh nosuid FUSE mount per launch, and
//     the kernel ignores file capabilities on nosuid mounts; even if it did
//     not, `current_exe()` is a /tmp/.mount_* path that ceases to exist when
//     the app closes, so the grant could not outlive one run. The honest fix
//     is to relax Yama.
//
//   Anything else (cargo run, a distro package built from source) - the
//     per-binary capability is still the tightest grant available, so keep it.
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
        Some(img) => format!(
            "Permission denied reading {mem_path} - reading the game's memory needs \
             permission to ptrace it.\n\
             `setcap` does not work for an AppImage: it runs from a temporary mount that \
             ignores file capabilities, and the path changes every launch.\n\
             Allow same-user ptrace instead:\n  \
             sudo sysctl kernel.yama.ptrace_scope=0\n\
             To keep it across reboots:\n  \
             echo 'kernel.yama.ptrace_scope=0' | sudo tee /etc/sysctl.d/10-tennoworth.conf\n\
             Or run this one launch with sudo:\n  \
             sudo \"{img}\""
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
                 Or run this one invocation with sudo:\n  \
                 sudo {bin}\n\
                 Note: re-installing or rebuilding the binary clears the capability - re-run setcap after an upgrade."
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
        let handle: HANDLE = OpenProcess(
            PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
            BOOL(0),
            pid,
        )
        .context("OpenProcess failed - not running as same user, or pid is wrong")?;

        let mut counts = PatternCounts::default();
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
            let q = VirtualQueryEx(
                handle,
                Some(addr as *const _),
                &mut mbi,
                mbi_size,
            );
            if q == 0 {
                break;
            }
            let base = mbi.BaseAddress as usize;
            let next = base.wrapping_add(mbi.RegionSize);
            let readable = mbi.State == MEM_COMMIT
                && (mbi.Protect.0 & (PAGE_NOACCESS.0 | PAGE_GUARD.0)) == 0;
            // A zero-sized or wrapping region would make `next` never advance;
            // it is skipped and the walk bails below.
            if readable && next > base {
                let end = next;
                let mut offset = base;
                let mut tail_len = 0usize;
                while offset < end {
                    let want = std::cmp::min(CHUNK, end - offset);
                    let mut read_n: usize = 0;
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
                    aggregate_match(&hay[..total], &pats, &mut counts);
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

    /// The scan reads cap[1]/cap[2]; this mirrors that so a pattern which would
    /// panic mid-scan fails here instead.
    fn creds_found(p: &ScanPatterns, hay: &[u8]) -> Vec<(String, String)> {
        p.cred
            .captures_iter(hay)
            .map(|c| {
                (
                    String::from_utf8_lossy(&c[1]).into_owned(),
                    String::from_utf8_lossy(&c[2]).into_owned(),
                )
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
        assert!(rej[0].reason.contains("does not compile"), "{}", rej[0].reason);
        // Fell back, so scanning still works rather than dying.
        assert_eq!(creds_found(&p, SAMPLE).len(), 1);
    }

    #[test]
    fn too_few_capture_groups_is_refused() {
        // Compiles fine, but the match loop indexes cap[2] - accepting this
        // would panic mid-scan on the user's machine.
        let (p, rej) = patterns_from_definitions(&defs(Some(r"accountId=([0-9a-f]{24})"), None, None));
        assert_eq!(rej.len(), 1);
        assert!(rej[0].reason.contains("needs 2 capture group"), "{}", rej[0].reason);
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
        assert_eq!(creds_found(&p, SAMPLE).len(), 1, "cred fell back and still works");
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
        let (p, _) = patterns_from_definitions(&defs(Some(r"z=([0-9a-f]{24})&q=([0-9]{6,})"), None, None));
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
}
