//! Automatic scanning: while the game is running, rescan it on a user-chosen
//! cadence instead of waiting for a click.
//!
//! Detection and the memory read stay in wfm-core (`find_wf_pid` /
//! `scan_session`), so this module adds a schedule, not a second scan path.
//! The timing decision is pure ([`decide`]) and tested; the loop around it is
//! the same thin shell as `services/watch.rs`.
//!
//! Two rules are deliberately NOT user options:
//!
//! * A scan records a new snapshot, and both
//!   `services::protection::validate_snapshot` and `Db::session_allowance`
//!   require the snapshot a listing submits to be the LATEST one. An automatic
//!   scan landing while a listing review or Trade Session batch is being
//!   prepared would fail that submit ("Inventory changed... prepare a new
//!   batch"), so the webview holds scanning while an interactive listing flow
//!   is open.
//! * A failure is expected - the game sits at the login screen for minutes
//!   after launch, and Linux may not have granted ptrace - so a failure never
//!   notifies. It is logged once per distinct message, kept in the status for
//!   Settings, and retried on the schedule.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::persistence::Db;

/// Where the settings ride in the `setting` key/value table.
pub const SETTINGS_KEY: &str = "auto-scan-v1";

/// How often the loop wakes to re-evaluate. Cheap: one process-list refresh,
/// and a scan only when [`decide`] says so.
pub const TICK: Duration = Duration::from_secs(30);

/// How long a freshly detected game run is left alone before the first scan.
/// Warframe spends its first minute in a launcher and loading screens, where a
/// scan walks the whole address space and can only come back empty.
pub const FIRST_SCAN_DELAY: Duration = Duration::from_secs(60);

/// Retry spacing inside the bounded prompt window.
pub const PROMPT_RETRY: Duration = Duration::from_secs(90);

/// How many prompt attempts a run gets before it falls back to the user's
/// cadence. Six attempts is about nine minutes of the game being up, which
/// covers "still logging in" without turning a game parked at the login screen
/// into an endless full address-space walk.
pub const PROMPT_ATTEMPTS: u32 = 6;

/// The cadences the setting offers. A memory scan is a multi-gigabyte walk plus
/// one DE request, so the choices stay coarse on purpose.
pub const CADENCE_CHOICES: [u32; 3] = [15, 30, 60];

/// Cadence for a settings blob that names none.
pub const DEFAULT_CADENCE_MINUTES: u32 = 30;

/// The user's automatic-scan preferences, one JSON blob in the settings table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct AutoScanSettings {
    pub enabled: bool,
    /// Minutes between attempts while the game is running.
    pub cadence_minutes: u32,
    /// Whether a background scan that finishes while the app is open replaces
    /// the inventory on screen. Off means the app offers it instead.
    pub adopt_automatically: bool,
}

impl Default for AutoScanSettings {
    /// Off. An automatic feature that reads the game's memory must be opted
    /// into, and this is also what a corrupt or absent stored value falls back
    /// to.
    fn default() -> Self {
        Self {
            enabled: false,
            cadence_minutes: DEFAULT_CADENCE_MINUTES,
            adopt_automatically: true,
        }
    }
}

impl AutoScanSettings {
    /// Snap the cadence onto an offered choice.
    ///
    /// Applied on read as well as on write: the raw settings table is reachable
    /// through `set_setting`, so a value this module never validated can still
    /// arrive from storage.
    pub fn sanitized(self) -> Self {
        let cadence_minutes = CADENCE_CHOICES
            .iter()
            .copied()
            .min_by_key(|choice| choice.abs_diff(self.cadence_minutes))
            .unwrap_or(DEFAULT_CADENCE_MINUTES);
        Self {
            cadence_minutes,
            ..self
        }
    }
}

/// What the Settings panel shows about the loop. No inventory bytes and no
/// credentials: `last_error` is the same redacted message a manual scan
/// surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct AutoScanStatus {
    pub enabled: bool,
    pub cadence_minutes: u32,
    pub adopt_automatically: bool,
    /// An interactive listing flow is open, so scanning is suspended.
    pub held: bool,
    pub game_running: bool,
    /// Unix seconds of the last successful automatic scan.
    pub last_scan_at: Option<i64>,
    /// The last failure, redacted; cleared by a success or a settings change.
    pub last_error: Option<String>,
    /// Unix seconds of the next scheduled attempt, when one is scheduled.
    pub next_check_at: Option<i64>,
}

