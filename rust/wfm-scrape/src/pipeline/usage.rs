use crate::ingest::Http;
use crate::reconcile::Observation;
use crate::{de, de_extract, render};
use std::collections::HashMap;

pub(super) struct UsageSources {
    pub(super) usage_old: Option<HashMap<String, serde_json::Value>>,
    pub(super) usage_history: render::UsageHistorySurface,
    pub(super) usage_observation: Observation<HashMap<String, serde_json::Value>>,
}

fn valid_compact_usage(rows: &HashMap<String, serde_json::Value>) -> bool {
    !rows.is_empty()
        && rows.values().all(|row| {
            row.get("name")
                .and_then(|v| v.as_str())
                .is_some_and(|v| !v.is_empty())
                && row
                    .get("category")
                    .and_then(|v| v.as_str())
                    .is_some_and(|v| !v.is_empty())
                && row
                    .get("share")
                    .and_then(|v| v.as_f64())
                    .is_some_and(|v| v.is_finite() && v >= 0.0)
                && row.get("by_mr").is_none()
        })
}

fn compact_prior_usage(
    rows: &HashMap<String, serde_json::Value>,
) -> Option<(u16, HashMap<String, serde_json::Value>)> {
    let year = rows.values().next()?.get("year")?.as_u64()? as u16;
    if !de::DE_USAGE_YEARS.contains(&year) {
        return None;
    }
    let mut compact = HashMap::new();
    for (slug, row) in rows {
        if row.get("year").and_then(|v| v.as_u64()) != Some(year as u64) {
            return None;
        }
        let name = row.get("name").and_then(|v| v.as_str())?;
        let category = row.get("category").and_then(|v| v.as_str())?;
        let share = row.get("share").and_then(|v| v.as_f64())?;
        if name.is_empty() || category.is_empty() || !share.is_finite() || share < 0.0 {
            return None;
        }
        compact.insert(
            slug.clone(),
            serde_json::json!({
                "name": name,
                "category": category,
                "share": share,
            }),
        );
    }
    valid_compact_usage(&compact).then_some((year, compact))
}

fn rich_prior_usage_year(rows: &HashMap<String, serde_json::Value>) -> Option<u16> {
    let mut common_year = None;
    if rows.is_empty() {
        return None;
    }
    for row in rows.values() {
        let name = row.get("name").and_then(|value| value.as_str())?;
        let category = row.get("category").and_then(|value| value.as_str())?;
        let year = u16::try_from(row.get("year").and_then(|value| value.as_u64())?).ok()?;
        let share = row.get("share").and_then(|value| value.as_f64())?;
        let peak_mr = row.get("peak_mr").and_then(|value| value.as_f64())?;
        let by_mr = row.get("by_mr").and_then(|value| value.as_array())?;
        if name.is_empty()
            || category.is_empty()
            || !de::DE_USAGE_YEARS.contains(&year)
            || !share.is_finite()
            || share < 0.0
            || !peak_mr.is_finite()
            || peak_mr < 0.0
            || by_mr.is_empty()
            || !by_mr.iter().all(|value| {
                value
                    .as_f64()
                    .is_some_and(|value| value.is_finite() && value >= 0.0)
            })
        {
            return None;
        }
        if common_year.replace(year).is_some_and(|prior| prior != year) {
            return None;
        }
    }
    common_year
}

pub(super) fn fetch(
    http: &dyn Http,
    prior: &serde_json::Value,
    catalog: &HashMap<String, String>,
) -> UsageSources {
    // Annual usage history is immutable. Keep every valid prior year, request
    // only missing published candidates, and leave failed years absent so the
    // next cycle retries them. DE publishes these files in arrears; never
    // manufacture a current-year candidate.
    let usage_old: Option<HashMap<String, serde_json::Value>> = prior
        .get("usage")
        .and_then(|s| serde_json::from_value(s.clone()).ok());
    let mut usage_history: render::UsageHistorySurface = prior
        .get("usage_history")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default();
    usage_history
        .by_year
        .retain(|year, rows| de::DE_USAGE_YEARS.contains(year) && valid_compact_usage(rows));
    if let Some((year, compact)) = usage_old.as_ref().and_then(compact_prior_usage) {
        usage_history.by_year.entry(year).or_insert(compact);
    }

    let prior_usage_year = usage_old.as_ref().and_then(rich_prior_usage_year);
    let rich_repair_year = usage_history
        .by_year
        .keys()
        .next_back()
        .copied()
        .filter(|year| prior_usage_year != Some(*year));
    let mut fresh_rich_usage = std::collections::BTreeMap::new();
    for year in de::DE_USAGE_YEARS {
        let has_compact = usage_history.by_year.contains_key(year);
        if has_compact && rich_repair_year != Some(*year) {
            eprintln!("Usage telemetry: {year} already in the snapshot - not refetched");
            continue;
        }
        if has_compact {
            eprintln!("Fetching DE usage telemetry ({year}) to repair rich usage...");
        } else {
            eprintln!("Fetching DE usage telemetry ({year})...");
        }
        match http.get_json(&de::usage_url(*year)) {
            Ok(doc) => {
                let (compact, accepted, unmatched) =
                    de_extract::usage_history_from_export(&doc, catalog);
                eprintln!(
                    "  {year}: {} joined · {unmatched} unmatched · {accepted} valid rows",
                    compact.len()
                );
                if valid_compact_usage(&compact) {
                    let (rich, _) = de_extract::usage_from_export(&doc, *year, catalog);
                    if !has_compact {
                        usage_history.by_year.insert(*year, compact);
                    }
                    fresh_rich_usage.insert(*year, rich);
                } else {
                    eprintln!("  warning: {year} usage shape had no joinable valid rows");
                }
            }
            Err(e) => eprintln!("  warning: {year}: {e}"),
        }
    }
    usage_history.years = usage_history.by_year.keys().copied().collect();
    let newest_usage_year = usage_history.years.last().copied().unwrap_or(0);
    let usage_observation = if let Some(rich) = fresh_rich_usage.remove(&newest_usage_year) {
        Observation::usable(rich)
    } else if prior_usage_year == Some(newest_usage_year) {
        Observation::Unchanged
    } else {
        Observation::Unavailable
    };

    UsageSources {
        usage_old,
        usage_history,
        usage_observation,
    }
}
