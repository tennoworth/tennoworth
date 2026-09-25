//! The notification contract: what a notification is, what a producer proposes,
//! and which categories the storage layer, the service, and the settings UI all
//! have to agree on.
//!
//! Leaf module on purpose. `services::notifications` owns delivery (dedupe,
//! cooldown, native popup) and `persistence::notifications` owns the rows, so
//! both depend on this and neither depends on the other. While these types lived
//! in the service, storage had to reach upward for the very shapes it persists.

use std::collections::BTreeMap;

pub const CATEGORIES: &[&str] = &["trades", "watches", "baro", "calendar", "digest"];

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct CategoryPreference {
    pub enabled: bool,
    pub native: bool,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Preferences {
    pub popups: bool,
    pub categories: BTreeMap<String, CategoryPreference>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            popups: true,
            categories: CATEGORIES
                .iter()
                .map(|k| {
                    (
                        k.to_string(),
                        CategoryPreference {
                            enabled: true,
                            native: true,
                        },
                    )
                })
                .collect(),
        }
    }
}
/// The category set storage and the settings UI must agree on. One predicate so
/// the read-side normalization and the write-side rejection cannot drift.
pub fn categories_match_contract(categories: &BTreeMap<String, CategoryPreference>) -> bool {
    categories.len() == CATEGORIES.len() && CATEGORIES.iter().all(|k| categories.contains_key(*k))
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Notification {
    pub id: i64,
    pub category: String,
    pub title: String,
    pub body: String,
    pub target: String,
    pub created_at: i64,
    pub read: bool,
    pub delivery: String,
}
#[derive(Clone, Debug)]
pub struct Candidate {
    pub key: String,
    pub stage: i64,
    pub expires_at: i64,
    pub cooldown: i64,
    pub category: String,
    pub title: String,
    pub body: String,
    pub target: String,
}
impl Candidate {
    pub fn once(
        key: String,
        category: &str,
        title: String,
        body: String,
        target: &str,
        now: i64,
    ) -> Self {
        Self {
            key,
            stage: 1,
            expires_at: now + 31 * 86400,
            cooldown: 0,
            category: category.into(),
            title,
            body,
            target: target.into(),
        }
    }
}
