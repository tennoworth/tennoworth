//! The dated rows the calendar views show, from the snapshot and the curated
//! feed: the trader feed ([`feed`], the site's and desktop's "what is coming"
//! list) and the month and lane calendar ([`events`]). Dates are read as the
//! page's `Date.parse` reads them ([`date`]) and `now` comes from the page.
//!
//! The snapshot surfaces arrive as the JSON `contracts/data.ts` describes and
//! are read field by field, as the page read them: typed deserialization of
//! every surface would triple the module's size. `JSON.stringify` wrote them,
//! so an object's entries arrive in the order `Object.entries` walks, and
//! ties keep that order.

pub mod date;
pub mod events;
pub mod feed;

use date::parse_ms;
use serde::Deserialize;
use serde_json::Value;
use ts_rs::TS;

const DAY: i64 = 86_400_000;
/// Whole days after which carried event data reads as stale: the age the
/// pipeline warns at (tests/fixtures/stale-days.json).
pub const STALE_DAYS: f64 = 7.0;
const BARO_PERIOD: i64 = 14 * DAY;
/// Projected Baro visits shown after the confirmed one.
const BARO_PROJECTIONS: i64 = 2;
/// How far ahead estimated vaulting is worth showing.
const VAULT_HORIZON: f64 = 120.0 * DAY as f64;
/// Finished events stay visible this long, so the month view keeps context.
const PAST: f64 = 45.0 * DAY as f64;

/// One owned record as the calendar reads it.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(optional_fields = nullable)]
pub struct CalendarHolding {
    pub slug: String,
    pub count: Option<f64>,
}

/// What both calendar rules read.
#[derive(Debug, Clone, Deserialize, TS)]
pub struct CalendarRequest {
    /// The snapshot's `baro`, `calendar`, `set_to_parts`, `de`,
    /// `event_rewards` and `surface_provenance`.
    #[ts(type = "unknown")]
    pub market: Value,
    /// The curated announcement feed, already validated by the page's
    /// reader; only the month and lane calendar shows it.
    #[serde(default)]
    #[ts(optional, type = "unknown")]
    pub feed: Value,
    /// Absent before an inventory is loaded, which is not an empty one.
    #[ts(optional = nullable)]
    pub owned: Option<Vec<CalendarHolding>>,
    pub now: f64,
}

/// `value?.[key]`, where null reads as absent.
fn field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key).filter(|v| !v.is_null())
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// A JavaScript-truthy string field.
fn filled<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    text(value, key).filter(|s| !s.is_empty())
}

/// A string field as a template literal prints it, for ids.
fn printed(value: &Value, key: &str) -> String {
    match value.get(key) {
        None => "undefined".to_owned(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// `value?.[key] ?? []`.
fn rows<'a>(value: Option<&'a Value>, key: &str) -> &'a [Value] {
    value
        .and_then(|v| v.get(key))
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// `Object.entries(value?.[key] ?? {})`.
fn entries<'a>(
    value: Option<&'a Value>,
    key: &str,
) -> impl Iterator<Item = (&'a String, &'a Value)> {
    value
        .and_then(|v| v.get(key))
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
}

/// `value ? Date.parse(value) : NaN`, as epoch ms.
fn at(value: &Value, key: &str) -> Option<i64> {
    text(value, key).and_then(parse_ms)
}

/// A JavaScript-truthy number field.
fn truthy_number(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64).filter(|n| *n != 0.0)
}

/// `typeof x === 'number' && x > 0`.
fn positive(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64).filter(|n| *n > 0.0)
}

#[allow(clippy::cast_precision_loss, reason = "time values are within 2^53")]
fn ms(value: i64) -> f64 {
    value as f64
}

/// The parts of a set, from `set_to_parts`.
fn set_parts<'a>(market: &'a Value, set: &str) -> &'a [Value] {
    rows(
        field(market, "set_to_parts").and_then(|sets| sets.get(set)),
        "parts",
    )
}

/// Goals, then events, of `event_rewards`.
fn reward_rows(market: &Value) -> impl Iterator<Item = &Value> {
    let rewards = field(market, "event_rewards");
    entries(rewards, "goals")
        .chain(entries(rewards, "events"))
        .map(|(_, entry)| entry)
}

/// Whether a provenance disposition records an actual observation. The
/// producer stamps both timestamps with the attempt when a read failed and
/// there was nothing to carry, so only these dispositions may be measured
/// (tests/fixtures/surface-freshness.json, `calendar_evidence`). A row with
/// no disposition predates them and is measured; a null one is not.
fn has_evidence(disposition: Option<&Value>) -> bool {
    match disposition {
        None => true,
        Some(Value::String(d)) => matches!(
            d.as_str(),
            "published_fresh"
                | "merged_partial"
                | "cleared_authoritative_empty"
                | "preserved_unchanged"
        ),
        Some(_) => false,
    }
}

