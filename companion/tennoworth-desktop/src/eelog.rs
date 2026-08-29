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

/// A trade the game confirmed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TradeEvent {
    /// The other Tenno's in-game name (platform glyph stripped).
    pub partner: String,
    /// "sale" (we received plat only), "purchase" (we gave plat only), "trade".
    pub kind: String,
    /// Plat that changed hands: received on a sale, spent on a purchase.
    pub plat: i64,
    pub items: Vec<TradeItem>,
    /// The game's uptime stamp at the head of the dialog line, if present -
    /// distinguishes two identical trades in one session.
    pub log_stamp: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TradeItem {
    /// Display name as the game printed it ("Primed Flow", "Lith C5 Relic").
    pub name: String,
    pub qty: i64,
    /// "given" (left our inventory) or "received".
    pub direction: String,
}

pub const DIALOG_START: &str = "Are you sure you want to accept this trade?";
pub const TRADE_SUCCESS: &str = "The trade was successful!";
/// A dialog that never resolves within this is discarded (declined trade).
pub const DIALOG_TIMEOUT: Duration = Duration::from_secs(120);

/// Platform glyphs the game appends to names (PC/PSN/XBOX/NSW/iOS markers
/// live in the Private Use Area) - stripped so partner/item names compare.
fn strip_glyphs(s: &str) -> String {
    s.chars()
        .filter(|c| !('\u{e000}'..='\u{f8ff}').contains(c) && !('\u{f0000}'..='\u{ffffd}').contains(c))
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
    stamp.parse::<f64>().is_ok() && (rest.contains("[Info]") || rest.contains("[Error]") || rest.contains("[Warning]"))
}

/// Cut a `, leftItem=/Menu/…` or `title=` argument tail glued to the last
/// item line of a single-line dialog dump.
fn strip_arg_tail(line: &str) -> &str {
    let mut end = line.len();
    for key in [", leftItem=", " leftItem=", ", rightItem=", " rightItem=", ", title=", " title="] {
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
        if line.starts_with("leftItem=") || line.starts_with("rightItem=") || line.starts_with("title=") {
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
            if let Some(n) = rest.strip_prefix('x').and_then(|n| n.trim().parse::<i64>().ok()) {
                plat += n;
                continue;
            }
        }
        // "Name x N" for stacks; single items repeat one line each.
        let (name, qty) = match cleaned.rsplit_once(" x ") {
            Some((n, q)) if q.trim().parse::<i64>().is_ok() => (n.trim().to_string(), q.trim().parse::<i64>().unwrap_or(1)),
            _ => (cleaned.clone(), 1),
        };
        if let Some(existing) = items.iter_mut().find(|i| i.name == name) {
            existing.qty += qty;
        } else {
            items.push(TradeItem { name, qty, direction: direction.into() });
        }
    }
    (items, plat)
}

/// Parse a buffered dialog (one or more log lines) into a trade, or `None`
/// when the lines are not a trade dialog.
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

pub fn proton_log_path(steam_library: &Path) -> PathBuf {
    steam_library
        .join("steamapps/compatdata/230410/pfx/drive_c/users/steamuser/AppData/Local/Warframe/EE.log")
}

/// `"path"  "/mnt/games/SteamLibrary"` lines out of libraryfolders.vdf.
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

