//! Meta Drift: DE's annual usage shares, the latest two years that actually
//! carry rows, compared item by item within one category. An item in only one
//! of the years is listed as such, never zero-filled into a change, and a
//! category move is counted rather than compared across categories.

use crate::math::collate::{locale_cmp, sorted};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Ordering;
use ts_rs::TS;

#[derive(Debug, Clone, Deserialize, TS)]
pub struct MetaDriftRequest {
    /// `usage_history` as published.
    #[ts(type = "unknown")]
    pub usage_history: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DriftShare {
    pub slug: String,
    pub name: String,
    pub category: String,
    pub prior_share: f64,
    pub current_share: f64,
    pub delta_pp: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct OnlyShare {
    pub slug: String,
    pub name: String,
    pub category: String,
    pub year: f64,
    pub share: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MetaDrift {
    pub prior_year: f64,
    pub current_year: f64,
    /// Years between the two snapshots that DE did not publish.
    pub missing_years: Vec<f64>,
    pub gains: Vec<DriftShare>,
    pub losses: Vec<DriftShare>,
    pub only_prior: Vec<OnlyShare>,
    pub only_current: Vec<OnlyShare>,
    pub category_changes: usize,
    pub categories: Vec<String>,
}

struct Usage<'a> {
    name: &'a str,
    category: &'a str,
    share: f64,
}

fn usage(value: Option<&Value>) -> Option<Usage<'_>> {
    let row = value?.as_object()?;
    let text = |key| {
        row.get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    };
    let share = row
        .get("share")
        .and_then(Value::as_f64)
        .filter(|s| *s >= 0.0)?;
    Some(Usage {
        name: text("name")?,
        category: text("category")?,
        share,
    })
}

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// `by_year[String(year)]`: an object's key, or an array's index.
fn year_rows(by_year: &Value, year: f64) -> Option<&serde_json::Map<String, Value>> {
    let raw = match by_year {
        Value::Object(map) => map.get(&format!("{year}")),
        Value::Array(list) => list.get(year as usize),
        _ => None,
    };
    raw?.as_object()
}

pub fn meta_drift(request: &MetaDriftRequest) -> Result<Option<MetaDrift>, String> {
    let Some(history) = request.usage_history.as_object() else {
        return Ok(None);
    };
    let by_year = match history.get("by_year") {
        Some(v @ (Value::Object(_) | Value::Array(_))) => v,
        _ => return Ok(None),
    };
    // `for (const value of years ?? [])`: a string iterates characters, none
    // of them a year; anything else that is not a list throws there.
    let declared: &[Value] = match history.get("years") {
        None | Some(Value::Null | Value::String(_)) => &[],
        Some(Value::Array(years)) => years,
        Some(_) => return Err("usage_history.years is not a list.".into()),
    };
    let mut years: Vec<f64> = declared
        .iter()
        .filter_map(Value::as_f64)
        .filter(|y| y.fract() == 0.0 && *y > 0.0 && *y <= MAX_SAFE_INTEGER)
        .collect();
    years.sort_by(f64::total_cmp);
    years.dedup();
    let available: Vec<(f64, &serde_json::Map<String, Value>)> = years
        .into_iter()
        .filter_map(|year| year_rows(by_year, year).map(|rows| (year, rows)))
        .filter(|(_, rows)| rows.values().any(|row| usage(Some(row)).is_some()))
        .collect();
    let [.., (prior_year, prior), (current_year, current)] = available.as_slice() else {
        return Ok(None);
    };
    let (mut gains, mut losses, mut only_prior, mut only_current) =
        (vec![], vec![], vec![], vec![]);
    let mut category_changes = 0;
    let slugs = prior
        .keys()
        .chain(current.keys().filter(|slug| !prior.contains_key(*slug)));
    for slug in slugs {
        let only = |u: Usage<'_>, year: f64| OnlyShare {
            slug: slug.clone(),
            name: u.name.to_owned(),
            category: u.category.to_owned(),
            year,
            share: u.share,
        };
        match (usage(prior.get(slug)), usage(current.get(slug))) {
            (None, None) => {}
            (None, Some(after)) => only_current.push(only(after, *current_year)),
            (Some(before), None) => only_prior.push(only(before, *prior_year)),
            (Some(before), Some(after)) if before.category != after.category => {
                category_changes += 1
            }
            (Some(before), Some(after)) => {
                let row = DriftShare {
                    slug: slug.clone(),
                    name: after.name.to_owned(),
                    category: after.category.to_owned(),
                    prior_share: before.share,
                    current_share: after.share,
                    delta_pp: after.share - before.share,
                };
                if row.delta_pp > 0.0 {
                    gains.push(row);
                } else if row.delta_pp < 0.0 {
                    losses.push(row);
                }
            }
        }
    }
    let tie =
        |a: (&str, &str), b: (&str, &str)| locale_cmp(a.0, b.0).then_with(|| locale_cmp(a.1, b.1));
    let by_delta = |a: f64, b: f64| a.partial_cmp(&b).unwrap_or(Ordering::Equal);
    let gains = sorted(gains, &|a, b| {
        by_delta(b.delta_pp, a.delta_pp).then_with(|| tie((&a.slug, &a.name), (&b.slug, &b.name)))
    });
    let losses = sorted(losses, &|a, b| {
        by_delta(a.delta_pp, b.delta_pp).then_with(|| tie((&a.slug, &a.name), (&b.slug, &b.name)))
    });
    let only_current = sorted(only_current, &|a, b| {
        tie((&a.slug, &a.name), (&b.slug, &b.name))
    });
    let only_prior = sorted(only_prior, &|a, b| {
        tie((&a.slug, &a.name), (&b.slug, &b.name))
    });
    let mut categories: Vec<String> = Vec::new();
    let all = gains.iter().chain(&losses).map(|r| &r.category);
    for category in all.chain(only_current.iter().chain(&only_prior).map(|r| &r.category)) {
        if !categories.contains(category) {
            categories.push(category.clone());
        }
    }
    let categories = sorted(categories, &|a, b| locale_cmp(a, b));
    let missing_years = (1..)
        .map(|n| prior_year + f64::from(n))
        .take_while(|year| year < current_year)
        .collect();
    Ok(Some(MetaDrift {
        prior_year: *prior_year,
        current_year: *current_year,
        missing_years,
        gains,
        losses,
        only_prior,
        only_current,
        category_changes,
        categories,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn drift(by_year: Value, years: Value) -> Option<MetaDrift> {
        meta_drift(&MetaDriftRequest {
            usage_history: json!({ "years": years, "by_year": by_year }),
        })
        .unwrap()
    }

    fn row(name: &str, category: &str, share: f64) -> Value {
        json!({ "name": name, "category": category, "share": share })
    }

    #[test]
    fn compares_the_latest_two_years_and_names_the_gap() {
        let model = drift(
            json!({
                "2023": { "gain_b": row("Gain B", "Primary", 1.0), "gain_a": row("Gain A", "Primary", 1.0), "loss": row("Loss", "Primary", 3.0) },
                "2025": { "gain_b": row("Gain B", "Primary", 2.0), "gain_a": row("Gain A", "Primary", 2.0), "loss": row("Loss", "Primary", 1.0) },
            }),
            json!([2023, 2025]),
        )
        .unwrap();
        assert_eq!(model.missing_years, vec![2024.0]);
        assert_eq!(
            model
                .gains
                .iter()
                .map(|r| r.slug.as_str())
                .collect::<Vec<_>>(),
            ["gain_a", "gain_b"]
        );
        assert_eq!(model.gains[0].delta_pp, 1.0);
        assert_eq!(
            model
                .losses
                .iter()
                .map(|r| (r.slug.as_str(), r.delta_pp))
                .collect::<Vec<_>>(),
            [("loss", -2.0)]
        );
    }

    #[test]
    fn orders_gains_and_losses_by_size() {
        let model = drift(
            json!({
                "2024": { "small": row("Small", "Primary", 1.0), "big": row("Big", "Primary", 1.0), "drop": row("Drop", "Primary", 5.0), "dip": row("Dip", "Primary", 5.0) },
                "2025": { "small": row("Small", "Primary", 1.5), "big": row("Big", "Primary", 4.0), "drop": row("Drop", "Primary", 1.0), "dip": row("Dip", "Primary", 4.5) },
            }),
            json!([2024, 2025]),
        )
        .unwrap();
        let slugs = |rows: &[DriftShare]| rows.iter().map(|r| r.slug.clone()).collect::<Vec<_>>();
        assert_eq!(slugs(&model.gains), ["big", "small"]);
        assert_eq!(slugs(&model.losses), ["drop", "dip"]);
        assert!(model.missing_years.is_empty());
    }

    #[test]
    fn lists_one_year_rows_and_counts_category_moves() {
        let model = drift(
            json!({
                "2023": { "only_old": row("Only Old", "Primary", 2.0), "changed": row("Changed", "Primary", 1.0), "same": row("Same", "Melee", 1.0), "steady": row("Steady", "Primary", 1.0) },
                "2025": { "only_new": row("Only New", "Secondary", 3.0), "changed": row("Changed", "Secondary", 2.0), "same": row("Same", "Melee", 1.0), "steady": row("Steady", "Primary", 2.0) },
            }),
            json!([2023, 2025]),
        )
        .unwrap();
        assert_eq!(model.gains.len(), 1);
        assert!(model.losses.is_empty());
        assert_eq!(
            model.only_current[0],
            OnlyShare {
                slug: "only_new".into(),
                name: "Only New".into(),
                category: "Secondary".into(),
                year: 2025.0,
                share: 3.0
            }
        );
        assert_eq!(model.only_prior[0].slug, "only_old");
        assert_eq!(model.category_changes, 1);
        assert_eq!(model.categories, ["Primary", "Secondary"]);
    }

    #[test]
    fn needs_two_years_with_valid_rows() {
        assert!(drift(json!({ "2023": { "bad": row("Bad", "Primary", -1.0) }, "2025": { "bad": row("Bad", "Primary", 2.0) } }), json!([2023, 2025])).is_none());
        assert!(drift(
            json!({ "2023": { "one": row("One", "Primary", 1.0) } }),
            json!([2023])
        )
        .is_none());
        assert!(meta_drift(&MetaDriftRequest {
            usage_history: Value::Null
        })
        .unwrap()
        .is_none());
        assert!(drift(json!({}), json!("2023")).is_none());
        assert!(meta_drift(&MetaDriftRequest {
            usage_history: json!({ "years": 5, "by_year": {} })
        })
        .is_err());
    }

    #[test]
    fn a_zero_share_is_a_reading() {
        let model = drift(
            json!({ "2024": { "rose": row("Rose", "Primary", 0.0) }, "2025": { "rose": row("Rose", "Primary", 2.0) } }),
            json!([2024, 2025]),
        )
        .unwrap();
        assert_eq!(model.gains[0].prior_share, 0.0);
        assert!(model.only_current.is_empty());
    }

    #[test]
    fn skips_declared_years_without_rows() {
        let model = drift(
            json!({ "2021": { "old": row("Old", "Primary", 1.0) }, "2024": {}, "2025": { "old": row("Old", "Primary", 2.0) } }),
            json!([2021, 2024, 2025, 2025.5, -1]),
        )
        .unwrap();
        assert_eq!((model.prior_year, model.current_year), (2021.0, 2025.0));
        assert_eq!(model.missing_years, vec![2022.0, 2023.0, 2024.0]);
    }
}