impl AutoScanStatus {
    fn from_settings(settings: AutoScanSettings) -> Self {
        Self {
            enabled: settings.enabled,
            cadence_minutes: settings.cadence_minutes,
            adopt_automatically: settings.adopt_automatically,
            held: false,
            game_running: false,
            last_scan_at: None,
            last_error: None,
            next_check_at: None,
        }
    }

    fn mirror_settings(&mut self, settings: AutoScanSettings) {
        self.enabled = settings.enabled;
        self.cadence_minutes = settings.cadence_minutes;
        self.adopt_automatically = settings.adopt_automatically;
    }
}

/// Process-wide loop state, managed by the shell.
pub struct AutoScanState {
    settings: Mutex<AutoScanSettings>,
    status: Mutex<AutoScanStatus>,
    held: AtomicBool,
}

impl AutoScanState {
    pub fn new(settings: AutoScanSettings) -> Self {
        let settings = settings.sanitized();
        Self {
            status: Mutex::new(AutoScanStatus::from_settings(settings)),
            settings: Mutex::new(settings),
            held: AtomicBool::new(false),
        }
    }

    pub fn from_db(db: &Db) -> Self {
        Self::new(load_settings(db))
    }

    pub fn settings(&self) -> AutoScanSettings {
        *wfm_core::poison::guard(&self.settings)
    }

    pub fn set_settings(&self, settings: AutoScanSettings) {
        let settings = settings.sanitized();
        *wfm_core::poison::guard(&self.settings) = settings;
        let mut status = wfm_core::poison::guard(&self.status);
        status.mirror_settings(settings);
        // The old failure belonged to the old configuration.
        status.last_error = None;
    }

    pub fn status(&self) -> AutoScanStatus {
        wfm_core::poison::guard(&self.status).clone()
    }

    pub fn update_status(&self, update: impl FnOnce(&mut AutoScanStatus)) {
        update(&mut wfm_core::poison::guard(&self.status));
    }

    pub fn held(&self) -> bool {
        self.held.load(Ordering::SeqCst)
    }

    pub fn set_held(&self, held: bool) {
        self.held.store(held, Ordering::SeqCst);
        self.update_status(|status| status.held = held);
    }
}

/// Progress through one detected game run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    pub pid: u32,
    pub detected_at: Instant,
    pub attempts: u32,
    pub succeeded: bool,
}

/// Everything one tick's decision depends on.
#[derive(Debug, Clone, Copy)]
pub struct Tick {
    /// The user's toggle.
    pub enabled: bool,
    /// An interactive listing flow is open.
    pub held: bool,
    /// Configured interval between attempts.
    pub cadence: Duration,
    /// The running game's pid, `None` when the game is not running.
    pub pid: Option<u32>,
    /// Bookkeeping for the run the loop is tracking.
    pub run: Option<Run>,
    /// When the last attempt began. Shared across runs on purpose: a game that
    /// restarts immediately after a scan must not buy an instant second walk.
    pub last_attempt_at: Option<Instant>,
    pub now: Instant,
}

/// What this tick should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Switching is off.
    Off,
    /// An interactive listing flow is open.
    Held,
    /// The game is not running.
    NoGame,
    /// Nothing to do before this instant.
    Wait(Instant),
    /// Run a scan now.
    Scan,
}

