//! Game process detection + cross-platform memory scanning.
//!
//! `scan_session(pid)` is implemented twice, gated by `#[cfg(target_os = …)]`:
//! Linux walks `/proc/<pid>/maps` and seek+reads `/proc/<pid>/mem`; Windows
//! walks regions with `VirtualQueryEx` and reads them with `ReadProcessMemory`.
//! Both feed the same regex aggregation and dominant-pair pick.

use anyhow::{anyhow, bail, Result};
use regex::bytes::Regex;
use std::collections::HashMap;
use sysinfo::System;

/// The session secrets + build metadata scraped out of the running game.
///
/// Fields are session secrets while a play session is live — never print
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

fn cred_re() -> Regex {
    // Confirmed in May 2026 memory scan: this exact form appears in the URLs
    // the game sends. Update here if DE ever rotates the parameter names.
    // ASCII [0-9] (not \d) so we don't need the regex crate's unicode-perl
    // feature — saves ~150 KB on the binary.
    Regex::new(r"accountId=([0-9a-fA-F]{24})&nonce=([0-9]{6,})").unwrap()
}

fn build_re() -> Regex {
    Regex::new(r#""BuildLabel":"([0-9.]+)/[A-Za-z0-9]+"#).unwrap()
}

fn ct_re() -> Regex {
    Regex::new(r"&ct=([A-Z]{2,4})\b").unwrap()
}

fn aggregate_match<'a>(haystack: &'a [u8], counts: &mut PatternCounts) {
    for cap in cred_re().captures_iter(haystack) {
        let aid = String::from_utf8_lossy(&cap[1]).to_ascii_lowercase();
        let nonce = String::from_utf8_lossy(&cap[2]).into_owned();
        *counts.creds.entry((aid, nonce)).or_insert(0) += 1;
    }
    for cap in build_re().captures_iter(haystack) {
        *counts
            .builds
            .entry(String::from_utf8_lossy(&cap[1]).into_owned())
            .or_insert(0) += 1;
    }
    for cap in ct_re().captures_iter(haystack) {
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
    if counts.creds.is_empty() {
        bail!(
            "No accountId/nonce pair found in WF memory.\n\
             Make sure you're past the login screen and a recent network\n\
             call has fired (opening the trade or profile screen is reliable)."
        );
    }
    let total_distinct = counts.creds.len();
    let ((aid, nonce), hits) = counts
        .creds
        .into_iter()
        .max_by_key(|(_, v)| *v)
        .expect("non-empty checked above");
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

    let maps_path = format!("/proc/{pid}/maps");
    let mem_path = format!("/proc/{pid}/mem");

    let maps_file = File::open(&maps_path)
        .with_context(|| format!("cannot open {maps_path} — does PID {pid} exist?"))?;
    let mut mem_file =
        File::open(&mem_path).map_err(|e| ptrace_open_error(&mem_path, pid, e))?;

    let mut counts = PatternCounts::default();
    let mut buf = vec![0u8; 4 * 1024 * 1024];
    let mut tail: Vec<u8> = Vec::new();
    let overlap = 96;

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
        tail.clear();
        while offset < end {
            let want = std::cmp::min(buf.len() as u64, end - offset) as usize;
            if mem_file.seek(SeekFrom::Start(offset)).is_err() {
                break;
            }
            let n = match mem_file.read(&mut buf[..want]) {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };
            // Concatenate small overlap from previous chunk so a pattern
            // straddling the boundary still matches.
            let mut hay: Vec<u8> = Vec::with_capacity(tail.len() + n);
            hay.extend_from_slice(&tail);
            hay.extend_from_slice(&buf[..n]);
            aggregate_match(&hay, &mut counts);
            tail.clear();
            let keep = std::cmp::min(overlap, n);
            tail.extend_from_slice(&buf[n - keep..n]);
            offset += n as u64;
        }
    }

    pick_dominant(counts)
}

// Turn a /proc/<pid>/mem open failure into actionable guidance. Permission
// denied is the common case (no CAP_SYS_PTRACE) and we lead with the
// grant-once setcap path so users never need sudo again; anything else
// usually means the PID exited between lookup and read.
#[cfg(target_os = "linux")]
fn ptrace_open_error(mem_path: &str, pid: u32, e: std::io::Error) -> anyhow::Error {
    if e.kind() != std::io::ErrorKind::PermissionDenied {
        return anyhow!(
            "cannot open {mem_path}: {e}\n\
             PID {pid} may have exited — restart Warframe past the title screen and retry."
        );
    }
    let bin = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_owned))
        .unwrap_or_else(|| "wfm-fetch-inventory".to_string());
    let mut msg = format!(
        "Permission denied reading {mem_path} — reading the game's memory needs CAP_SYS_PTRACE.\n\
         Grant it once (no sudo needed afterwards):\n  \
         sudo setcap cap_sys_ptrace=eip \"{bin}\"\n  \
         {bin}\n\
         Or run this one invocation with sudo:\n  \
         sudo {bin}\n\
         Note: re-installing or rebuilding the binary clears the capability — re-run setcap after an upgrade."
    );
    // Yama scope 3 disables ptrace entirely; even a capable binary can't attach.
    if matches!(
        std::fs::read_to_string("/proc/sys/kernel/yama/ptrace_scope"),
        Ok(s) if s.trim() == "3"
    ) {
        msg.push_str(
            "\n\nkernel.yama.ptrace_scope is 3 (ptrace disabled) — setcap alone won't help.\n\
             Lower it until reboot:\n  sudo sysctl kernel.yama.ptrace_scope=1",
        );
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

    unsafe {
        let handle: HANDLE = OpenProcess(
            PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
            BOOL(0),
            pid,
        )
        .context("OpenProcess failed — not running as same user, or pid is wrong")?;

        let mut counts = PatternCounts::default();
        let mut addr: usize = 0;
        let mut mbi = MEMORY_BASIC_INFORMATION::default();
        let mbi_size = std::mem::size_of::<MEMORY_BASIC_INFORMATION>();

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
            let next = mbi.BaseAddress as usize + mbi.RegionSize;
            let readable = mbi.State == MEM_COMMIT
                && (mbi.Protect.0 & (PAGE_NOACCESS.0 | PAGE_GUARD.0)) == 0;
            if readable {
                let mut buf = vec![0u8; mbi.RegionSize];
                let mut read_n: usize = 0;
                let ok = ReadProcessMemory(
                    handle,
                    mbi.BaseAddress,
                    buf.as_mut_ptr() as *mut _,
                    mbi.RegionSize,
                    Some(&mut read_n),
                );
                if ok.is_ok() && read_n > 0 {
                    aggregate_match(&buf[..read_n], &mut counts);
                }
            }
            addr = next;
            if addr == 0 {
                break;
            }
        }

        let _ = CloseHandle(handle);
        pick_dominant(counts)
    }
}
