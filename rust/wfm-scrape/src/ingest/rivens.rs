use super::Http;
use crate::clock;
use chrono::DateTime;
use chrono::Utc;
use std::collections::HashMap;

pub const WFM_RIVEN_WEAPONS_URL: &str = "https://api.warframe.market/v2/riven/weapons";

/// How long a disposition change stays in the snapshot's rolling change log.
/// DE ships disposition passes with each Prime Access (~quarterly); 90 days
/// keeps the last pass visible until the next one lands.
pub const RIVEN_CHANGE_RETENTION_DAYS: i64 = 90;

/// Fetch WFM's riven-weapon manifest and reduce it to
/// `weapons: {slug: {name, disposition, group, riven_type, req_mr}}`, then
/// diff dispositions against the prior snapshot's `rivens.weapons` into a
/// rolling `changes: [{slug, name, from, to, seen_at}]` (newest first,
/// bounded by [`RIVEN_CHANGE_RETENTION_DAYS`]).
///
/// Why: DE has stopped decreasing dispositions and only raises them (2024
/// policy, restated in the 2025-26 workshops), so a change is a one-sided
/// price event for anyone holding that weapon's rivens - and repricing on
/// WFM lands within a day of the patch notes. The scrape runs every 2 h, so
/// the log catches it the same day. `seen_at` is when WE first saw the new
/// value, not DE's patch time.
///
/// Returns `{}` on fetch failure so reconcile falls back to the prior surface.
/// The riven weapons manifest: slug → {name, disposition, group, riven_type,
/// req_mr, game_ref}. Dispositions here are warframe.market's MIRROR of DE's;
/// the caller overrides them from `ExportWeapons` and only then computes the
/// change log - see `riven_change_log` for why the order matters.
pub fn fetch_rivens(http: &dyn Http) -> HashMap<String, serde_json::Value> {
    let data = match http.get_json(WFM_RIVEN_WEAPONS_URL) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("  warning: could not fetch {WFM_RIVEN_WEAPONS_URL}: {e}");
            return HashMap::new();
        }
    };
    let arr = data
        .get("data")
        .and_then(|d| d.as_array())
        .or_else(|| data.as_array());
    let Some(arr) = arr else {
        eprintln!("  warning: {WFM_RIVEN_WEAPONS_URL}: unexpected shape");
        return HashMap::new();
    };

    let mut weapons = serde_json::Map::new();
    for w in arr {
        let Some(slug) = w.get("slug").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(dispo) = w.get("disposition").and_then(|v| v.as_f64()) else {
            continue;
        };
        let name = w
            .get("i18n")
            .and_then(|i| i.get("en"))
            .and_then(|e| e.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or(slug);
        let mut row = serde_json::Map::new();
        row.insert("name".into(), serde_json::Value::String(name.into()));
        row.insert("disposition".into(), serde_json::json!(dispo));
        for (src, dst) in [("group", "group"), ("rivenType", "riven_type")] {
            if let Some(v) = w.get(src).and_then(|v| v.as_str()) {
                row.insert(dst.into(), serde_json::Value::String(v.into()));
            }
        }
        if let Some(mr) = w.get("reqMasteryRank").and_then(|v| v.as_i64()) {
            row.insert("req_mr".into(), serde_json::Value::from(mr));
        }
        // The weapon's in-game path - the SPA maps a scanned riven's `compat`
        // fingerprint field to this slug through it.
        if let Some(gr) = w.get("gameRef").and_then(|v| v.as_str()) {
            row.insert("game_ref".into(), serde_json::Value::String(gr.into()));
        }
        weapons.insert(slug.to_string(), serde_json::Value::Object(row));
    }
    if weapons.is_empty() {
        return HashMap::new();
    }

    // NOTE: the disposition change log is NOT computed here. It has to diff
    // the values that actually get PUBLISHED, and the DE override is applied
    // by the caller after this returns - see `riven_change_log`.

    // The stat-name/unit manifest (`/v2/riven/attributes`). A scanned
    // riven's fingerprint names stats by DE tag (`WeaponCritDamageMod`); this
    // maps that tag (game_ref) to a display name + whether it is a percent
    // stat, so the Rivens view can render the raw fingerprint value as
    // "+95.3% crit damage" instead of a bare integer.
    let mut attributes: Vec<serde_json::Value> = Vec::new();
    if let Ok(data) = http.get_json(WFM_RIVEN_ATTRIBUTES_URL) {
        let arr = data
            .get("data")
            .and_then(|d| d.as_array())
            .or_else(|| data.as_array());
        if let Some(arr) = arr {
            for a in arr {
                let Some(gr) = a.get("gameRef").and_then(|v| v.as_str()) else {
                    continue;
                };
                let mut row = serde_json::Map::new();
                row.insert("game_ref".into(), serde_json::Value::String(gr.into()));
                if let Some(slug) = a.get("slug").and_then(|v| v.as_str()) {
                    row.insert("slug".into(), serde_json::Value::String(slug.into()));
                }
                if let Some(name) = a
                    .get("i18n")
                    .and_then(|i| i.get("en"))
                    .and_then(|e| e.get("name"))
                    .and_then(|n| n.as_str())
                {
                    row.insert("name".into(), serde_json::Value::String(name.into()));
                }
                if let Some(unit) = a.get("unit").and_then(|v| v.as_str()) {
                    row.insert("unit".into(), serde_json::Value::String(unit.into()));
                }
                attributes.push(serde_json::Value::Object(row));
            }
        }
    } else {
        eprintln!("  warning: could not fetch {WFM_RIVEN_ATTRIBUTES_URL}");
    }

    let mut out = HashMap::new();
    out.insert("weapons".into(), serde_json::Value::Object(weapons));
    if !attributes.is_empty() {
        out.insert("attributes".into(), serde_json::Value::Array(attributes));
    }
    out
}

