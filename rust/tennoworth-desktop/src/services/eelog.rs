//! EE.log trade detection - the game's own log, read-only, tailed.
//!
//! When a trade completes, Warframe writes the confirmation dialog it showed
//! ("Are you sure you want to accept this trade? You are offering: … and will
//! receive from <partner> the following: …") and, on success, "The trade was
//! successful!". Reading those two lines is enough to know exactly what was
//! sold, to whom, for how much - the ground truth a profit ledger needs and
//! the trigger a "close the WFM listing I just sold" automation needs. It is
//! a plain file read; nothing is injected and nothing touches the process.
//!
//! Layout: [`parse_trade_dialog`] and [`TradeMachine`] are pure and tested
//! against captured line shapes; [`locate_log`] knows the Windows and
//! Steam/Proton paths; [`tail_forever_with_lines`] is the loop that polls the file
//! and hands confirmed trades to a callback.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use crate::trading_contract::{LogPosition, TradeEvent, TradeItem};

fn file_position(file: &mut std::fs::File) -> Option<LogPosition> {
    let meta = file.metadata().ok()?;
    // The startup timestamp identifies a game session across copies/replays.
    // File birth time cannot: replacing the same log would recount its trades.
    if meta.len() < 4096 {
        return None;
    }
    file.seek(SeekFrom::Start(0)).ok()?;
    let mut prefix = [0u8; 4096];
    file.read_exact(&mut prefix).ok()?;
    if !String::from_utf8_lossy(&prefix)
        .lines()
        .any(|line| line.contains("Sys [Diag]: Current time:") && line.contains("[UTC:"))
    {
        return None;
    }
    Some(LogPosition {
        session: wfm_core::identity::local_fingerprint("tennoworth-eelog-v1", &prefix),
        start: meta.len(),
        end: meta.len(),
        observed_after: crate::services::allowance::unix_now(),
    })
}

pub fn log_position(path: &Path) -> Option<LogPosition> {
    file_position(&mut std::fs::File::open(path).ok()?)
}

pub const DIALOG_START: &str = "Are you sure you want to accept this trade?";
pub const TRADE_SUCCESS: &str = "The trade was successful!";
/// A dialog that never resolves within this is discarded (declined trade).
pub const DIALOG_TIMEOUT: Duration = Duration::from_secs(120);

/// Platform glyphs the game appends to names (PC/PSN/XBOX/NSW/iOS markers
/// live in the Private Use Area) - stripped so partner/item names compare.
fn strip_glyphs(s: &str) -> String {
    s.chars()
        .filter(|c| {
            !('\u{e000}'..='\u{f8ff}').contains(c) && !('\u{f0000}'..='\u{ffffd}').contains(c)
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// A line the log framework itself wrote (`123.456 Sys [Info]: …`) - must
/// never be read as an item name when it interleaves with a dialog dump.
fn is_framework_line(line: &str) -> bool {
    let t = line.trim_start();
    let mut it = t.splitn(2, ' ');
    let stamp = it.next().unwrap_or("");
    let rest = it.next().unwrap_or("");
    stamp.parse::<f64>().is_ok()
        && (rest.contains("[Info]") || rest.contains("[Error]") || rest.contains("[Warning]"))
}

/// Cut a `, leftItem=/Menu/…` or `title=` argument tail glued to the last
/// item line of a single-line dialog dump.
#[allow(
    clippy::string_slice,
    reason = "every bound comes from an ASCII marker returned by find and is a UTF-8 boundary"
)]
fn strip_arg_tail(line: &str) -> &str {
    let mut end = line.len();
    for key in [
        ", leftItem=",
        " leftItem=",
        ", rightItem=",
        " rightItem=",
        ", title=",
        " title=",
    ] {
        if let Some(i) = line.find(key) {
            end = end.min(i);
        }
    }
    &line[..end]
}

/// One side of the dialog: item lines and the plat total among them.
fn parse_item_block(block: &str, direction: &str) -> (Vec<TradeItem>, i64) {
    let mut plat = 0i64;
    let mut items: Vec<TradeItem> = Vec::new();
    for raw in block.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("leftItem=")
            || line.starts_with("rightItem=")
            || line.starts_with("title=")
        {
            break;
        }
        if is_framework_line(line) {
            continue;
        }
        let cleaned = strip_glyphs(strip_arg_tail(line).trim_end_matches('\r'));
        if cleaned.is_empty() {
            continue;
        }
        // "Platinum" / "Platinum x 40"
        if let Some(rest) = cleaned.strip_prefix("Platinum") {
            let rest = rest.trim();
            if rest.is_empty() {
                plat += 1;
                continue;
            }
            if let Some(n) = rest
                .strip_prefix('x')
                .and_then(|n| n.trim().parse::<i64>().ok())
            {
                plat += n;
                continue;
            }
        }
        // "Name x N" for stacks; single items repeat one line each.
        let (name, qty) = match cleaned.rsplit_once(" x ") {
            Some((n, q)) if q.trim().parse::<i64>().is_ok() => {
                (n.trim().to_string(), q.trim().parse::<i64>().unwrap_or(1))
            }
            _ => (cleaned.clone(), 1),
        };
        if let Some(existing) = items.iter_mut().find(|i| i.name == name) {
            existing.qty += qty;
        } else {
            items.push(TradeItem {
                name,
                qty,
                direction: direction.into(),
            });
        }
    }
    (items, plat)
}

