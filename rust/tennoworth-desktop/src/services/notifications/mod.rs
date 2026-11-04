//! Durable notifications. Producers describe evidence; this module owns delivery.

pub(crate) mod reminders;

use crate::persistence::Db;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;

// The contract lives beside the layers that share it; re-exported here because
// every producer already names it through this module.
pub use crate::contracts::notification::{
    categories_match_contract, Candidate, Notification, Preferences, IN_GAME_CATEGORIES,
};

/// Emitted when the inbox changes, so an open surface reloads without polling.
pub const EVENT: &str = "notifications-changed";

/// What a notification hands to in-game delivery.
pub struct InGameRequest<'a> {
    pub id: i64,
    pub title: &'a str,
    pub detail: &'a str,
    pub whisper: Option<&'a str>,
    /// The player asked for the whisper to be copied when the card shows.
    pub copy_whisper: bool,
}

type InGameSink = Box<dyn Fn(&InGameRequest<'_>) -> bool + Send + Sync>;

/// Where "Show in game" notifications go. Startup installs the overlay here,
/// so this service never has to know the overlay exists. With nothing
/// installed every notification is delivered the ordinary way.
#[derive(Default)]
pub struct InGameDelivery(std::sync::RwLock<Option<InGameSink>>);

impl InGameDelivery {
    pub fn install(&self, sink: impl Fn(&InGameRequest<'_>) -> bool + Send + Sync + 'static) {
        *self.0.write().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(sink));
    }

    /// True when the card was shown; false means "deliver it the ordinary way".
    fn deliver(&self, request: &InGameRequest<'_>) -> bool {
        self.0
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|sink| sink(request))
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
    // Decided after the row is stored: the inbox keeps the record whichever
    // way it is delivered, and polling and streaming have already been
    // deduplicated by the insert.
    if wants_in_game(&prefs, &candidate.category)
        && app.try_state::<InGameDelivery>().is_some_and(|sink| {
            sink.deliver(&InGameRequest {
                id,
                title: &candidate.title,
                detail: &candidate.body,
                whisper: candidate.whisper.as_deref(),
                copy_whisper: prefs.copy_whisper,
            })
        })
    {
        db.notification_delivery(id, "in_game")
            .map_err(|e| e.to_string())?;
    } else if native {
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
/// "Show in game" is offered per category, and pausing popups pauses the card
/// too: both interrupt, and the switch promises alerts without interruptions.
fn wants_in_game(prefs: &Preferences, category: &str) -> bool {
    prefs.popups
        && IN_GAME_CATEGORIES.contains(&category)
        && prefs.categories.get(category).is_some_and(|pref| pref.in_game)
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
    if !categories_match_contract(&preferences.categories) {
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
            "../../../../../tests/fixtures/notifications/contracts.json"
        ))
        .unwrap();
        assert_eq!(EVENT, v["event"]);
        assert_eq!(
            crate::services::notifications::reminders::MARKET_EVENT,
            v["market_event"]
        );
        assert_eq!(
            serde_json::to_value(crate::contracts::notification::CATEGORIES).unwrap(),
            v["categories"]
        );
        let defaults = Preferences::default();
        assert!(defaults.popups && defaults.categories.values().all(|c| c.enabled && c.native));
        assert!(!defaults.copy_whisper && defaults.categories.values().all(|c| !c.in_game));
    }

    #[test]
    fn only_an_offered_category_that_chose_it_shows_in_game_and_pausing_popups_pauses_it() {
        let mut prefs = Preferences::default();
        assert!(!wants_in_game(&prefs, "watches"), "off by default");
        for pref in prefs.categories.values_mut() {
            pref.in_game = true;
        }
        assert!(wants_in_game(&prefs, "watches"));
        assert!(!wants_in_game(&prefs, "baro"), "not offered for baro");
        assert!(!wants_in_game(&prefs, "nonsense"));
        prefs.popups = false;
        assert!(!wants_in_game(&prefs, "watches"));
    }

    #[test]
    fn settings_saved_before_in_game_existed_read_as_off() {
        let stored = r#"{"popups":true,"categories":{"watches":{"enabled":true,"native":false}}}"#;
        let prefs: Preferences = serde_json::from_str(stored).unwrap();
        assert!(!prefs.copy_whisper);
        assert!(prefs.categories.get("watches").is_some_and(|p| !p.in_game && !p.native));
    }
}
