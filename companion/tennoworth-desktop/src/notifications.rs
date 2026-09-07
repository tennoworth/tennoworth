//! Durable notifications. Producers describe evidence; this module owns delivery.
use crate::db::Db;
use std::collections::BTreeMap;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;

pub const EVENT: &str = "notifications-changed";
pub const CATEGORIES: &[&str] = &["trades", "watches", "scans", "baro", "calendar", "digest"];

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
pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

pub fn publish(app: &AppHandle, candidate: Candidate) -> Result<bool, String> {
    let db = app.state::<Db>();
    let prefs = db.notification_preferences()?;
    let pref = prefs
        .categories
        .get(&candidate.category)
        .ok_or("Unknown notification category")?;
    let native = prefs.popups && pref.native;
    let Some(id) = db
        .insert_notification(&candidate, now(), native, pref.enabled)
        .map_err(|e| e.to_string())?
    else {
        return Ok(false);
    };
    if native {
        // Platform errors are deliberately not stored: they may contain local paths.
        let delivery = if app
            .notification()
            .builder()
            .title(&candidate.title)
            .body(&candidate.body)
            .show()
            .is_ok()
        {
            "sent"
        } else {
            "failed"
        };
        db.notification_delivery(id, delivery)
            .map_err(|e| e.to_string())?;
    }
    db.prune_notifications(now()).map_err(|e| e.to_string())?;
    let _ = app.emit(EVENT, ());
    Ok(true)
}
pub fn send(app: &AppHandle, candidate: Candidate) {
    if let Err(e) = publish(app, candidate) {
        eprintln!("tennoworth: notification could not be recorded: {e}");
    }
}

#[tauri::command]
pub fn list_notifications(db: State<'_, Db>) -> Result<Vec<Notification>, String> {
    db.prune_notifications(now()).map_err(|e| e.to_string())?;
    db.list_notifications().map_err(|e| e.to_string())
}
#[tauri::command]
pub fn mark_notifications_read(
    app: AppHandle,
    db: State<'_, Db>,
    id: Option<i64>,
) -> Result<(), String> {
    db.mark_notifications_read(id).map_err(|e| e.to_string())?;
    let _ = app.emit(EVENT, ());
    Ok(())
}
#[tauri::command]
pub fn clear_notifications(app: AppHandle, db: State<'_, Db>) -> Result<(), String> {
    db.clear_notifications().map_err(|e| e.to_string())?;
    let _ = app.emit(EVENT, ());
    Ok(())
}
#[tauri::command]
pub fn get_notification_preferences(db: State<'_, Db>) -> Result<Preferences, String> {
    db.notification_preferences()
}
#[tauri::command]
pub fn set_notification_preferences(
    db: State<'_, Db>,
    preferences: Preferences,
) -> Result<Preferences, String> {
    if preferences.categories.len() != CATEGORIES.len()
        || CATEGORIES
            .iter()
            .any(|k| !preferences.categories.contains_key(*k))
    {
        return Err("Notification categories are incomplete".into());
    }
    db.set_setting(
        "notifications-v1",
        &serde_json::to_string(&preferences).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(preferences)
}
#[tauri::command]
pub fn test_notification(app: AppHandle) -> Result<String, String> {
    // An explicit test exercises native delivery even when routine popups are paused.
    app.notification().builder().title("TennoWorth notifications").body("Your desktop notification test.").show().map_err(|_| "Desktop notification delivery failed. Check your operating system notification settings.".to_string())?;
    Ok(
        "Test sent. If no popup appeared, check your operating system notification settings."
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contracts_match_the_frontend_fixture() {
        let v: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/notifications/contracts.json"
        ))
        .unwrap();
        assert_eq!(EVENT, v["event"]);
        assert_eq!(crate::reminders::MARKET_EVENT, v["market_event"]);
        assert_eq!(serde_json::to_value(CATEGORIES).unwrap(), v["categories"]);
        let defaults = Preferences::default();
        assert!(defaults.popups && defaults.categories.values().all(|c| c.enabled && c.native));
    }
}