/// Split an identifier into lowercase words.
///
/// Bundle SKUs are CamelCase with no separators (`MPVRevenantPrimeSinglePack`
/// is mpv, revenant, prime, single, pack); set names are already spaced. Word
/// lists are what make the comparison safe: seven prime names are letter
/// substrings of another (Bo inside Limbo, Bronco inside Akbronco, Lex inside
/// Aklex), and "akbronco" is one word that never equals "bronco".
pub fn words(text: &str) -> Vec<String> {
    // DE spells "&" out (Cobra & Crane Prime is MPVCobraAndCranePrimeSinglePack),
    // so dropping it as punctuation would lose the word.
    let chars: Vec<char> = text.replace('&', " and ").chars().collect();
    let mut out = Vec::new();
    let mut word = String::new();
    let mut end = |word: &mut String| {
        if !word.is_empty() {
            out.push(std::mem::take(word));
        }
    };
    for (i, c) in chars.iter().enumerate() {
        let previous = i.checked_sub(1).and_then(|p| chars.get(p));
        // An acronym ends before its last capital (MPVRevenant is MPV Revenant),
        // and a lower case letter or digit ends before a capital (AkbroncoPrime).
        if c.is_ascii_uppercase()
            && previous.is_some_and(|p| {
                p.is_ascii_lowercase()
                    || p.is_ascii_digit()
                    || (p.is_ascii_uppercase()
                        && chars.get(i + 1).is_some_and(char::is_ascii_lowercase))
            })
        {
            end(&mut word);
        }
        // Lower-cased as `toLowerCase` does, keeping [a-z0-9]: only U+0130
        // (to "i" and a combining dot) and the Kelvin sign (to "k") reach it
        // from outside ASCII.
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' => word.push(c.to_ascii_lowercase()),
            '\u{212a}' => word.push('k'),
            '\u{130}' => {
                word.push('i');
                end(&mut word);
            }
            _ => end(&mut word),
        }
    }
    end(&mut word);
    out
}

/// Whether `needle` appears as a contiguous run of whole words in `haystack`.
pub fn contains_words(haystack: &[String], needle: &[String]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|run| run == needle)
}

/// Whether to nudge toward listing: Thursday and Friday say list before the
/// weekend, Saturday and Sunday say now. Over the year to 2026-11 each weekend
/// day carried 16.5-16.7% of a week's trades against 13.3-13.4% Monday to
/// Thursday, about a quarter more buyers, while prices moved under 1% by
/// weekday: this is about selling sooner, not for more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeekendNudge {
    Before,
    During,
}

/// `weekday` as `Date.prototype.getDay` gives it in the reader's zone: 0 is Sunday.
pub fn weekend_nudge(weekday: f64) -> Option<WeekendNudge> {
    if weekday == 4.0 || weekday == 5.0 {
        Some(WeekendNudge::Before)
    } else if weekday == 6.0 || weekday == 0.0 {
        Some(WeekendNudge::During)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_identifiers_into_words() {
        assert_eq!(
            words("MPVRevenantPrimeSinglePack"),
            ["mpv", "revenant", "prime", "single", "pack"]
        );
        assert_eq!(
            words("Cobra & Crane Prime"),
            ["cobra", "and", "crane", "prime"]
        );
        assert_eq!(
            words("MPVCobraAndCranePrimeSinglePack"),
            ["mpv", "cobra", "and", "crane", "prime", "single", "pack"]
        );
        assert_eq!(
            words("ABCDef aBcDEf A1Bc"),
            ["abc", "def", "a", "bc", "d", "ef", "a1", "bc"]
        );
        assert_eq!(
            words("\u{130}x a\u{212a}b \u{e9}t\u{e9}"),
            ["i", "x", "akb", "t"]
        );
        assert!(contains_words(
            &words("MPVAkbroncoPrimePack"),
            &words("Akbronco Prime")
        ));
        assert!(!contains_words(
            &words("MPVAkbroncoPrimePack"),
            &words("Bronco Prime")
        ));
        assert!(!contains_words(&words("MPVAkbroncoPrimePack"), &[]));
    }

    #[test]
    fn the_weekend_nudge_follows_the_weekday() {
        let nudges: Vec<_> = (0..7).map(|d| weekend_nudge(f64::from(d))).collect();
        use WeekendNudge::{Before, During};
        assert_eq!(
            nudges,
            [
                Some(During),
                None,
                None,
                None,
                Some(Before),
                Some(Before),
                Some(During)
            ]
        );
    }

    #[test]
    fn evidence_follows_the_shared_freshness_fixture() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/surface-freshness.json"
        ))
        .unwrap();
        // The fixture writes a legacy row's absent disposition as null.
        for case in fixture["cases"].as_array().unwrap() {
            assert_eq!(
                has_evidence(Some(&case["disposition"]).filter(|d| !d.is_null())),
                case["calendar_evidence"].as_bool().unwrap(),
                "{}",
                case["name"]
            );
        }
        assert!(!has_evidence(Some(&Value::Null)));
    }

    #[test]
    fn stale_days_is_the_pipelines() {
        let fixture: Value =
            serde_json::from_str(include_str!("../../../../tests/fixtures/stale-days.json"))
                .unwrap();
        assert_eq!(fixture["stale_days"].as_f64(), Some(STALE_DAYS));
    }
}