/// The rolling disposition change log, diffed against the FINAL published
/// weapons map.
///
/// This used to live inside `fetch_rivens`, which made it diff
/// warframe.market's fresh mirror against the prior snapshot's STORED values -
/// and those are DE's, because the caller overrides them afterwards. The
/// result was wrong in both directions at once:
///
/// - **Phantom changes.** WFM's mirror lags DE. Every cycle where it still
///   disagreed logged a change from DE's stored value to WFM's stale one, and
///   then the override rewrote the value back to DE's. Nothing had changed;
///   the log said something had.
/// - **Missed changes.** A genuine DE disposition move is applied by the
///   override *after* the diff ran, so it never appeared in the log at all.
///
/// Diffing the published values against the previously published values is the
/// only comparison that means anything: the log describes what the snapshot
/// says, not what one upstream happened to report mid-pipeline.
pub fn riven_change_log(
    weapons: &serde_json::Map<String, serde_json::Value>,
    prior: Option<&HashMap<String, serde_json::Value>>,
    now: DateTime<Utc>,
) -> Vec<serde_json::Value> {
    let prior_weapons = prior
        .and_then(|p| p.get("weapons"))
        .and_then(|w| w.as_object());
    let mut changes: Vec<serde_json::Value> = Vec::new();
    if let Some(pw) = prior_weapons {
        for (slug, row) in weapons {
            let Some(to) = row.get("disposition").and_then(|d| d.as_f64()) else {
                continue;
            };
            let Some(from) = pw
                .get(slug)
                .and_then(|r| r.get("disposition"))
                .and_then(|d| d.as_f64())
            else {
                continue;
            };
            // Dispositions are quoted to 2 dp; anything under half a hundredth
            // is float noise, not a change.
            if (to - from).abs() < 0.005 {
                continue;
            }
            changes.push(serde_json::json!({
                "slug": slug,
                "name": row.get("name").cloned().unwrap_or(serde_json::Value::String(slug.clone())),
                "from": from,
                "to": to,
                "seen_at": clock::iso_z(now),
            }));
        }
    }
    // Carry the prior log forward, dropping entries past retention and any
    // entry for a slug that just changed again (the new row supersedes it).
    let cutoff = now - chrono::Duration::days(RIVEN_CHANGE_RETENTION_DAYS);
    let changed_now: std::collections::HashSet<String> = changes
        .iter()
        .filter_map(|c| c.get("slug").and_then(|s| s.as_str()).map(String::from))
        .collect();
    if let Some(old) = prior
        .and_then(|p| p.get("changes"))
        .and_then(|c| c.as_array())
    {
        for c in old {
            let slug = c.get("slug").and_then(|s| s.as_str()).unwrap_or("");
            if changed_now.contains(slug) {
                continue;
            }
            let keep = c
                .get("seen_at")
                .and_then(|s| s.as_str())
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|t| t.with_timezone(&Utc) >= cutoff)
                .unwrap_or(false);
            if keep {
                changes.push(c.clone());
            }
        }
    }
    changes.sort_by(|a, b| {
        let sa = a.get("seen_at").and_then(|s| s.as_str()).unwrap_or("");
        let sb = b.get("seen_at").and_then(|s| s.as_str()).unwrap_or("");
        sb.cmp(sa).then_with(|| {
            a.get("slug")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .cmp(b.get("slug").and_then(|s| s.as_str()).unwrap_or(""))
        })
    });
    changes
}