/// Decide one tick. Pure: no clock, no process lookup, no I/O.
pub fn decide(tick: &Tick) -> Decision {
    if !tick.enabled {
        return Decision::Off;
    }
    if tick.held {
        return Decision::Held;
    }
    let Some(pid) = tick.pid else {
        return Decision::NoGame;
    };
    // A run the loop is not tracking yet - a first sighting, or a pid that
    // changed. It gets the settling delay before anything reads its memory.
    let Some(run) = tick.run.filter(|tracked| tracked.pid == pid) else {
        return wait_or_scan(tick.now + FIRST_SCAN_DELAY, tick.now);
    };
    // Bounded prompt window. Right after a launch the game is still logging in,
    // so the first attempts usually walk the whole address space and find no
    // credentials. Retrying on a short interval is what makes the first useful
    // scan arrive minutes after launch rather than one cadence later; the
    // attempt cap is what stops a game left sitting at the login screen from
    // being walked forever.
    if !run.succeeded && run.attempts < PROMPT_ATTEMPTS {
        let due = match tick.last_attempt_at {
            Some(at) => at + PROMPT_RETRY,
            None => run.detected_at + FIRST_SCAN_DELAY,
        };
        return wait_or_scan(due, tick.now);
    }
    match tick.last_attempt_at {
        Some(at) => wait_or_scan(at + tick.cadence, tick.now),
        None => Decision::Scan,
    }
}

fn wait_or_scan(due: Instant, now: Instant) -> Decision {
    if now >= due {
        Decision::Scan
    } else {
        Decision::Wait(due)
    }
}

/// Read the stored settings, falling back to the default (off) for an absent,
/// corrupt, or hand-written value.
pub fn load_settings(db: &Db) -> AutoScanSettings {
    db.get_setting(SETTINGS_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str::<AutoScanSettings>(&raw).ok())
        .unwrap_or_default()
        .sanitized()
}

/// Persist the settings and return what was actually stored.
pub fn save_settings(db: &Db, settings: AutoScanSettings) -> Result<AutoScanSettings, String> {
    let settings = settings.sanitized();
    let raw = serde_json::to_string(&settings).map_err(|e| e.to_string())?;
    db.set_setting(SETTINGS_KEY, &raw).map_err(|e| e.to_string())?;
    Ok(settings)
}

/// Start the background loop. Detached thread: sleep, evaluate, maybe scan.
/// Nothing here can take the app down - every failure is logged and the loop
/// simply re-evaluates on the next tick.
pub fn start(app: AppHandle) {
    std::thread::Builder::new()
        .name("auto-scan".into())
        .spawn(move || run_loop(app))
        .map(|_| ())
        .unwrap_or_else(|e| eprintln!("tennoworth: automatic scan thread failed to start: {e}"));
}

/// Assemble one tick's inputs. A function rather than a closure so the loop can
/// keep mutating its bookkeeping after an attempt.
fn tick_at(
    settings: AutoScanSettings,
    held: bool,
    pid: Option<u32>,
    run: Option<Run>,
    last_attempt_at: Option<Instant>,
    now: Instant,
) -> Tick {
    Tick {
        enabled: settings.enabled,
        held,
        cadence: Duration::from_secs(u64::from(settings.cadence_minutes) * 60),
        pid,
        run,
        last_attempt_at,
        now,
    }
}