/// Parse a buffered dialog (one or more log lines) into a trade, or `None`
/// when the lines are not a trade dialog.
#[allow(
    clippy::string_slice,
    reason = "every bound comes from an ASCII marker returned by find and is a UTF-8 boundary"
)]
pub fn parse_trade_dialog(lines: &[String]) -> Option<TradeEvent> {
    let text = lines.join("\n");
    let start = text.find("You are offering:")?;
    let desc = &text[start..];
    let divider_re_start = desc.find("and will receive from")?;
    let after = &desc[divider_re_start + "and will receive from".len()..];
    let following = after.find("the following:")?;
    let partner = strip_glyphs(after[..following].trim());
    let offering_block = &desc["You are offering:".len()..divider_re_start];
    let receiving_block = &after[following + "the following:".len()..];

    let (mut given, plat_spent) = parse_item_block(offering_block, "given");
    let (received, plat_gained) = parse_item_block(receiving_block, "received");
    given.extend(received);

    let kind = if plat_gained > 0 && plat_spent == 0 {
        "sale"
    } else if plat_spent > 0 && plat_gained == 0 {
        "purchase"
    } else {
        "trade"
    };
    let stamp = lines
        .first()
        .and_then(|l| l.split_whitespace().next())
        .filter(|s| s.parse::<f64>().is_ok())
        .map(String::from);
    Some(TradeEvent {
        partner,
        kind: kind.into(),
        plat: plat_gained.max(plat_spent),
        items: given,
        log_stamp: stamp,
    })
}

/// Line-at-a-time state machine: buffers the dialog dump from
/// [`DIALOG_START`] until the next framework line, then emits a
/// [`TradeEvent`] when [`TRADE_SUCCESS`] follows. Time-agnostic - the caller
/// passes `now_ms` so tests don't sleep.
#[derive(Default)]
pub struct TradeMachine {
    buffer: Option<Vec<String>>,
    sealed: bool,
    started_ms: u64,
}

impl TradeMachine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one line; returns a confirmed trade when the success line lands.
    pub fn feed(&mut self, line: &str, now_ms: u64) -> Option<TradeEvent> {
        if line.contains(DIALOG_START) {
            self.buffer = Some(vec![line.to_string()]);
            self.started_ms = now_ms;
            // A single-line dump already carries the whole dialog; the next
            // framework line seals a multi-line one.
            self.sealed = line.contains("leftItem=") || line.contains("rightItem=");
        } else if let Some(buf) = self.buffer.as_mut() {
            if now_ms.saturating_sub(self.started_ms) > DIALOG_TIMEOUT.as_millis() as u64 {
                self.buffer = None;
                self.sealed = false;
            } else if is_framework_line(line) {
                self.sealed = true;
            } else if !self.sealed {
                buf.push(line.to_string());
            }
        }
        if line.contains(TRADE_SUCCESS) {
            if let Some(buf) = self.buffer.take() {
                self.sealed = false;
                return parse_trade_dialog(&buf);
            }
        }
        None
    }
}

/// Where the game writes EE.log.
///   Windows: `%LOCALAPPDATA%\Warframe\EE.log`
///   Linux (Steam/Proton): `<library>/steamapps/compatdata/230410/pfx/drive_c/
///     users/steamuser/AppData/Local/Warframe/EE.log`, for every Steam library
///     listed in `libraryfolders.vdf` under the usual Steam roots.
/// `TENNOWORTH_EELOG` overrides everything (tests, unusual installs).
pub fn locate_log() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("TENNOWORTH_EELOG") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let p = Path::new(&local).join("Warframe").join("EE.log");
            if p.is_file() {
                return Some(p);
            }
        }
        None
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home = wfm_core::platform::dirs_home();
        let roots = [
            home.join(".local/share/Steam"),
            home.join(".steam/steam"),
            home.join(".steam/root"),
            home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
            home.join("snap/steam/common/.local/share/Steam"),
        ];
        let mut libraries: Vec<PathBuf> = Vec::new();
        for root in roots.iter() {
            if !root.is_dir() {
                continue;
            }
            libraries.push(root.clone());
            if let Ok(vdf) = std::fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) {
                for lib in parse_steam_library_paths(&vdf) {
                    libraries.push(PathBuf::from(lib));
                }
            }
        }
        for lib in libraries {
            let p = proton_log_path(&lib);
            if p.is_file() {
                return Some(p);
            }
        }
        None
    }
}