/// Tail `path` forever: start at the current end (past trades are not
/// re-announced), poll every `poll`, handle truncation (game restart writes a
/// fresh file) by re-seeking to 0. Each confirmed trade goes to `on_trade`.
/// Blocking - run on its own thread.
pub fn tail_forever_with_lines(
    path: &Path,
    poll: Duration,
    mut on_line: impl FnMut(&str),
    mut on_trade: impl FnMut(TradeEvent),
) {
    let mut machine = TradeMachine::new();
    let mut offset: u64 = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let mut remainder = String::new();
    let start = std::time::Instant::now();
    loop {
        std::thread::sleep(poll);
        let Ok(meta) = std::fs::metadata(path) else { continue };
        let len = meta.len();
        if len < offset {
            // Truncated / rotated: the game restarted. Start over.
            offset = 0;
            remainder.clear();
            machine = TradeMachine::new();
        }
        if len == offset {
            continue;
        }
        let Ok(mut f) = std::fs::File::open(path) else { continue };
        if f.seek(SeekFrom::Start(offset)).is_err() {
            continue;
        }
        let mut buf = Vec::with_capacity((len - offset) as usize);
        if f.read_to_end(&mut buf).is_err() {
            continue;
        }
        offset = len;
        remainder.push_str(&String::from_utf8_lossy(&buf));
        let now_ms = start.elapsed().as_millis() as u64;
        // Keep a trailing partial line for the next poll.
        let complete_upto = remainder.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let (complete, rest) = remainder.split_at(complete_upto);
        let lines: Vec<String> = complete.lines().map(|l| l.trim_end_matches('\r').to_string()).collect();
        remainder = rest.to_string();
        for line in lines {
            on_line(&line);
            if let Some(t) = machine.feed(&line, now_ms) {
                on_trade(t);
            }
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
        let Ok(meta) = std::fs::metadata(path) else { continue };
        let signature = (meta.len(), meta.modified().ok());
        if previous.as_ref() == Some(&signature) { continue; }
        previous = Some(signature);
        let Ok(mut file) = std::fs::File::open(path) else { continue };
        let start = meta.len().saturating_sub(WINDOW);
        if file.seek(SeekFrom::Start(start)).is_err() { continue; }
        let mut bytes = Vec::with_capacity((meta.len() - start) as usize);
        if file.read_to_end(&mut bytes).is_err() { continue; }
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
    use super::*;

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
        assert_eq!(t.items, vec![
            TradeItem { name: "Primed Flow".into(), qty: 1, direction: "given".into() },
            TradeItem { name: "Lith C5 Relic".into(), qty: 3, direction: "given".into() },
        ]);
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
        assert_eq!(t.items, vec![TradeItem { name: "Ash Prime Blueprint".into(), qty: 2, direction: "received".into() }]);

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
            "10.0 Sys [Info]: …(description=Are you sure you want to accept this trade?".to_string(),
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
        assert!(parse_trade_dialog(&["10.0 Sys [Info]: Dialog: Are you sure you want to sell this item?".to_string()]).is_none());
    }

    #[test]
    fn machine_emits_only_after_the_success_line_and_resets() {
        let mut m = TradeMachine::new();
        assert!(m.feed(&dialog_single_line(), 0).is_none());
        assert!(m.feed("1235.0 Sys [Info]: something else", 100).is_none());
        let t = m.feed("1236.0 Sys [Info]: The trade was successful!", 200).unwrap();
        assert_eq!(t.plat, 45);
        // A second success line without a new dialog is nothing.
        assert!(m.feed("1237.0 Sys [Info]: The trade was successful!", 300).is_none());
    }

    #[test]
    fn machine_buffers_multiline_dialogs_until_the_next_framework_line() {
        let mut m = TradeMachine::new();
        m.feed("10.0 Sys [Info]: …(description=Are you sure you want to accept this trade?", 0);
        m.feed("You are offering:", 0);
        m.feed("Primed Flow", 0);
        m.feed("and will receive from Buyer the following:", 0);
        m.feed("Platinum x 30", 0);
        m.feed("11.0 Sys [Info]: seals the dialog", 0);
        m.feed("this line must NOT become an item", 0);
        let t = m.feed("12.0 Sys [Info]: The trade was successful!", 0).unwrap();
        assert_eq!(t.items, vec![TradeItem { name: "Primed Flow".into(), qty: 1, direction: "given".into() }]);
    }

    #[test]
    fn a_declined_dialog_times_out() {
        let mut m = TradeMachine::new();
        m.feed(&dialog_single_line(), 0);
        // 3 minutes later the success line arrives for some OTHER trade whose
        // dialog we never saw - must not pair with the stale buffer.
        assert!(m.feed("x", 3 * 60 * 1000).is_none());
        assert!(m.feed("9.0 Sys [Info]: The trade was successful!", 3 * 60 * 1000 + 1).is_none());
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
        assert_eq!(parse_steam_library_paths(vdf), vec!["/home/me/.local/share/Steam", "/mnt/games/SteamLibrary"]);
        assert!(proton_log_path(Path::new("/mnt/games/SteamLibrary")).ends_with("Warframe/EE.log"));
    }
}
