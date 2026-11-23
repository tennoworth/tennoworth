//! `Date.parse` for the ISO strings snapshots and the curated feed carry.
//!
//! The rules compare these dates with the page's `now`, so they must read
//! each string exactly as the page's engine would. This is ECMAScript's Date
//! Time String Format as the engines read it: a year (four digits, or a sign
//! and six), optional month and day (a day past the month's end rolls over, as
//! `Date.parse` does), then an optional time with `Z` or a numeric offset.
//! A date alone is UTC. A time without an offset is local time, which only the
//! page knows, so it reads as no date; snapshots never carry one
//! (tests/fixtures/calendar/date-parse.json pins both sides).

const DAY_MS: i64 = 86_400_000;
/// ECMAScript's time value range.
const MAX_MS: i64 = 8_640_000_000_000_000;

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn eat(&mut self, any: &[u8]) -> Option<u8> {
        let c = self.peek().filter(|c| any.contains(c))?;
        self.at += 1;
        Some(c)
    }

    fn number(&mut self, width: usize) -> Option<i64> {
        let digits = self.bytes.get(self.at..self.at + width)?;
        if !digits.iter().all(u8::is_ascii_digit) {
            return None;
        }
        self.at += width;
        Some(digits.iter().fold(0, |n, d| n * 10 + i64::from(d - b'0')))
    }

    /// Milliseconds from a fraction of one or more digits; past three they are dropped.
    fn millis(&mut self) -> Option<i64> {
        let start = self.at;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.at += 1;
        }
        let digits = self.bytes.get(start..self.at).filter(|d| !d.is_empty())?;
        Some(
            (0..3)
                .map(|i| digits.get(i).map_or(0, |d| i64::from(d - b'0')))
                .fold(0, |n, d| n * 10 + d),
        )
    }

    fn done(&self) -> bool {
        self.at == self.bytes.len()
    }
}

/// Days from 1970-01-01 to the first of `month` in `year` (proleptic Gregorian).
fn days_from_civil(year: i64, month: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let year_of_era = y - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Epoch milliseconds, or `None` where `Date.parse` gives `NaN` (and for
/// local times, see the module note).
pub fn parse_ms(raw: &str) -> Option<i64> {
    let mut s = Cursor {
        bytes: raw.as_bytes(),
        at: 0,
    };
    let year = match s.eat(b"+-") {
        Some(sign) => {
            let year = s.number(6)?;
            if sign == b'-' {
                if year == 0 {
                    return None;
                }
                -year
            } else {
                year
            }
        }
        None => s.number(4)?,
    };
    let (mut month, mut day) = (1, 1);
    if s.eat(b"-").is_some() {
        month = s.number(2)?;
        if s.eat(b"-").is_some() {
            day = s.number(2)?;
        }
    }
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let date = (days_from_civil(year, month) + day - 1) * DAY_MS;
    if s.done() {
        return within(date);
    }
    s.eat(b"Tt ")?;
    let hour = s.number(2)?;
    s.eat(b":")?;
    let minute = s.number(2)?;
    let (mut second, mut milli) = (0, 0);
    if s.eat(b":").is_some() {
        second = s.number(2)?;
        if s.eat(b".").is_some() {
            milli = s.millis()?;
        }
    }
    if hour > 24 || minute > 59 || second > 59 || (hour == 24 && minute + second + milli > 0) {
        return None;
    }
    let offset = match s.eat(b"Zz+-")? {
        b'Z' | b'z' => 0,
        sign => {
            let hours = s.number(2)?;
            s.eat(b":");
            let minutes = s.number(2)?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            let offset = (hours * 60 + minutes) * 60_000;
            if sign == b'-' {
                -offset
            } else {
                offset
            }
        }
    };
    if !s.done() {
        return None;
    }
    within(date + ((hour * 60 + minute) * 60 + second) * 1000 + milli - offset)
}

fn within(ms: i64) -> Option<i64> {
    (-MAX_MS..=MAX_MS).contains(&ms).then_some(ms)
}

/// `String.prototype.trim`: ECMAScript's white space and line terminators,
/// which differ from Unicode's `White_Space` (U+FEFF is trimmed, U+0085 is not).
pub fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| {
        matches!(
            c,
            '\u{9}'
                | '\u{a}'
                | '\u{b}'
                | '\u{c}'
                | '\u{d}'
                | ' '
                | '\u{a0}'
                | '\u{1680}'
                | '\u{2000}'
                ..='\u{200a}'
                    | '\u{2028}'
                    | '\u{2029}'
                    | '\u{202f}'
                    | '\u{205f}'
                    | '\u{3000}'
                    | '\u{feff}'
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn reads_dates_as_date_parse_does() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/calendar/date-parse.json"
        ))
        .unwrap();
        for case in fixture["utc"].as_array().unwrap() {
            let raw = case[0].as_str().unwrap();
            assert_eq!(parse_ms(raw), case[1].as_i64(), "{raw:?}");
        }
        for raw in fixture["unread"].as_array().unwrap() {
            assert_eq!(parse_ms(raw.as_str().unwrap()), None, "{raw}");
        }
    }

    #[test]
    fn trims_what_javascript_trims() {
        assert_eq!(js_trim("\u{feff} a\u{3000}\n"), "a");
        assert_eq!(js_trim("\u{85}a"), "\u{85}a");
    }
}