/// Steam/Proton library layout, used by the non-Windows EE.log discovery.
/// `test` keeps it compiled on Windows, where only its unit test refers to it.
#[cfg(any(not(target_os = "windows"), test))]
pub fn proton_log_path(steam_library: &Path) -> PathBuf {
    steam_library.join(
        "steamapps/compatdata/230410/pfx/drive_c/users/steamuser/AppData/Local/Warframe/EE.log",
    )
}

/// `"path"  "/mnt/games/SteamLibrary"` lines out of libraryfolders.vdf.
/// `test` keeps it compiled on Windows, where only its unit test refers to it.
#[cfg(any(not(target_os = "windows"), test))]
#[allow(
    clippy::string_slice,
    reason = "the ASCII path marker length is always a UTF-8 boundary"
)]
pub fn parse_steam_library_paths(vdf: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in vdf.lines() {
        let t = line.trim();
        if !t.starts_with("\"path\"") {
            continue;
        }
        let rest = t["\"path\"".len()..].trim();
        let val = rest.trim_matches('"').replace("\\\\", "\\");
        if !val.is_empty() && !out.contains(&val) {
            out.push(val);
        }
    }
    out
}

/// Why the tailer could not account for a stretch of the log.
///
/// The callers do not all want the same thing from a gap: the allowance logic
/// treats every kind as "stop certifying the tracked figure", while the recording
/// health only reports the kinds that mean trades are going unread. Rotation is
/// the ordinary case - the game rewriting its own log - and reporting it as a
/// fault would leave the ledger surface permanently warning about normal play.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapKind {
    /// The log could not be opened, positioned, sought, or decoded, or it grew
    /// past what one poll will buffer. Trades may be happening unobserved.
    Io,
    /// A completed trade the machine could not parse. The log says a trade
    /// happened and we have no record of its contents.
    Unrecognized,
    /// The log was replaced or truncated and re-read from the start. Ordinary.
    Rotated,
}

impl GapKind {
    /// Whether this gap means trades may be going unread.
    pub fn is_unread(self) -> bool {
        !matches!(self, Self::Rotated)
    }

    /// What to tell the user, or `None` for a gap they need not know about.
    pub fn explanation(self) -> Option<&'static str> {
        match self {
            Self::Io => Some("the game log could not be read"),
            Self::Unrecognized => Some("a completed trade in the game log could not be read"),
            Self::Rotated => None,
        }
    }
}

/// Tail `path` forever: start at the current end (past trades are not
/// re-announced), poll every `poll`, handle truncation (game restart writes a
/// fresh file) by re-seeking to 0. Each confirmed trade goes to `on_trade`,
/// which reports whether the ledger accepted it, and each stretch the tailer
/// could not account for goes to `on_gap` with the reason. After a gap that
/// left the log unreadable, the first poll that reads it again calls
/// `on_readable`.
/// Blocking - run on its own thread.
pub fn tail_forever_with_lines(
    path: &Path,
    poll: Duration,
    on_line: impl FnMut(&str),
    on_trade: impl FnMut(TradeEvent, LogPosition) -> bool,
    on_gap: impl FnMut(GapKind),
    on_readable: impl FnMut(),
) {
    let start = std::time::Instant::now();
    tail_with_ticks(
        path,
        || {
            std::thread::sleep(poll);
            Some((
                start.elapsed().as_millis() as u64,
                crate::services::allowance::unix_now(),
            ))
        },
        on_line,
        on_trade,
        on_gap,
        on_readable,
    );
}