pub const WFM_RIVEN_ATTRIBUTES_URL: &str = "https://api.warframe.market/v2/riven/attributes";

/// DE's weekly riven price statistics, published every Monday as a JS object
/// literal (NOT JSON: unquoted keys, single-quoted strings), keyed by weapon
/// display name x `rerolled`. ~150 KB; the only stats that actually sample
/// riven auctions - WFM's `/statistics` has no riven rows at all.
pub use crate::de::DE_WEEKLY_RIVENS_URL;

/// The same file per platform. Console riven markets diverge sharply from PC's
/// because of different populations, different metas, and far smaller samples.
/// DE publishes all four while nothing in the ecosystem compares them.
///
/// PC stays the primary surface (`unrolled`/`rolled` at the top level); the
/// consoles ride along under `platforms` so a consumer that only knows about
/// PC keeps working untouched.
pub use crate::de::DE_WEEKLY_RIVEN_PLATFORMS;

/// One row of `DE_WEEKLY_RIVENS_URL`: one weapon x reroll-state's price band.
/// Generic rows (`compatibility: null` - "Rifle Riven Mod") carry no weapon
/// and are dropped by `fetch_riven_stats`.
#[derive(Debug, Clone, PartialEq)]
pub struct WeeklyRivenRow {
    pub item_type: String,
    pub compatibility: Option<String>,
    pub rerolled: bool,
    pub avg: f64,
    pub stddev: f64,
    pub min: f64,
    pub max: f64,
    pub pop: u64,
    pub median: f64,
}

#[derive(Debug, Clone, PartialEq)]
enum JsTok {
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Str(String),
    /// unquoted key or the bare literals true/false/null
    Ident(String),
    Num(f64),
}

#[allow(
    clippy::indexing_slicing,
    clippy::string_slice,
    reason = "the byte cursor is bounds-checked at every access and slice boundaries advance only over ASCII tokens"
)]
fn tokenize_js_literal(text: &str) -> Result<Vec<JsTok>, String> {
    let b = text.as_bytes();
    let mut i = 0usize;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i] as char;
        match c {
            ' ' | '\t' | '\r' | '\n' => i += 1,
            '{' => {
                out.push(JsTok::LBrace);
                i += 1;
            }
            '}' => {
                out.push(JsTok::RBrace);
                i += 1;
            }
            '[' => {
                out.push(JsTok::LBracket);
                i += 1;
            }
            ']' => {
                out.push(JsTok::RBracket);
                i += 1;
            }
            ',' => {
                out.push(JsTok::Comma);
                i += 1;
            }
            ':' => {
                out.push(JsTok::Colon);
                i += 1;
            }
            '\'' => {
                let mut s = String::new();
                i += 1;
                loop {
                    if i >= b.len() {
                        return Err(format!("unterminated string at byte {i}"));
                    }
                    let ch = b[i] as char;
                    if ch == '\\' {
                        i += 1;
                        if i >= b.len() {
                            return Err(format!("unterminated escape at byte {i}"));
                        }
                        s.push(b[i] as char);
                        i += 1;
                    } else if ch == '\'' {
                        i += 1;
                        break;
                    } else {
                        s.push(ch);
                        i += 1;
                    }
                }
                out.push(JsTok::Str(s));
            }
            '0'..='9' | '-' => {
                let start = i;
                while i < b.len()
                    && (b[i].is_ascii_digit() || matches!(b[i], b'.' | b'-' | b'+' | b'e' | b'E'))
                {
                    i += 1;
                }
                let num = &text[start..i];
                let v: f64 = num
                    .parse()
                    .map_err(|_| format!("bad number in {DE_WEEKLY_RIVENS_URL}: {num:?}"))?;
                out.push(JsTok::Num(v));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                    i += 1;
                }
                out.push(JsTok::Ident(text[start..i].to_string()));
            }
            other => return Err(format!("unexpected char {other:?} at byte {i}")),
        }
    }
    Ok(out)
}