fn run_loop(app: AppHandle) {
    let mut run: Option<Run> = None;
    let mut last_attempt_at: Option<Instant> = None;
    // The last failure already written to stderr. A scan that keeps failing the
    // same way - no ptrace, or a game parked at the login screen - must not fill
    // the log at one line per tick.
    let mut logged_error: Option<String> = None;

    loop {
        std::thread::sleep(TICK);
        let Some(state) = app.try_state::<AutoScanState>() else {
            return;
        };
        let settings = state.settings();
        let pid = wfm_core::acquisition::scan::find_wf_pid();
        let now = Instant::now();
        if let Some(pid) = pid {
            if run.is_none_or(|tracked| tracked.pid != pid) {
                run = Some(Run {
                    pid,
                    detected_at: now,
                    attempts: 0,
                    succeeded: false,
                });
            }
        }

        let decision = decide(&tick_at(settings, state.held(), pid, run, last_attempt_at, now));
        let mut due = match decision {
            Decision::Wait(at) => Some(at),
            _ => None,
        };
        if decision == Decision::Scan {
            if crate::commands::inventory::scan_in_progress() {
                // A scan the user asked for owns the scanner. Skip without
                // spending an attempt, so a race does not consume the cadence.
                due = Some(now + TICK);
            } else {
                if let Some(tracked) = run.as_mut() {
                    tracked.attempts += 1;
                }
                last_attempt_at = Some(now);
                match crate::commands::inventory::scan_and_record(&app) {
                    Ok(payload) => {
                        if let Some(tracked) = run.as_mut() {
                            tracked.succeeded = true;
                        }
                        logged_error = None;
                        eprintln!(
                            "tennoworth: automatic scan recorded snapshot {:?}",
                            payload.snapshot_id
                        );
                        state.update_status(|status| {
                            status.last_scan_at = Some(crate::services::allowance::unix_now());
                            status.last_error = None;
                        });
                        crate::commands::inventory::publish_scan(&app, &payload);
                    }
                    Err(error) => {
                        if logged_error.as_deref() != Some(error.as_str()) {
                            eprintln!("tennoworth: automatic scan failed: {error}");
                            logged_error = Some(error.clone());
                        }
                        state.update_status(|status| status.last_error = Some(error));
                    }
                }
                due = match decide(&tick_at(
                    settings,
                    state.held(),
                    pid,
                    run,
                    last_attempt_at,
                    Instant::now(),
                )) {
                    Decision::Wait(at) => Some(at),
                    _ => None,
                };
            }
        }

        let after = Instant::now();
        state.update_status(|status| {
            status.mirror_settings(settings);
            status.game_running = pid.is_some();
            status.next_check_at = due.map(|at| {
                crate::services::allowance::unix_now()
                    + at.saturating_duration_since(after).as_secs() as i64
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: Duration = Duration::from_secs(60 * 60);

    /// `Instant - Duration`. The workspace denies unchecked time subtraction
    /// everywhere, tests included.
    fn ago(base: Instant, offset: Duration) -> Instant {
        base.checked_sub(offset)
            .expect("the test base instant is far enough from zero")
    }

    fn run(pid: u32, detected_at: Instant, attempts: u32, succeeded: bool) -> Run {
        Run {
            pid,
            detected_at,
            attempts,
            succeeded,
        }
    }

    fn tick(now: Instant, pid: Option<u32>, run: Option<Run>, last: Option<Instant>) -> Tick {
        Tick {
            enabled: true,
            held: false,
            cadence: HOUR,
            pid,
            run,
            last_attempt_at: last,
            now,
        }
    }

    #[test]
    fn a_closed_game_is_not_scanned() {
        let now = Instant::now();
        assert_eq!(
            decide(&tick(now, None, None, Some(ago(now, HOUR)))),
            Decision::NoGame
        );
    }

    #[test]
    fn disabled_never_scans() {
        let now = Instant::now();
        let running = run(7, ago(now, HOUR), 0, false);
        let mut t = tick(now, Some(7), Some(running), Some(ago(now, HOUR)));
        t.enabled = false;
        assert_eq!(decide(&t), Decision::Off);
    }

    #[test]
    fn a_listing_flow_holds_scanning_even_when_due() {
        let now = Instant::now();
        let running = run(7, ago(now, HOUR), 0, true);
        let mut t = tick(now, Some(7), Some(running), Some(ago(now, HOUR)));
        t.held = true;
        assert_eq!(decide(&t), Decision::Held);
    }

    #[test]
    fn the_first_scan_waits_for_the_game_to_settle() {
        let now = Instant::now();
        let running = run(7, now, 0, false);
        assert_eq!(
            decide(&tick(now, Some(7), Some(running), None)),
            Decision::Wait(now + FIRST_SCAN_DELAY)
        );
        let later = now + FIRST_SCAN_DELAY;
        assert_eq!(decide(&tick(later, Some(7), Some(running), None)), Decision::Scan);
    }

    #[test]
    fn a_successful_run_waits_the_users_cadence() {
        let now = Instant::now();
        let last = ago(now, HOUR) + Duration::from_secs(1);
        let running = run(7, ago(now, HOUR), 1, true);
        assert_eq!(
            decide(&tick(now, Some(7), Some(running), Some(last))),
            Decision::Wait(last + HOUR)
        );
        let due = last + HOUR;
        assert_eq!(decide(&tick(due, Some(7), Some(running), Some(last))), Decision::Scan);
    }

    #[test]
    fn a_failed_first_scan_retries_quickly_then_falls_back_to_the_cadence() {
        let now = Instant::now();
        let detected = ago(now, Duration::from_secs(300));
        let last = ago(now, PROMPT_RETRY) + Duration::from_secs(1);
        let failing = run(7, detected, 1, false);
        assert_eq!(
            decide(&tick(now, Some(7), Some(failing), Some(last))),
            Decision::Wait(last + PROMPT_RETRY)
        );

        // Once the prompt attempts are spent, the retry spacing is the cadence.
        let spent = run(7, detected, PROMPT_ATTEMPTS, false);
        assert_eq!(
            decide(&tick(now, Some(7), Some(spent), Some(last))),
            Decision::Wait(last + HOUR)
        );
    }

    #[test]
    fn a_restarted_game_gets_its_own_prompt_window() {
        let now = Instant::now();
        // No tracked run yet - the loop has just seen the new pid.
        assert_eq!(
            decide(&tick(now, Some(9), None, Some(ago(now, Duration::from_secs(1))))),
            Decision::Wait(now + FIRST_SCAN_DELAY)
        );
        // A run that changed pid is not the tracked run: same answer.
        let stale = run(7, ago(now, HOUR), PROMPT_ATTEMPTS, true);
        assert_eq!(
            decide(&tick(now, Some(9), Some(stale), Some(ago(now, Duration::from_secs(1))))),
            Decision::Wait(now + FIRST_SCAN_DELAY)
        );
    }

    #[test]
    fn cadence_snaps_to_the_nearest_offered_choice() {
        let with = |cadence_minutes: u32| AutoScanSettings {
            cadence_minutes,
            ..AutoScanSettings::default()
        };
        assert_eq!(with(0).sanitized().cadence_minutes, 15);
        assert_eq!(with(16).sanitized().cadence_minutes, 15);
        assert_eq!(with(23).sanitized().cadence_minutes, 30);
        assert_eq!(with(45).sanitized().cadence_minutes, 30);
        assert_eq!(with(200).sanitized().cadence_minutes, 60);
        for choice in CADENCE_CHOICES {
            assert_eq!(with(choice).sanitized().cadence_minutes, choice);
        }
    }

    #[test]
    fn defaults_are_off_with_adoption_on() {
        let default = AutoScanSettings::default();
        assert!(!default.enabled, "automatic scanning is opt-in");
        assert_eq!(default.cadence_minutes, DEFAULT_CADENCE_MINUTES);
        assert!(default.adopt_automatically);
    }

    #[test]
    fn a_corrupt_stored_setting_leaves_scanning_off() {
        let db = Db::open_in_memory().unwrap();
        for raw in [
            "not json",
            "{}",
            r#"{"enabled":"yes","cadenceMinutes":30,"adoptAutomatically":true}"#,
        ] {
            db.set_setting(SETTINGS_KEY, raw).unwrap();
            let settings = load_settings(&db);
            assert!(!settings.enabled, "{raw} must not enable scanning");
            assert!(CADENCE_CHOICES.contains(&settings.cadence_minutes));
        }
        // A hand-written raw value still gets its cadence snapped on read.
        db.set_setting(
            SETTINGS_KEY,
            r#"{"enabled":true,"cadenceMinutes":1000,"adoptAutomatically":false}"#,
        )
        .unwrap();
        let settings = load_settings(&db);
        assert!(settings.enabled);
        assert_eq!(settings.cadence_minutes, 60);
        assert!(!settings.adopt_automatically);
    }

    #[test]
    fn settings_round_trip_through_the_store() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(load_settings(&db), AutoScanSettings::default());
        let saved = save_settings(
            &db,
            AutoScanSettings {
                enabled: true,
                cadence_minutes: 45,
                adopt_automatically: false,
            },
        )
        .unwrap();
        assert_eq!(
            saved.cadence_minutes, 30,
            "an off-choice cadence is snapped before it is stored"
        );
        assert_eq!(load_settings(&db), saved);
    }
}