/// The tailer loop with its tick source injected: the production entry point
/// supplies a sleeping tick and the real clock, and a test supplies a finite
/// script of clock readings and file appends. A tick that returns `None` ends the
/// otherwise-forever loop, which is what keeps a broken retry a failure rather
/// than a hang.
fn tail_with_ticks(
    path: &Path,
    mut next_tick: impl FnMut() -> Option<(u64, i64)>,
    mut on_line: impl FnMut(&str),
    mut on_trade: impl FnMut(TradeEvent, LogPosition) -> bool,
    mut on_gap: impl FnMut(GapKind),
    mut on_readable: impl FnMut(),
) {
    let mut machine = TradeMachine::new();
    let initial = log_position(path);
    let mut fallback_session = wfm_core::identity::random_token(16);
    let mut offset = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let mut session = initial
        .map(|p| p.session)
        .unwrap_or_else(|| fallback_session.clone());
    let mut remainder = Vec::new();
    let mut line_offset = offset;
    let mut dialog_start = offset;
    let mut observed_after = crate::services::allowance::unix_now();
    let mut dialog_observed_after = observed_after;
    // Trades the ledger has not yet accepted, oldest first. A refused trade is
    // held here and re-offered each poll, and every later trade queues behind
    // it, so none is recorded ahead of an earlier one. Holding the parsed event
    // instead of rewinding the read keeps `on_line` live: the overlay behind it
    // must keep seeing the log while the ledger is down, and its consumer counts
    // slot markers, so no line may reach it twice.
    let mut held: std::collections::VecDeque<(TradeEvent, LogPosition)> =
        std::collections::VecDeque::new();
    // Set by a gap that left the log unreadable, so the next clean read can
    // withdraw the warning it raised.
    let mut unreadable = false;
    while let Some((now_ms, now)) = next_tick() {
        while let Some((trade, position)) = held.pop_front() {
            if !on_trade(trade.clone(), position.clone()) {
                held.push_front((trade, position));
                break;
            }
        }
        let Ok(mut file) = std::fs::File::open(path) else {
            on_gap(GapKind::Io);
            unreadable = true;
            continue;
        };
        // An unidentifiable log is still read, but it is a gap on every poll;
        // reporting recovery in the same poll would flap the warning 4x a second.
        let mut identified = true;
        let position = match file_position(&mut file) {
            Some(position) => position,
            None => {
                on_gap(GapKind::Io);
                unreadable = true;
                identified = false;
                let Ok(meta) = file.metadata() else { continue };
                if meta.len() < offset {
                    fallback_session = wfm_core::identity::random_token(16);
                }
                LogPosition {
                    session: fallback_session.clone(),
                    start: meta.len(),
                    end: meta.len(),
                    observed_after: now,
                }
            }
        };
        // Unrecognized startup headers still feed the ledger/overlay, but
        // scan_boundary cannot certify their accounting identity.
        let gained_identity = session == fallback_session && position.session != fallback_session;
        if position.end < offset || (session != position.session && !gained_identity) {
            on_gap(GapKind::Rotated);
            offset = 0;
            line_offset = 0;
            remainder.clear();
            machine = TradeMachine::new();
        }
        session = position.session.clone();
        if position.end == offset {
            if remainder.is_empty() {
                observed_after = now;
            }
            if identified && std::mem::take(&mut unreadable) {
                on_readable();
            }
            continue;
        }
        if file.seek(SeekFrom::Start(offset)).is_err() {
            on_gap(GapKind::Io);
            unreadable = true;
            continue;
        }
        let mut buf = Vec::new();
        if file.take(1024 * 1024).read_to_end(&mut buf).is_err() {
            on_gap(GapKind::Io);
            unreadable = true;
            continue;
        }
        if identified && std::mem::take(&mut unreadable) {
            on_readable();
        }
        offset += buf.len() as u64;
        remainder.extend_from_slice(&buf);
        let complete_upto = remainder
            .iter()
            .rposition(|b| *b == b'\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let complete: Vec<u8> = remainder.drain(..complete_upto).collect();
        for raw in complete.split_inclusive(|b| *b == b'\n') {
            let beginning = line_offset;
            line_offset += raw.len() as u64;
            let Ok(line) = std::str::from_utf8(raw) else {
                on_gap(GapKind::Io);
                machine = TradeMachine::new();
                continue;
            };
            let line = line.trim_end_matches(['\r', '\n']);
            if line.contains(DIALOG_START) {
                dialog_start = beginning;
                dialog_observed_after = observed_after;
            }
            on_line(line);
            if let Some(t) = machine.feed(line, now_ms) {
                let trade_position = LogPosition {
                    session: position.session.clone(),
                    start: dialog_start,
                    end: line_offset,
                    observed_after: dialog_observed_after,
                };
                if !held.is_empty() || !on_trade(t.clone(), trade_position.clone()) {
                    held.push_back((t, trade_position));
                }
            } else if line.contains(TRADE_SUCCESS) {
                on_gap(GapKind::Unrecognized);
            }
        }
        if remainder.len() > 256 * 1024 {
            on_gap(GapKind::Io);
            remainder.clear();
            line_offset = offset;
            machine = TradeMachine::new();
        }
        // Do not advance the time bound while draining a backlog or retaining
        // a partial line: those bytes may predate a scan or the UTC reset.
        if offset >= position.end && remainder.is_empty() {
            observed_after = now;
        }
    }
}

/// Poll a bounded snapshot independently from the streaming tailer's offset.
/// Proton can replace EE.log and leave a long-lived offset past new events.
pub fn watch_recent_text(path: &Path, poll: Duration, mut on_text: impl FnMut(&str)) {
    const WINDOW: u64 = 128 * 1024;
    let mut previous = None;
    loop {
        std::thread::sleep(poll);
        let Ok(meta) = std::fs::metadata(path) else {
            continue;
        };
        let signature = (meta.len(), meta.modified().ok());
        if previous.as_ref() == Some(&signature) {
            continue;
        }
        previous = Some(signature);
        let Ok(mut file) = std::fs::File::open(path) else {
            continue;
        };
        let start = meta.len().saturating_sub(WINDOW);
        if file.seek(SeekFrom::Start(start)).is_err() {
            continue;
        }
        let mut bytes = Vec::with_capacity((meta.len() - start) as usize);
        if file.read_to_end(&mut bytes).is_err() {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        let text = if start > 0 {
            text.split_once('\n').map(|(_, rest)| rest).unwrap_or("")
        } else {
            &text
        };
        on_text(text);
    }
}

#[cfg(test)]
mod tests {
    /// Rotation is the game rewriting its own log, which happens on every
    /// restart. Treating it as an unread stretch would leave the ledger warning
    /// about ordinary play, so the kind has to carry the distinction rather than
    /// every gap looking alike to the caller.
    #[test]
    fn only_the_gaps_that_lose_trades_count_as_unread() {
        assert!(!GapKind::Rotated.is_unread());
        assert_eq!(GapKind::Rotated.explanation(), None);

        for kind in [GapKind::Io, GapKind::Unrecognized] {
            assert!(kind.is_unread());
            assert!(
                kind.explanation().is_some(),
                "an unread gap needs copy the user can act on"
            );
        }
    }

    use super::*;

    #[test]
    fn log_identity_is_stable_on_append_and_changes_when_the_prefix_is_replaced() {
        use std::io::Write;
        let path = std::env::temp_dir().join(format!(
            "eelog-position-{}",
            wfm_core::identity::random_token(12)
        ));
        let header = "0.1 Sys [Diag]: Current time: Mon Sep 7 10:00:00 2026 [UTC: Mon Sep 7 09:00:00 2026]\n";
        let content = format!("{header}{}", "a".repeat(4096));
        std::fs::write(&path, content.as_bytes()).unwrap();
        let first = log_position(&path).unwrap();
        let copied = path.with_extension("copied");
        std::fs::copy(&path, &copied).unwrap();
        assert_eq!(
            log_position(&copied).unwrap().session,
            first.session,
            "a copied log is the same game session"
        );
        std::fs::remove_file(copied).unwrap();
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all("\nTrade ✓\n".as_bytes()).unwrap();
        drop(file);
        let appended = log_position(&path).unwrap();
        assert_eq!(first.session, appended.session);
        assert!(appended.end > first.end);
        std::fs::write(
            &path,
            format!(
                "{}{}",
                header
                    .replace("10:00:00", "11:00:00")
                    .replace("09:00:00", "10:00:00"),
                "b".repeat(8192)
            ),
        )
        .unwrap();
        let replaced = log_position(&path).unwrap();
        assert_ne!(
            appended.session, replaced.session,
            "replacement can be larger than the previous offset"
        );
        std::fs::write(&path, b"short").unwrap();
        assert!(log_position(&path).is_none());
        std::fs::write(&path, vec![b'x'; 4096]).unwrap();
        assert!(
            log_position(&path).is_none(),
            "no identifiable startup header"
        );
        std::fs::remove_file(path).unwrap();
    }

    fn dialog_single_line() -> String {
        // Real shape: one framework line carrying the whole dialog dump with
        // the Dialog args glued on the end.
        "1234.567 Sys [Info]: Dialog.lua: Dialog::CreateOkCancel(description=Are you sure you want to accept this trade?\nYou are offering:\nPrimed Flow\nLith C5 Relic x 3\nand will receive from SomeTenno\u{e000} the following:\nPlatinum x 45, leftItem=/Menu/Confirm_Item_Ok)".to_string()
    }

    #[test]
    fn parses_a_sale_with_stacks_and_platform_glyph_on_the_partner() {
        let t = parse_trade_dialog(&[dialog_single_line()]).unwrap();
        assert_eq!(t.partner, "SomeTenno");
        assert_eq!(t.kind, "sale");
        assert_eq!(t.plat, 45);
        assert_eq!(t.log_stamp.as_deref(), Some("1234.567"));
        assert_eq!(
            t.items,
            vec![
                TradeItem {
                    name: "Primed Flow".into(),
                    qty: 1,
                    direction: "given".into()
                },
                TradeItem {
                    name: "Lith C5 Relic".into(),
                    qty: 3,
                    direction: "given".into()
                },
            ]
        );
    }

    #[test]
    fn parses_a_purchase_and_a_mixed_trade() {
        let lines = vec![
            "10.0 Sys [Info]: Dialog.lua: Dialog::CreateOkCancel(description=Are you sure you want to accept this trade?".to_string(),
            "You are offering:".into(),
            "Platinum x 20".into(),
            "and will receive from Buyer the following:".into(),
            "Ash Prime Blueprint".into(),
            "Ash Prime Blueprint".into(),
            ", leftItem=/Menu/Confirm_Item_Ok)".into(),
        ];
        let t = parse_trade_dialog(&lines).unwrap();
        assert_eq!(t.kind, "purchase");
        assert_eq!(t.plat, 20);
        assert_eq!(
            t.items,
            vec![TradeItem {
                name: "Ash Prime Blueprint".into(),
                qty: 2,
                direction: "received".into()
            }]
        );

        let lines = vec![
            "10.0 Sys [Info]: …CreateOkCancel(description=Are you sure you want to accept this trade?".to_string(),
            "You are offering:".into(),
            "Primed Flow".into(),
            "and will receive from Other the following:".into(),
            "Primed Continuity".into(),
        ];
        let t = parse_trade_dialog(&lines).unwrap();
        assert_eq!(t.kind, "trade");
        assert_eq!(t.plat, 0);
        assert_eq!(t.items.len(), 2);
    }

    #[test]
    fn framework_lines_leaking_into_a_multiline_dialog_are_ignored() {
        let lines = vec![
            "10.0 Sys [Info]: …(description=Are you sure you want to accept this trade?"
                .to_string(),
            "You are offering:".into(),
            "Primed Flow".into(),
            "10.5 Net [Info]: some unrelated chatter".into(),
            "and will receive from Buyer the following:".into(),
            "Platinum x 30".into(),
        ];
        let t = parse_trade_dialog(&lines).unwrap();
        assert_eq!(t.items.len(), 1);
        assert_eq!(t.plat, 30);
    }

    #[test]
    fn not_a_trade_dialog_is_none() {
        assert!(parse_trade_dialog(&[
            "10.0 Sys [Info]: Dialog: Are you sure you want to sell this item?".to_string()
        ])
        .is_none());
    }

    #[test]
    fn machine_emits_only_after_the_success_line_and_resets() {
        let mut m = TradeMachine::new();
        assert!(m.feed(&dialog_single_line(), 0).is_none());
        assert!(m.feed("1235.0 Sys [Info]: something else", 100).is_none());
        let t = m
            .feed("1236.0 Sys [Info]: The trade was successful!", 200)
            .unwrap();
        assert_eq!(t.plat, 45);
        // A second success line without a new dialog is nothing.
        assert!(m
            .feed("1237.0 Sys [Info]: The trade was successful!", 300)
            .is_none());
    }

    #[test]
    fn machine_buffers_multiline_dialogs_until_the_next_framework_line() {
        let mut m = TradeMachine::new();
        m.feed(
            "10.0 Sys [Info]: …(description=Are you sure you want to accept this trade?",
            0,
        );
        m.feed("You are offering:", 0);
        m.feed("Primed Flow", 0);
        m.feed("and will receive from Buyer the following:", 0);
        m.feed("Platinum x 30", 0);
        m.feed("11.0 Sys [Info]: seals the dialog", 0);
        m.feed("this line must NOT become an item", 0);
        let t = m
            .feed("12.0 Sys [Info]: The trade was successful!", 0)
            .unwrap();
        assert_eq!(
            t.items,
            vec![TradeItem {
                name: "Primed Flow".into(),
                qty: 1,
                direction: "given".into()
            }]
        );
    }

    #[test]
    fn a_declined_dialog_times_out() {
        let mut m = TradeMachine::new();
        m.feed(&dialog_single_line(), 0);
        // 3 minutes later the success line arrives for some OTHER trade whose
        // dialog we never saw - must not pair with the stale buffer.
        assert!(m.feed("x", 3 * 60 * 1000).is_none());
        assert!(m
            .feed(
                "9.0 Sys [Info]: The trade was successful!",
                3 * 60 * 1000 + 1
            )
            .is_none());
    }

    #[test]
    fn steam_library_paths_parse_from_vdf() {
        let vdf = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"/home/me/.local/share/Steam"
	}
	"1"
	{
		"path"		"/mnt/games/SteamLibrary"
	}
}
"#;
        assert_eq!(
            parse_steam_library_paths(vdf),
            vec!["/home/me/.local/share/Steam", "/mnt/games/SteamLibrary"]
        );
        assert!(proton_log_path(Path::new("/mnt/games/SteamLibrary")).ends_with("Warframe/EE.log"));
    }

    fn trade_text(n: u32, partner: &str) -> String {
        format!(
            "{n}0.0 Sys [Info]: Dialog.lua: Dialog::CreateOkCancel(description=Are you sure you want to accept this trade?\n\
             You are offering:\n\
             Primed Flow\n\
             and will receive from {partner} the following:\n\
             Platinum x 45, leftItem=/Menu/Confirm_Item_Ok)\n\
             {n}5.0 Sys [Info]: The trade was successful!\n"
        )
    }

    /// A trade the ledger refuses must be offered again before anything later in
    /// the batch, and the retry must not replay its lines to the line consumer:
    /// the overlay behind `on_line` counts slot markers and triggers captures, so
    /// it is not idempotent.
    #[test]
    fn a_refused_trade_is_retried_before_later_trades_without_replaying_lines() {
        use std::cell::Cell;
        use std::io::Write;

        let path = std::env::temp_dir().join(format!(
            "eelog-ack-{}",
            wfm_core::identity::random_token(12)
        ));
        let header =
            "0.1 Sys [Diag]: Current time: Mon Sep 7 10:00:00 2026 [UTC: Mon Sep 7 09:00:00 2026]\n";
        std::fs::write(&path, format!("{header}{}\n", "a".repeat(4096)).as_bytes()).unwrap();

        let lead = trade_text(1, "Lead");
        let first = trade_text(2, "First");
        let second = trade_text(3, "Second");
        let third = trade_text(4, "Third");
        let cut = third.len() / 2;
        let (third_head, third_tail) = third.split_at(cut);
        let batch = format!("{lead}{first}{second}{third_head}");

        let tick = Cell::new(0usize);
        let mut refusals = 0usize;
        let mut attempts: Vec<(usize, String, LogPosition)> = Vec::new();
        let mut lines: Vec<String> = Vec::new();
        let mut gaps: Vec<GapKind> = Vec::new();
        let now = crate::services::allowance::unix_now();

        tail_with_ticks(
            &path,
            || {
                let next = tick.get() + 1;
                tick.set(next);
                if next > 3 {
                    return None;
                }
                let appended = match next {
                    1 => Some(batch.as_bytes()),
                    3 => Some(third_tail.as_bytes()),
                    _ => None,
                };
                if let Some(bytes) = appended {
                    let mut file = std::fs::OpenOptions::new()
                        .append(true)
                        .open(&path)
                        .expect("append");
                    file.write_all(bytes).expect("append batch");
                }
                Some((next as u64 * 250, now))
            },
            |line| lines.push(line.to_string()),
            |trade, position| {
                let refuse = trade.partner == "First" && refusals < 2;
                if refuse {
                    refusals += 1;
                }
                attempts.push((tick.get(), trade.partner.clone(), position));
                !refuse
            },
            |kind| gaps.push(kind),
            || {},
        );

        std::fs::remove_file(&path).expect("cleanup");

        let order: Vec<(usize, &str)> = attempts
            .iter()
            .map(|(at, partner, _)| (*at, partner.as_str()))
            .collect();
        assert_eq!(
            order,
            vec![
                (1, "Lead"),
                (1, "First"),
                (2, "First"),
                (3, "First"),
                (3, "Second"),
                (3, "Third"),
            ],
            "a refused trade is re-offered before anything later in the batch"
        );

        // The retry must be the same event at the same log position: that identity
        // is what the ledger dedupes on, so offering it again cannot double-record
        // or let an old trade look newer than the scan it predates.
        let retries: Vec<&(usize, String, LogPosition)> = attempts
            .iter()
            .filter(|(_, partner, _)| partner == "First")
            .collect();
        assert_eq!(retries.len(), 3);
        assert_eq!(retries[0].2.start, retries[1].2.start);
        for pair in retries.windows(2) {
            assert_eq!(pair[0].2, pair[1].2);
        }

        // Every line reaches the consumer exactly once, retry or not.
        assert_eq!(
            lines.iter().filter(|l| l.contains(DIALOG_START)).count(),
            4,
            "each dialog line is delivered once"
        );
        assert_eq!(
            lines.iter().filter(|l| l.contains(TRADE_SUCCESS)).count(),
            4,
            "each success line is delivered once"
        );
    }

    /// While the ledger keeps refusing a trade, the log must keep reaching the
    /// line consumer. The overlay behind `on_line` hides itself on the reward
    /// close marker; a tailer that parked at the refused trade silenced it for
    /// as long as the database stayed down.
    #[test]
    fn a_trade_the_ledger_keeps_refusing_does_not_starve_the_line_consumer() {
        use std::cell::Cell;
        use std::io::Write;

        let path = std::env::temp_dir().join(format!(
            "eelog-stuck-{}",
            wfm_core::identity::random_token(12)
        ));
        let header =
            "0.1 Sys [Diag]: Current time: Mon Sep 7 10:00:00 2026 [UTC: Mon Sep 7 09:00:00 2026]\n";
        std::fs::write(&path, header.as_bytes()).unwrap();

        let stuck = trade_text(1, "Stuck");
        let later = trade_text(2, "Later");
        let tick = Cell::new(0usize);
        let mut offered: Vec<String> = Vec::new();
        let mut lines: Vec<String> = Vec::new();
        let now = crate::services::allowance::unix_now();

        tail_with_ticks(
            &path,
            || {
                let next = tick.get() + 1;
                tick.set(next);
                if next > 4 {
                    return None;
                }
                let appended = match next {
                    1 => Some(stuck.clone()),
                    2 => Some("30.0 Sys [Info]: after the refused trade\n".to_string()),
                    3 => Some(later.clone()),
                    _ => None,
                };
                if let Some(text) = appended {
                    let mut file = std::fs::OpenOptions::new()
                        .append(true)
                        .open(&path)
                        .expect("append");
                    file.write_all(text.as_bytes()).expect("append");
                }
                Some((next as u64 * 250, now))
            },
            |line| lines.push(line.to_string()),
            |trade, _| {
                offered.push(trade.partner.clone());
                false
            },
            |_| {},
            || {},
        );

        std::fs::remove_file(&path).expect("cleanup");

        assert!(
            lines.iter().any(|l| l.contains("after the refused trade")),
            "lines written after a refused trade still reach the consumer"
        );
        assert_eq!(
            lines.iter().filter(|l| l.contains(TRADE_SUCCESS)).count(),
            2,
            "each line is delivered once, however often the trade is retried"
        );
        assert!(
            offered.iter().all(|partner| partner == "Stuck"),
            "nothing is offered ahead of the refused trade: {offered:?}"
        );
        assert_eq!(offered.len(), 4, "the refused trade is retried every poll");
    }

    /// An unreadable log raises a warning; the tailer has to say when the log
    /// reads again, or the warning outlives the fault.
    #[test]
    fn a_log_that_reads_again_is_reported_once() {
        use std::cell::Cell;

        let path = std::env::temp_dir().join(format!(
            "eelog-readable-{}",
            wfm_core::identity::random_token(12)
        ));
        let tick = Cell::new(0usize);
        let mut gaps: Vec<GapKind> = Vec::new();
        let readable = Cell::new(0usize);
        let now = crate::services::allowance::unix_now();

        tail_with_ticks(
            &path,
            || {
                let next = tick.get() + 1;
                tick.set(next);
                if next > 3 {
                    return None;
                }
                if next == 2 {
                    let header = "0.1 Sys [Diag]: Current time: Mon Sep 7 10:00:00 2026 [UTC: Mon Sep 7 09:00:00 2026]\n";
                    std::fs::write(&path, format!("{header}{}\n", "a".repeat(4096)))
                        .expect("create log");
                }
                Some((next as u64 * 250, now))
            },
            |_| {},
            |_, _| true,
            |kind| gaps.push(kind),
            || readable.set(readable.get() + 1),
        );

        let _ = std::fs::remove_file(&path);

        assert_eq!(gaps.first(), Some(&GapKind::Io), "the missing log is a gap");
        assert_eq!(readable.get(), 1, "recovery is reported once, not per poll");
    }

    /// A log the tailer can read but not identify is a gap on every poll. It
    /// must not also report recovery each poll, or the warning would flap.
    #[test]
    fn an_unidentified_log_does_not_report_recovery() {
        use std::cell::Cell;

        let path = std::env::temp_dir().join(format!(
            "eelog-unidentified-{}",
            wfm_core::identity::random_token(12)
        ));
        std::fs::write(&path, "0.1 Sys [Info]: no startup header\n").unwrap();
        let tick = Cell::new(0usize);
        let mut gaps = 0usize;
        let readable = Cell::new(0usize);
        let now = crate::services::allowance::unix_now();

        tail_with_ticks(
            &path,
            || {
                let next = tick.get() + 1;
                tick.set(next);
                (next <= 3).then_some((next as u64 * 250, now))
            },
            |_| {},
            |_, _| true,
            |_| gaps += 1,
            || readable.set(readable.get() + 1),
        );

        std::fs::remove_file(&path).expect("cleanup");

        assert_eq!(gaps, 3, "every poll reports the unidentified log");
        assert_eq!(readable.get(), 0);
    }
}
