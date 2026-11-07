//! Localized relic-reward names for the desktop's reward-screen OCR.
//!
//! The game shows reward names in the player's language, so recognition needs
//! the same names it matches in English. warframe.market returns one extra
//! `i18n` language per `/v2/items` request (~2.5 MB), so each build fetches a
//! language only when it is missing, plus at most the single stalest one once
//! it is a day old: about one extra request per build after the first.
//!
//! The surface is optional and carried: a failed or implausible fetch keeps the
//! prior names for that language with their own `fetched_at`. Names are not
//! dropped with age - an old name for an existing part is still right, while
//! dropping it would stop recognition for that language entirely.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::clock;
use crate::ingest::Http;

pub const ITEMS_URL: &str = "https://api.warframe.market/v2/items";

/// Every non-English `Language` warframe.market accepts (its API overview).
/// Thai, the one other game language, has no WFM translation.
pub const LANGUAGES: &[&str] = &[
    "de", "es", "fr", "it", "ja", "ko", "pl", "pt", "ru", "tr", "uk", "zh-hans", "zh-hant",
];

const REFRESH_AFTER_HOURS: i64 = 24;
/// A response naming fewer of the current rewards than this is not a
/// translation of them - WFM ignored the header or sent a partial list.
const MIN_COVERAGE: f64 = 0.5;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalizedNames {
    pub fetched_at: String,
    /// Reward slug → the name the game shows in this language.
    pub names: BTreeMap<String, String>,
}

pub type RewardNames = BTreeMap<String, LocalizedNames>;

/// The slugs any published relic can drop.
pub fn reward_slugs(relic_rewards: &HashMap<String, serde_json::Value>) -> BTreeSet<String> {
    relic_rewards
        .values()
        .filter_map(|rewards| rewards.as_array())
        .flatten()
        .filter_map(|reward| reward.get("reward_slug").and_then(|s| s.as_str()))
        .map(str::trim)
        .filter(|slug| !slug.is_empty())
        .map(String::from)
        .collect()
}

fn prior_names(prior: &serde_json::Value) -> RewardNames {
    prior
        .get("reward_names")
        .and_then(|value| serde_json::from_value::<RewardNames>(value.clone()).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|(language, _)| LANGUAGES.contains(&language.as_str()))
        .collect()
}

/// Which languages this build fetches: all missing ones, then the stalest
/// carried language once it is past the refresh age.
fn due_languages(prior: &RewardNames, now: DateTime<Utc>) -> Vec<&'static str> {
    let mut due: Vec<&'static str> = LANGUAGES
        .iter()
        .copied()
        .filter(|language| !prior.contains_key(*language))
        .collect();
    let stalest = LANGUAGES
        .iter()
        .copied()
        .filter_map(|language| {
            let stamp = prior
                .get(language)
                .and_then(|entry| clock::parse_stamp(&entry.fetched_at));
            prior.get(language).map(|_| (language, stamp))
        })
        // An unreadable stamp sorts first: its age is unknown.
        .min_by_key(|(_, stamp)| *stamp);
    if let Some((language, stamp)) = stalest {
        let old = stamp.is_none_or(|at| now - at >= Duration::hours(REFRESH_AFTER_HOURS));
        if old {
            due.push(language);
        }
    }
    due
}

/// Parse one localized `/v2/items` body into names for the given slugs.
pub fn names_from_items(
    body: &serde_json::Value,
    language: &str,
    slugs: &BTreeSet<String>,
) -> Result<BTreeMap<String, String>, String> {
    let items = wfm_client::unwrap_envelope(body)
        .as_array()
        .ok_or_else(|| format!("{language}: items payload is not an array"))?;
    let names: BTreeMap<String, String> = items
        .iter()
        .filter_map(|item| {
            let slug = item.get("slug")?.as_str()?;
            if !slugs.contains(slug) {
                return None;
            }
            let name = item
                .get("i18n")?
                .get(language)?
                .get("name")?
                .as_str()?
                .trim();
            (!name.is_empty()).then(|| (slug.to_string(), name.to_string()))
        })
        .collect();
    let coverage = names.len() as f64 / slugs.len().max(1) as f64;
    if slugs.is_empty() || coverage < MIN_COVERAGE {
        return Err(format!(
            "{language}: names for {} of {} rewards; keeping the prior names",
            names.len(),
            slugs.len()
        ));
    }
    Ok(names)
}