/// Recursive-descent parse of the JS-literal subset DE actually emits: objects,
/// arrays, strings, numbers, `null`, booleans. Anything else is an error - a
/// shape change upstream should fail the surface loudly, not silently zero it.
fn js_literal_to_json(tokens: &[JsTok], pos: &mut usize) -> Result<serde_json::Value, String> {
    let Some(tok) = tokens.get(*pos) else {
        return Err(format!("unexpected end of {DE_WEEKLY_RIVENS_URL}"));
    };
    *pos += 1;
    match tok {
        JsTok::LBrace => {
            let mut map = serde_json::Map::new();
            loop {
                if matches!(tokens.get(*pos), Some(JsTok::RBrace)) {
                    *pos += 1;
                    break;
                }
                let key = match tokens.get(*pos) {
                    Some(JsTok::Str(s)) | Some(JsTok::Ident(s)) => s.clone(),
                    _ => return Err(format!("expected object key at token {}", *pos)),
                };
                *pos += 1;
                if !matches!(tokens.get(*pos), Some(JsTok::Colon)) {
                    return Err(format!("expected ':' after {key:?}"));
                }
                *pos += 1;
                let value = js_literal_to_json(tokens, pos)?;
                map.insert(key, value);
                if matches!(tokens.get(*pos), Some(JsTok::Comma)) {
                    *pos += 1;
                }
            }
            Ok(serde_json::Value::Object(map))
        }
        JsTok::LBracket => {
            let mut arr = Vec::new();
            loop {
                if matches!(tokens.get(*pos), Some(JsTok::RBracket)) {
                    *pos += 1;
                    break;
                }
                arr.push(js_literal_to_json(tokens, pos)?);
                match tokens.get(*pos) {
                    Some(JsTok::Comma) => *pos += 1,
                    Some(JsTok::RBracket) => {
                        *pos += 1;
                        break;
                    }
                    _ => return Err(format!("expected ',' or ']' at token {}", *pos)),
                }
            }
            Ok(serde_json::Value::Array(arr))
        }
        JsTok::Str(s) => Ok(serde_json::Value::String(s.clone())),
        JsTok::Num(n) => Ok(serde_json::Value::from(*n)),
        JsTok::Ident(id) => match id.as_str() {
            "true" => Ok(serde_json::Value::Bool(true)),
            "false" => Ok(serde_json::Value::Bool(false)),
            "null" => Ok(serde_json::Value::Null),
            other => Err(format!("unexpected identifier {other:?}")),
        },
        _ => Err(format!("unexpected token at position {}", *pos)),
    }
}

