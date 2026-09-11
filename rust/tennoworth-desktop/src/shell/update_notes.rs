//! Installed-release notes are bundled; neither startup nor Settings fetches release text.
#![allow(
    clippy::unreachable,
    reason = "Tauri command wrappers expand to unreachable"
)]
use crate::persistence::Db;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Mutex;
use tauri::{Manager, State, WebviewWindow};

const HISTORY: &str = "update-notes.history-v1";
const BUNDLE: &str = include_str!("../../resources/update-notes.json");

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Version(u64, u64, u64);
fn version(text: &str) -> Result<Version, String> {
    let mut parts = text.split('.');
    let mut number = || {
        let part = parts.next().ok_or("Invalid version")?;
        if part.is_empty()
            || !part.bytes().all(|c| c.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return Err("Invalid version".to_string());
        }
        let value = part.parse::<u64>().map_err(|_| "Invalid version")?;
        if value > 9_007_199_254_740_991 {
            return Err("Invalid version".into());
        }
        Ok(value)
    };
    let parsed = Version(number()?, number()?, number()?);
    if parts.next().is_some() {
        return Err("Invalid version".into());
    }
    Ok(parsed)
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Improved,
    Fixed,
    Action,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    id: String,
    kind: Kind,
    title: String,
    body: String,
    platforms: Vec<String>,
    supersedes: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    version: String,
    date: String,
    changes: Vec<Change>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    version: String,
    coverage_since: String,
    releases: Vec<Release>,
}
impl Catalog {
    fn parse(raw: &str, installed: &str) -> Result<Self, String> {
        let catalog: Self =
            serde_json::from_str(raw).map_err(|_| "Installed release notes are unavailable.")?;
        let current = version(installed)?;
        if catalog.version != installed
            || catalog.releases.is_empty()
            || catalog.releases.len() > 1000
        {
            return Err("Installed release notes are unavailable.".into());
        }
        let mut previous = version(&catalog.coverage_since)?;
        for release in &catalog.releases {
            let next = version(&release.version)?;
            if next <= previous
                || next > current
                || release.changes.is_empty()
                || release.changes.len() > 100
            {
                return Err("Installed release history is incomplete.".into());
            }
            previous = next;
        }
        if previous != current {
            return Err("Installed release notes are missing.".into());
        }
        Ok(catalog)
    }
    fn summary(
        &self,
        from: Option<&str>,
        platform: &str,
    ) -> Result<(Vec<Release>, Vec<Change>, bool), String> {
        let since = from.map(version).transpose()?;
        let mut releases = Vec::new();
        for release in self.releases.iter().rev() {
            if since.is_some_and(|v| version(&release.version).is_ok_and(|r| r <= v)) {
                continue;
            }
            if since.is_none() && release.version != self.version {
                continue;
            }
            let mut filtered = release.clone();
            filtered
                .changes
                .retain(|c| c.platforms.iter().any(|p| p == platform));
            releases.push(filtered);
        }
        // Reopening an acknowledged release still shows the installed release's notes.
        if releases.is_empty() {
            if let Some(latest) = self.releases.last() {
                let mut latest = latest.clone();
                latest
                    .changes
                    .retain(|c| c.platforms.iter().any(|p| p == platform));
                releases.push(latest);
            }
        }
        let mut seen = HashSet::new();
        let superseded: HashSet<_> = releases
            .iter()
            .flat_map(|r| &r.changes)
            .flat_map(|c| c.supersedes.iter().cloned())
            .collect();
        let mut changes = Vec::new();
        for release in &releases {
            for change in &release.changes {
                if !seen.insert(change.id.clone()) {
                    continue;
                }
                if change.kind != Kind::Action && superseded.contains(&change.id) {
                    continue;
                }
                changes.push(change.clone());
            }
        }
        let partial =
            since.is_some_and(|v| version(&self.coverage_since).is_ok_and(|floor| v < floor));
        Ok((releases, changes, partial))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct History {
    last_run: String,
    acknowledged: Option<String>,
    summary_from: Option<String>,
}
impl History {
    fn valid(&self) -> bool {
        version(&self.last_run).is_ok()
            && self
                .acknowledged
                .as_deref()
                .is_none_or(|v| version(v).is_ok())
            && self
                .summary_from
                .as_deref()
                .is_none_or(|v| version(v).is_ok())
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct NotesStatus {
    current_version: String,
    previous_version: Option<String>,
    auto_show: bool,
    earlier_version_unknown: bool,
    partial_history: bool,
    releases: Vec<Release>,
    changes: Vec<Change>,
}
struct Session {
    status: NotesStatus,
    history: History,
    persist: bool,
}
pub struct UpdateNotes(Mutex<Result<Session, String>>);

fn session(
    db: &Db,
    installed: &str,
    existed: bool,
    automatic: bool,
    platform: &str,
    catalog: &Catalog,
) -> Result<Session, String> {
    let current = version(installed)?;
    let raw = db.get_setting(HISTORY);
    let history = raw
        .as_ref()
        .ok()
        .and_then(|v| v.as_deref())
        .and_then(|v| serde_json::from_str::<History>(v).ok())
        .filter(History::valid);
    let fresh = !existed && matches!(raw, Ok(None));
    let mut history = history.unwrap_or(History {
        last_run: installed.into(),
        acknowledged: fresh.then(|| installed.into()),
        summary_from: None,
    });
    let downgrade = version(&history.last_run)? > current;
    let unread = history
        .acknowledged
        .as_deref()
        .map(version)
        .transpose()?
        .is_none_or(|v| v < current);
    if version(&history.last_run)? < current {
        history.summary_from = history.acknowledged.clone();
    }
    let unknown = history.acknowledged.is_none();
    let previous = if unread {
        history.acknowledged.clone()
    } else {
        history.summary_from.clone()
    };
    let previous = previous.filter(|v| version(v).is_ok_and(|v| v < current));
    let (releases, changes, partial_history) = catalog.summary(previous.as_deref(), platform)?;
    history.last_run = installed.into();
    if automatic && raw.is_ok() {
        // A failed marker write must not prevent app startup. Explicit dismissal retries it.
        let _ = db.set_setting(
            HISTORY,
            &serde_json::to_string(&history).map_err(|_| "Could not record update history.")?,
        );
    }
    Ok(Session {
        status: NotesStatus {
            current_version: installed.into(),
            previous_version: previous,
            auto_show: automatic && !fresh && !downgrade && unread,
            earlier_version_unknown: !fresh && unknown,
            partial_history,
            releases,
            changes,
        },
        history,
        persist: automatic,
    })
}
pub fn initialize(app: &tauri::AppHandle, existed: bool) {
    let automatic = !cfg!(any(debug_assertions, test))
        && option_env!("TENNOWORTH_OCR_TEST_BUILD") != Some("1")
        && std::env::var_os("TENNOWORTH_PROBE").is_none()
        && std::env::var_os("TENNOWORTH_OCR_BOOT_PROBE").is_none();
    let installed = env!("CARGO_PKG_VERSION");
    let result = Catalog::parse(BUNDLE, installed).and_then(|catalog| {
        session(
            &app.state::<Db>(),
            installed,
            existed,
            automatic,
            std::env::consts::OS,
            &catalog,
        )
    });
    app.manage(UpdateNotes(Mutex::new(result)));
}
#[tauri::command]
pub fn update_notes(state: State<'_, UpdateNotes>) -> Result<NotesStatus, String> {
    state
        .0
        .lock()
        .map_err(|_| "Release notes are unavailable.")?
        .as_ref()
        .map(|s| s.status.clone())
        .map_err(Clone::clone)
}
fn acknowledge(db: &Db, state: &UpdateNotes, installed: &str) -> Result<(), String> {
    let mut guard = state
        .0
        .lock()
        .map_err(|_| "Release notes are unavailable.")?;
    let session = guard.as_mut().map_err(|e| e.clone())?;
    if installed != session.status.current_version {
        return Err("These notes belong to a different installed version.".into());
    }
    session.status.auto_show = false;
    if !session.persist {
        return Ok(());
    }
    let mut history = session.history.clone();
    // Another acknowledgement must not move the persisted high-water mark backwards.
    if let Some(raw) = db
        .get_setting(HISTORY)
        .map_err(|_| "Could not save that you read these notes.")?
    {
        if let Ok(saved) = serde_json::from_str::<History>(&raw) {
            if let Some(saved) = saved.acknowledged {
                if version(&saved)? > version(installed)? {
                    history.acknowledged = Some(saved);
                }
            }
        }
    }
    let current = version(installed)?;
    if history
        .acknowledged
        .as_deref()
        .map(version)
        .transpose()?
        .is_none_or(|v| v < current)
    {
        history.acknowledged = Some(installed.into());
    }
    db.set_setting(
        HISTORY,
        &serde_json::to_string(&history)
            .map_err(|_| "Could not save that you read these notes.")?,
    )
    .map_err(|_| "Could not save that you read these notes.")?;
    session.history = history;
    Ok(())
}
#[tauri::command]
pub fn acknowledge_update_notes(
    db: State<'_, Db>,
    state: State<'_, UpdateNotes>,
    version: String,
) -> Result<(), String> {
    acknowledge(&db, &state, &version)
}
#[tauri::command]
pub fn update_notes_can_present(window: WebviewWindow) -> bool {
    window.label() == "main"
        && window.is_visible().unwrap_or(false)
        && window.is_focused().unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    const CURRENT: &str = "0.7.103";
    fn catalog() -> Catalog {
        Catalog::parse(
            include_str!("../../../../tests/fixtures/update-notes/catalog.json"),
            CURRENT,
        )
        .unwrap()
    }
    fn seed(db: &Db, last: &str, ack: Option<&str>) {
        db.set_setting(
            HISTORY,
            &serde_json::to_string(&History {
                last_run: last.into(),
                acknowledged: ack.map(str::to_string),
                summary_from: None,
            })
            .unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn stable_version_order_matches_shared_cases() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/update-notes/versions.json"
        ))
        .unwrap();
        let versions: Vec<_> = cases["ascending"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| version(v.as_str().unwrap()).unwrap())
            .collect();
        assert!(versions.windows(2).all(|v| v[0] < v[1]));
        for invalid in cases["invalid"].as_array().unwrap() {
            assert!(version(invalid.as_str().unwrap()).is_err());
        }
    }
    #[test]
    fn skipped_releases_match_the_frontend_fixture_without_acknowledging() {
        let db = Db::open_in_memory().unwrap();
        seed(&db, "0.7.1", Some("0.7.1"));
        let result = session(&db, CURRENT, true, true, "linux", &catalog()).unwrap();
        let expected: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/update-notes/status.json"
        ))
        .unwrap();
        assert_eq!(serde_json::to_value(&result.status).unwrap(), expected);
        let stored: History =
            serde_json::from_str(&db.get_setting(HISTORY).unwrap().unwrap()).unwrap();
        assert_eq!(stored.acknowledged.as_deref(), Some("0.7.1"));
        assert!(
            session(&db, CURRENT, true, true, "linux", &catalog())
                .unwrap()
                .status
                .auto_show
        );
        let state = UpdateNotes(Mutex::new(Ok(result)));
        acknowledge(&db, &state, CURRENT).unwrap();
        let next = session(&db, CURRENT, true, true, "linux", &catalog()).unwrap();
        assert!(!next.status.auto_show);
        assert_eq!(next.status.releases.len(), 3);
    }
    #[test]
    fn new_profiles_unknown_history_and_development_are_distinct() {
        let db = Db::open_in_memory().unwrap();
        let first = session(&db, CURRENT, false, true, "linux", &catalog()).unwrap();
        assert!(!first.status.auto_show);
        assert!(!first.status.earlier_version_unknown);
        let existing = Db::open_in_memory().unwrap();
        let unknown = session(&existing, CURRENT, true, true, "linux", &catalog()).unwrap();
        assert!(unknown.status.auto_show && unknown.status.earlier_version_unknown);
        assert_eq!(unknown.status.releases.len(), 1);
        assert!(unknown.status.previous_version.is_none());
        let development = Db::open_in_memory().unwrap();
        assert!(
            !session(&development, CURRENT, true, false, "linux", &catalog())
                .unwrap()
                .status
                .auto_show
        );
        assert!(development.get_setting(HISTORY).unwrap().is_none());
        existing.set_setting(HISTORY, "corrupt").unwrap();
        assert!(
            session(&existing, CURRENT, true, true, "linux", &catalog())
                .unwrap()
                .status
                .earlier_version_unknown
        );
    }
    #[test]
    fn downgrade_and_stale_acknowledgement_do_not_erase_the_high_water_mark() {
        let db = Db::open_in_memory().unwrap();
        seed(&db, "0.8.0", Some("0.8.0"));
        let result = session(&db, CURRENT, true, true, "linux", &catalog()).unwrap();
        assert!(!result.status.auto_show);
        let state = UpdateNotes(Mutex::new(Ok(result)));
        assert!(acknowledge(&db, &state, "0.7.102").is_err());
        acknowledge(&db, &state, CURRENT).unwrap();
        let stored: History =
            serde_json::from_str(&db.get_setting(HISTORY).unwrap().unwrap()).unwrap();
        assert_eq!(stored.acknowledged.as_deref(), Some("0.8.0"));
        assert!(
            !session(&db, CURRENT, true, true, "linux", &catalog())
                .unwrap()
                .status
                .auto_show
        );
    }
    #[test]
    fn platform_filtering_and_partial_coverage_keep_the_release_range_honest() {
        let (releases, changes, partial) = catalog().summary(Some("0.3.8"), "windows").unwrap();
        assert!(partial);
        assert_eq!(releases.len(), 3);
        assert!(changes.iter().all(|c| c.id != "appimage-links"));
        assert!(releases
            .iter()
            .find(|r| r.version == "0.7.101")
            .unwrap()
            .changes
            .is_empty());
        let (_, single, partial) = catalog().summary(Some("0.7.102"), "linux").unwrap();
        assert!(!partial);
        assert_eq!(single.len(), 2);
    }
    #[test]
    fn duplicate_ids_and_explicit_supersession_preserve_actions_and_release_details() {
        let mut catalog = catalog();
        let original = catalog.releases[1].changes[0].clone();
        catalog.releases[2].changes.push(original.clone());
        let mut replacement = original.clone();
        replacement.id = "replacement".into();
        replacement.supersedes = vec![original.id.clone()];
        catalog.releases[2].changes.push(replacement);
        let (history, changes, _) = catalog.summary(Some("0.7.1"), "linux").unwrap();
        assert!(!changes.iter().any(|c| c.id == original.id));
        assert!(history[1].changes.iter().any(|c| c.id == original.id));
        catalog.releases[1].changes[0].kind = Kind::Action;
        catalog.releases[2].changes.retain(|c| c.id != original.id);
        let (_, changes, _) = catalog.summary(Some("0.7.1"), "linux").unwrap();
        assert!(changes
            .iter()
            .any(|c| c.id == original.id && c.kind == Kind::Action));
    }
    #[test]
    fn failed_acknowledgement_suppresses_the_session_but_retries_next_launch() {
        let path = std::env::temp_dir().join(format!(
            "update-notes-{}-{}.db",
            std::process::id(),
            wfm_core::identity::random_token(8)
        ));
        let db = Db::open(&path).unwrap();
        seed(&db, "0.7.1", Some("0.7.1"));
        let state = UpdateNotes(Mutex::new(Ok(session(
            &db,
            CURRENT,
            true,
            true,
            "linux",
            &catalog(),
        )
        .unwrap())));
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TRIGGER fail_notes BEFORE INSERT ON setting BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
        assert!(acknowledge(&db, &state, CURRENT).is_err());
        assert!(!state.0.lock().unwrap().as_ref().unwrap().status.auto_show);
        assert!(
            session(&db, CURRENT, true, true, "linux", &catalog())
                .unwrap()
                .status
                .auto_show
        );
        conn.execute_batch("DROP TRIGGER fail_notes;").unwrap();
        acknowledge(&db, &state, CURRENT).unwrap();
        drop(conn);
        drop(db);
        let db = Db::open(&path).unwrap();
        assert!(
            !session(&db, CURRENT, true, true, "linux", &catalog())
                .unwrap()
                .status
                .auto_show
        );
        drop(db);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn the_bundle_matches_the_installed_version_and_rejects_missing_or_future_notes() {
        Catalog::parse(BUNDLE, env!("CARGO_PKG_VERSION")).unwrap();
        assert!(Catalog::parse(BUNDLE, "0.7.102").is_err());
        let mut malformed: serde_json::Value = serde_json::from_str(BUNDLE).unwrap();
        malformed["releases"].as_array_mut().unwrap().pop();
        assert!(Catalog::parse(&malformed.to_string(), CURRENT).is_err());
    }
}