/// Build this snapshot's `reward_names`. `wfm_fresh` is false when the
/// English catalog itself could not be fetched; nothing is requested then.
pub fn refresh(
    http: &dyn Http,
    prior: &serde_json::Value,
    relic_rewards: &HashMap<String, serde_json::Value>,
    wfm_fresh: bool,
    now: DateTime<Utc>,
) -> RewardNames {
    let mut names = prior_names(prior);
    let slugs = reward_slugs(relic_rewards);
    if !wfm_fresh || slugs.is_empty() {
        return names;
    }
    for language in due_languages(&names, now) {
        match http
            .get_json_localized(ITEMS_URL, language)
            .and_then(|body| names_from_items(&body, language, &slugs))
        {
            Ok(fresh) => {
                eprintln!("  reward names ({language}): {}", fresh.len());
                names.insert(
                    language.to_string(),
                    LocalizedNames {
                        fetched_at: clock::iso_z(now),
                        names: fresh,
                    },
                );
            }
            Err(error) => eprintln!("  reward names: {error}"),
        }
    }
    names
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, reason = "tests index known fixtures")]
mod tests {
    use super::*;
    use crate::ingest::FixtureHttp;
    use serde_json::json;

    fn now() -> DateTime<Utc> {
        clock::parse_stamp("2026-11-07T12:00:00Z").unwrap()
    }

    fn rewards() -> HashMap<String, serde_json::Value> {
        HashMap::from([(
            "lith_v1_relic".to_string(),
            json!([
                {"reward_slug": "volt_prime_neuroptics_blueprint", "reward_name": "Volt Prime Neuroptics Blueprint"},
                {"reward_slug": "", "reward_name": "Forma Blueprint"}
            ]),
        )])
    }

    fn items(language: &str, name: &str) -> serde_json::Value {
        json!({"data": [
            {"slug": "volt_prime_neuroptics_blueprint",
             "i18n": {"en": {"name": "Volt Prime Neuroptics Blueprint"}, language: {"name": name}}},
            {"slug": "unrelated", "i18n": {language: {"name": "Ailleurs"}}}
        ]})
    }

    fn fixture(languages: &[(&str, serde_json::Value)]) -> FixtureHttp {
        FixtureHttp {
            responses: languages
                .iter()
                .map(|(language, body)| (format!("{ITEMS_URL}#language={language}"), body.clone()))
                .collect(),
        }
    }

    #[test]
    fn first_build_fetches_every_language_and_keeps_only_rewards() {
        let http = fixture(&[("fr", items("fr", "Volt Prime - Neuroptiques (Schéma)"))]);
        let names = refresh(&http, &json!({}), &rewards(), true, now());
        assert_eq!(
            names.len(),
            1,
            "only the language that answered is published"
        );
        let fr = &names["fr"];
        assert_eq!(fr.fetched_at, "2026-11-07T12:00:00Z");
        assert_eq!(
            fr.names,
            BTreeMap::from([(
                "volt_prime_neuroptics_blueprint".to_string(),
                "Volt Prime - Neuroptiques (Schéma)".to_string()
            )])
        );
    }

    #[test]
    fn a_failed_or_untranslated_fetch_carries_the_prior_names() {
        let prior = json!({"reward_names": {"fr": {
            "fetched_at": "2026-11-01T00:00:00Z",
            "names": {"volt_prime_neuroptics_blueprint": "Ancien nom"}
        }}});
        // WFM answering in English only is not a translation.
        let http = fixture(&[("fr", items("en", "Volt Prime Neuroptics Blueprint"))]);
        let names = refresh(&http, &prior, &rewards(), true, now());
        assert_eq!(
            names["fr"].names["volt_prime_neuroptics_blueprint"],
            "Ancien nom"
        );
        assert_eq!(names["fr"].fetched_at, "2026-11-01T00:00:00Z");
    }

    #[test]
    fn nothing_is_requested_when_the_english_catalog_failed() {
        let http = fixture(&[("fr", items("fr", "Nouveau"))]);
        assert!(refresh(&http, &json!({}), &rewards(), false, now()).is_empty());
    }

    #[test]
    fn only_missing_languages_and_the_one_stalest_are_due() {
        let at = |stamp: &str| LocalizedNames {
            fetched_at: stamp.into(),
            names: BTreeMap::new(),
        };
        let mut prior: RewardNames = LANGUAGES
            .iter()
            .map(|language| (language.to_string(), at("2026-11-07T06:00:00Z")))
            .collect();
        assert!(due_languages(&prior, now()).is_empty(), "all fresh");

        prior.insert("ru".into(), at("2026-11-06T06:00:00Z"));
        prior.insert("de".into(), at("2026-11-05T06:00:00Z"));
        prior.remove("ja");
        assert_eq!(due_languages(&prior, now()), vec!["ja", "de"]);

        prior.insert("pl".into(), at("not a stamp"));
        assert_eq!(due_languages(&prior, now()), vec!["ja", "pl"]);
    }

    #[test]
    fn unknown_languages_in_the_prior_snapshot_are_dropped() {
        let prior =
            json!({"reward_names": {"xx": {"fetched_at": "2026-11-07T00:00:00Z", "names": {}}}});
        assert!(prior_names(&prior).is_empty());
    }
}