/// Parse the full `DE_WEEKLY_RIVENS_URL` body into rows. Rows missing the
/// price fields are dropped (a shape change upstream surfaces as a count
/// drop, which the build logs).
fn parse_weekly_rivens_counted(text: &str) -> Result<(Vec<WeeklyRivenRow>, usize), String> {
    let tokens = tokenize_js_literal(text)?;
    let mut pos = 0usize;
    let value = js_literal_to_json(&tokens, &mut pos)?;
    let arr = value
        .as_array()
        .ok_or_else(|| format!("{DE_WEEKLY_RIVENS_URL}: not an array"))?;
    let raw_count = arr.len();
    let mut rows = Vec::new();
    for v in arr {
        let Some(obj) = v.as_object() else { continue };
        let f = |k: &str| obj.get(k).and_then(|v| v.as_f64());
        let Some(avg) = f("avg") else { continue };
        let Some(median) = f("median") else { continue };
        rows.push(WeeklyRivenRow {
            item_type: obj
                .get("itemType")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            compatibility: obj
                .get("compatibility")
                .and_then(|v| v.as_str())
                .map(String::from),
            rerolled: obj
                .get("rerolled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            avg,
            stddev: f("stddev").unwrap_or(0.0),
            min: f("min").unwrap_or(0.0),
            max: f("max").unwrap_or(0.0),
            // as_u64() is None for the f64 the JS literal produces (10 → 10.0)
            pop: obj
                .get("pop")
                .and_then(|v| v.as_f64())
                .map(|f| f as u64)
                .unwrap_or(0),
            median,
        });
    }
    Ok((rows, raw_count))
}

#[cfg(test)]
pub(super) fn parse_weekly_rivens(text: &str) -> Result<Vec<WeeklyRivenRow>, String> {
    parse_weekly_rivens_counted(text).map(|(rows, _)| rows)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RivenChildOutcome {
    Unavailable,
    Invalid,
    AuthoritativeEmpty,
    Usable,
}

/// Build the `riven_stats` surface: DE's weekly price bands per weapon x
/// reroll-state, keyed by WFM slug (`unrolled` / `rolled` tiers, each
/// `{avg, median, min, max, stddev, pop}`). `weapons_by_name` maps
/// display-name-lower -> slug from the riven-weapons manifest (the same source
/// the disposition surface uses), so DE's names join without a second
/// request. Returns the unmatched-name count for the build log. `{}` + 0 on
/// fetch/parse failure so reconcile falls back to the prior surface.
pub fn fetch_riven_stats(
    http: &dyn Http,
    weapons_by_name: &HashMap<String, String>,
) -> (
    HashMap<String, serde_json::Value>,
    usize,
    HashMap<String, RivenChildOutcome>,
) {
    let mut outcomes = HashMap::new();
    let text = match http.get_text(DE_WEEKLY_RIVENS_URL) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("  warning: could not fetch {DE_WEEKLY_RIVENS_URL}: {e}");
            outcomes.insert("pc".into(), RivenChildOutcome::Unavailable);
            return (HashMap::new(), 0, outcomes);
        }
    };
    let (rows, raw_count) = match parse_weekly_rivens_counted(&text) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  warning: {DE_WEEKLY_RIVENS_URL}: {e}");
            outcomes.insert("pc".into(), RivenChildOutcome::Invalid);
            return (HashMap::new(), 0, outcomes);
        }
    };
    if raw_count == 0 {
        outcomes.insert("pc".into(), RivenChildOutcome::AuthoritativeEmpty);
        return (HashMap::new(), 0, outcomes);
    }
    if rows.is_empty() {
        outcomes.insert("pc".into(), RivenChildOutcome::Invalid);
        return (HashMap::new(), 0, outcomes);
    }
    if rows.len() < raw_count {
        outcomes.insert("pc".into(), RivenChildOutcome::Invalid);
        return (HashMap::new(), 0, outcomes);
    }
    let mut stats: HashMap<String, serde_json::Value> = HashMap::new();
    let mut unmatched = 0usize;
    for row in rows {
        let Some(weapon) = row.compatibility else {
            continue;
        };
        let Some(slug) = weapons_by_name.get(&weapon.to_lowercase()) else {
            unmatched += 1;
            continue;
        };
        let tier_key = if row.rerolled { "rolled" } else { "unrolled" };
        let tier = serde_json::json!({
            "avg": row.avg, "median": row.median, "min": row.min, "max": row.max,
            "stddev": row.stddev, "pop": row.pop,
        });
        let entry = stats
            .entry(slug.clone())
            .or_insert_with(|| serde_json::json!({ "name": weapon }));
        entry[tier_key] = tier;
    }

    // Consoles, folded in under `platforms`. A platform that fails to fetch or
    // parse is skipped with a warning rather than failing the surface - PC is
    // what the app actually prices against, and losing it to a Switch outage
    // would be absurd.
    for (platform, url) in DE_WEEKLY_RIVEN_PLATFORMS {
        let Ok(text) = http.get_text(url) else {
            outcomes.insert((*platform).to_string(), RivenChildOutcome::Unavailable);
            eprintln!("  warning: could not fetch {url}");
            continue;
        };
        let (rows, raw_count) = match parse_weekly_rivens_counted(&text) {
            Ok(r) => r,
            Err(e) => {
                outcomes.insert((*platform).to_string(), RivenChildOutcome::Invalid);
                eprintln!("  warning: {url}: {e}");
                continue;
            }
        };
        if raw_count == 0 {
            outcomes.insert(
                (*platform).to_string(),
                RivenChildOutcome::AuthoritativeEmpty,
            );
            continue;
        }
        if rows.is_empty() {
            outcomes.insert((*platform).to_string(), RivenChildOutcome::Invalid);
            continue;
        }
        if rows.len() < raw_count {
            outcomes.insert((*platform).to_string(), RivenChildOutcome::Invalid);
            continue;
        }
        let mut joined = 0usize;
        for row in rows {
            let Some(weapon) = row.compatibility else {
                continue;
            };
            let Some(slug) = weapons_by_name.get(&weapon.to_lowercase()) else {
                continue;
            };
            // Only alongside a weapon PC already knows. A console-only row has
            // no PC baseline to compare against, which is the only reason to
            // carry it.
            let Some(entry) = stats.get_mut(slug) else {
                continue;
            };
            let tier_key = if row.rerolled { "rolled" } else { "unrolled" };
            let tier = serde_json::json!({
                "avg": row.avg, "median": row.median, "min": row.min, "max": row.max,
                "stddev": row.stddev, "pop": row.pop,
            });
            if !entry
                .get("platforms")
                .map(|p| p.is_object())
                .unwrap_or(false)
            {
                entry["platforms"] = serde_json::json!({});
            }
            #[allow(
                clippy::indexing_slicing,
                reason = "serde_json object indexing inserts or returns Null instead of panicking"
            )]
            let plat = &mut entry["platforms"][*platform];
            if !plat.is_object() {
                *plat = serde_json::json!({});
            }
            plat[tier_key] = tier;
            joined += 1;
        }
        outcomes.insert(
            (*platform).to_string(),
            if joined == 0 {
                RivenChildOutcome::Invalid
            } else {
                RivenChildOutcome::Usable
            },
        );
    }

    if stats.is_empty() {
        outcomes.insert("pc".into(), RivenChildOutcome::Invalid);
    } else {
        outcomes.insert("pc".into(), RivenChildOutcome::Usable);
    }
    (stats, unmatched, outcomes)
}

