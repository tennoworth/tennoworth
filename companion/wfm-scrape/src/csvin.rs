//! CSV input — mirrors Python's `csv.DictReader` over `wfm_results.csv`.
//!
//! Each row carries the fields the render stage reads. Old CSVs lack
//! several columns (the scraper grew fields over time); every accessor
//! defaults are contract from the Python converter.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct CsvRow {
    pub url_name: String,
    pub name: String,
    pub tags: String,
    pub ducats: String,
    pub live_buys: String,
    pub live_sells: String,
    pub buy_sell_ratio: String,
    pub top_buy_price: String,
    pub low_sell_price: String,
    pub low5_avg: String,
    pub spread: String,
    pub volume_48h: String,
    pub avg_price_48h: String,
    pub median_now: String,
    pub median_90d: String,
    pub medians_7d: String,
    pub donch_top_90d: String,
    pub donch_bot_90d: String,
    pub score: String,
}

impl CsvRow {
    fn from_headers_and_fields(headers: &[String], fields: &[String]) -> Self {
        let map: HashMap<&str, &str> = headers.iter().map(|h| h.as_str()).zip(fields.iter().map(|f| f.as_str())).collect();
        let g = |k: &str| map.get(k).copied().unwrap_or("").to_string();
        CsvRow {
            url_name: g("url_name"),
            name: g("name"),
            tags: g("tags"),
            ducats: g("ducats"),
            live_buys: g("live_buys"),
            live_sells: g("live_sells"),
            buy_sell_ratio: g("buy_sell_ratio"),
            top_buy_price: g("top_buy_price"),
            low_sell_price: g("low_sell_price"),
            low5_avg: g("low5_avg"),
            spread: g("spread"),
            volume_48h: g("volume_48h"),
            avg_price_48h: g("avg_price_48h"),
            median_now: g("median_now"),
            median_90d: g("median_90d"),
            medians_7d: g("medians_7d"),
            donch_top_90d: g("donch_top_90d"),
            donch_bot_90d: g("donch_bot_90d"),
            score: g("score"),
        }
    }
}

/// All rows from a CSV, parsed into [`CsvRow`]s. The first line is the header.
pub fn read_csv_rows(path: &Path) -> Result<Vec<CsvRow>, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("cannot open {path:?}: {e}"))?;
    let mut s = String::new();
    f.read_to_string(&mut s).map_err(|e| format!("cannot read {path:?}: {e}"))?;
    read_csv(&s)
}

/// Like [`read_csv_rows`] but takes the raw content (useful in tests).
pub fn read_csv(content: &str) -> Result<Vec<CsvRow>, String> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .trim(csv::Trim::All)
        .from_reader(content.as_bytes());
    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| format!("CSV header: {e}"))?
        .iter()
        .map(|h| h.to_string())
        .collect();
    let mut rows = Vec::new();
    for result in reader.records() {
        let record = result.map_err(|e| format!("CSV row: {e}"))?;
        let fields: Vec<String> = record.iter().map(|f| f.to_string()).collect();
        rows.push(CsvRow::from_headers_and_fields(&headers, &fields));
    }
    Ok(rows)
}

/// Parse the `medians_7d` field from a CSV row.
///
/// CSV stores the 7-day median series as the Python `repr` of a list
/// (`str()` on the list, which uses single quotes for strings).
/// Parse defensively: `""` → `[]`, old CSV with no column → `[]`,
/// malformed → `[]`. Mirror of Python's `_parse_medians`.
pub fn parse_medians_7d(raw: &str) -> Vec<f64> {
    if raw.is_empty() || raw == "[]" {
        return vec![];
    }
    let cleaned = raw.replace('\'', "\"");
    let v: serde_json::Value = serde_json::from_str(&cleaned).unwrap_or(serde_json::Value::Array(vec![]));
    match v {
        serde_json::Value::Array(arr) => arr.iter().filter_map(|x| x.as_f64()).collect(),
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_brackets_are_empty() {
        let empty: Vec<f64> = vec![];
        assert_eq!(parse_medians_7d(""), empty);
        assert_eq!(parse_medians_7d("[]"), empty);
    }

    #[test]
    fn parses_integer_list_repr() {
        assert_eq!(parse_medians_7d("[10, 12, 8, 14, 11, 10, 9]"), vec![10.0, 12.0, 8.0, 14.0, 11.0, 10.0, 9.0]);
    }

    #[test]
    fn parses_float_list_repr() {
        assert_eq!(parse_medians_7d("[10.5, 12.3, 8.1]"), vec![10.5, 12.3, 8.1]);
    }

    #[test]
    fn malformed_returns_empty_or_partial() {
        let empty: Vec<f64> = vec![];
        assert_eq!(parse_medians_7d("garbage"), empty);
        // numeric entries survive, non-numeric filtered (mirrors Python)
        assert_eq!(parse_medians_7d("[1, 3]"), vec![1.0, 3.0]);
    }

    #[test]
    fn read_csv_parses_all_rows() {
        let csv = "\
url_name,name,tags,ducats,live_buys,live_sells,buy_sell_ratio,top_buy_price,low_sell_price,low5_avg,spread,volume_48h,avg_price_48h,median_now,median_90d,medians_7d,donch_top_90d,donch_bot_90d,score
primed_continuity,Primed Continuity,mod,0,12,18,0.67,35,42,42.6,16.7,384,43.2,37,33,\"[33, 36, 42]\",45,15,12
";
        let rows = read_csv(csv).unwrap();
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.url_name, "primed_continuity");
        assert_eq!(r.name, "Primed Continuity");
        assert_eq!(r.low_sell_price, "42");
        assert_eq!(r.volume_48h, "384");
        assert_eq!(parse_medians_7d(&r.medians_7d), vec![33.0, 36.0, 42.0]);
    }

    /// Old CSVs lack columns that were added later — missing keys must
    /// default to the empty string without panicking.
    #[test]
    fn missing_column_defaults_to_empty_string() {
        let csv = "\
url_name,name,low_sell_price,volume_48h
primed_continuity,Primed Continuity,42,384
";
        let rows = read_csv(csv).unwrap();
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.url_name, "primed_continuity");
        assert_eq!(r.name, "Primed Continuity");
        assert_eq!(r.low_sell_price, "42");
        assert_eq!(r.volume_48h, "384");
        // Missing columns default to ""
        assert_eq!(r.median_now, "");
        assert_eq!(r.medians_7d, "");
    }
}