/// Retain only failed console children while replacing the successful PC
/// baseline. Iterating fresh PC slugs prevents a stale console-only weapon from
/// being resurrected after it disappears from the primary feed.
pub fn carry_failed_riven_platforms(
    fresh: &mut HashMap<String, serde_json::Value>,
    prior: Option<&HashMap<String, serde_json::Value>>,
    outcomes: &HashMap<String, RivenChildOutcome>,
) {
    let Some(prior) = prior else { return };
    for (platform, _) in DE_WEEKLY_RIVEN_PLATFORMS {
        if matches!(
            outcomes.get(*platform),
            Some(RivenChildOutcome::Usable | RivenChildOutcome::AuthoritativeEmpty)
        ) {
            continue;
        }
        for (slug, row) in fresh.iter_mut() {
            let Some(old_platform) = prior
                .get(slug)
                .and_then(|old| old.get("platforms"))
                .and_then(|p| p.get(*platform))
                .cloned()
            else {
                continue;
            };
            let Some(obj) = row.as_object_mut() else {
                continue;
            };
            let platforms = obj
                .entry("platforms")
                .or_insert_with(|| serde_json::json!({}));
            if let Some(map) = platforms.as_object_mut() {
                map.insert((*platform).to_string(), old_platform);
            }
        }
    }
}
