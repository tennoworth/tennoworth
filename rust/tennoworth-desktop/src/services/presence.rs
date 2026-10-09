//! Trade presence: the user's warframe.market status - Online, Online in game
//! or Invisible - set from the app, and optionally following the game.
//!
//! It offers what the website offers and nothing more: the three statuses and
//! how long to keep one. Following the game is the app's addition: Online in
//! game from login to quit, then the status chosen for when Warframe closes.
//!
//! Two facts from the live socket shape the rules below (see
//! `wfm_core::trading::presence`):
//!
//! * The server is the source of truth, and any of the account's clients can
//!   change the status. A change the app did not make wins: it stops what the
//!   app was keeping up, and pauses following until the next game session.
//! * Closing the socket does not clear a status. Whatever the app keeps up on
//!   its own - following the game, or a status kept "while running" - goes out
//!   with a short duration it renews, so a crash leaves the account available
//!   for minutes rather than hours, and leaving says Invisible explicitly.
//!
//! The decisions are pure ([`Brain`]) and tested; the loop around them is the
//! same thin shell as the order stream's.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use wfm_core::trading::presence::{Presence, PresenceEvent, PresenceSocket, RichStatus, SignInError};

use crate::command_error::CmdError;
use crate::persistence::Db;
use crate::services::wfm_session::WfmSession;

/// Where the settings ride in the `setting` key/value table.
pub const SETTINGS_KEY: &str = "presence-v1";

/// Emitted with a [`PresenceStatus`] whenever what the user sees changes.
pub const EVENT_PRESENCE_CHANGED: &str = "presence-changed";

/// The "keep status for" choices, in minutes, as warframe.market offers them.
/// `None` in the settings is its "while connected" choice, which the app reads
/// as "while TennoWorth runs".
pub const KEEP_FOR_CHOICES: [u32; 4] = [30, 60, 120, 240];

/// What the app keeps up goes out with this duration and is renewed every
/// [`RENEW_EVERY`], so it lapses on its own if the app dies.
pub const MANAGED_DURATION_SECS: u32 = 600;
pub const RENEW_EVERY: Duration = Duration::from_secs(300);

/// Socket read granularity: how quickly a command or a game change is acted on.
const TICK: Duration = Duration::from_millis(500);
/// How often the process list is checked for a game that ended without saying so.
const GAME_CHECK: Duration = Duration::from_secs(15);
/// How long a command waits for the server's answer.
const REPLY_TIMEOUT: Duration = Duration::from_secs(10);
/// Reconnect backoff bounds after a dropped or refused connection.
const BACKOFF_MIN: Duration = Duration::from_secs(15);
const BACKOFF_MAX: Duration = Duration::from_secs(15 * 60);
/// How long leaving waits to say Invisible before the app exits anyway.
const QUIT_TIMEOUT: Duration = Duration::from_secs(3);

/// EE.log lines that mark a game session. The process list catches a session
/// that ends without the shutdown line, a crash for instance.
const LOGGED_IN: &str = "Sys [Info]: Logged in ";
const SHUTDOWN: [&str; 2] = ["Main Shutdown Initiated", "Exiting main loop"];

/// One of warframe.market's three statuses, as the webview names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum PresenceChoice {
    Online,
    Ingame,
    Invisible,
}

impl From<PresenceChoice> for Presence {
    fn from(choice: PresenceChoice) -> Self {
        match choice {
            PresenceChoice::Online => Presence::Online,
            PresenceChoice::Ingame => Presence::Ingame,
            PresenceChoice::Invisible => Presence::Invisible,
        }
    }
}

impl From<Presence> for PresenceChoice {
    fn from(presence: Presence) -> Self {
        match presence {
            Presence::Online => PresenceChoice::Online,
            Presence::Ingame => PresenceChoice::Ingame,
            Presence::Invisible => PresenceChoice::Invisible,
        }
    }
}

impl PresenceChoice {
    pub fn label(self) -> &'static str {
        match self {
            PresenceChoice::Online => "Online",
            PresenceChoice::Ingame => "Online in game",
            PresenceChoice::Invisible => "Invisible",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct PresenceSettings {
    /// Online in game from login to quit.
    pub follow_game: bool,
    /// The status following sets when Warframe closes: Online or Invisible.
    pub when_closed: PresenceChoice,
    /// How long a status picked by hand is kept, in minutes; `null` keeps it
    /// while TennoWorth runs.
    pub keep_for_minutes: Option<u32>,
}

impl Default for PresenceSettings {
    /// Following is off: it changes what other traders see, so it is opted into.
    fn default() -> Self {
        Self {
            follow_game: false,
            when_closed: PresenceChoice::Invisible,
            keep_for_minutes: None,
        }
    }
}

impl PresenceSettings {
    /// Snap onto the offered choices. Applied on read as well as on write: the
    /// generic settings commands can reach the raw row.
    pub fn sanitized(self) -> Self {
        let when_closed = match self.when_closed {
            PresenceChoice::Ingame => PresenceChoice::Invisible,
            other => other,
        };
        let keep_for_minutes = self.keep_for_minutes.map(|minutes| {
            KEEP_FOR_CHOICES
                .iter()
                .copied()
                .min_by_key(|choice| choice.abs_diff(minutes))
                .unwrap_or(60)
        });
        Self { follow_game: self.follow_game, when_closed, keep_for_minutes }
    }
}

pub fn load_settings(db: &Db) -> PresenceSettings {
    db.get_setting(SETTINGS_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str::<PresenceSettings>(&raw).ok())
        .unwrap_or_default()
        .sanitized()
}

pub fn save_settings(db: &Db, settings: PresenceSettings) -> Result<PresenceSettings, String> {
    let settings = settings.sanitized();
    let raw = serde_json::to_string(&settings).map_err(|e| e.to_string())?;
    db.set_setting(SETTINGS_KEY, &raw).map_err(|e| e.to_string())?;
    Ok(settings)
}

/// Why the status cannot be shown or changed right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum PresenceProblem {
    /// warframe.market requires a verified account to set a status.
    NotVerified,
    /// warframe.market refused the sign-in or a change; `detail` is its code.
    Refused,
    /// WFM access is paused by the access policy or a throttle.
    Paused,
    /// The socket could not be reached; it is retried.
    Unreachable,
}

/// What the strip, Settings and the tray show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct PresenceStatus {
    /// A warframe.market session is unlocked, so a status can be read and set.
    pub signed_in: bool,
    /// The status channel is connected and signed in.
    pub connected: bool,
    /// The status warframe.market last reported; `null` until it has.
    pub status: Option<PresenceChoice>,
    /// When a timed status ends (RFC 3339), if it has a duration.
    pub status_until: Option<String>,
    /// When the status was set (RFC 3339).
    pub status_set_at: Option<String>,
    /// TennoWorth keeps this status up and renews it.
    pub managed: bool,
    /// Following the game is on and not paused.
    pub following: bool,
    /// Following is on but paused by a status picked by hand, here or on
    /// warframe.market, until the next game session.
    pub follow_paused: bool,
    pub game_running: bool,
    pub problem: Option<PresenceProblem>,
    /// The server's code behind `problem`, for troubleshooting.
    pub detail: Option<String>,
    pub settings: PresenceSettings,
}

impl PresenceStatus {
    fn new(settings: PresenceSettings) -> Self {
        Self {
            signed_in: false,
            connected: false,
            status: None,
            status_until: None,
            status_set_at: None,
            managed: false,
            following: false,
            follow_paused: false,
            game_running: false,
            problem: None,
            detail: None,
            settings,
        }
    }

    /// The tray's one-line summary.
    pub fn tray_label(&self) -> String {
        if !self.signed_in {
            return "Status: unlock TennoWorth to change".into();
        }
        if self.problem == Some(PresenceProblem::NotVerified) {
            return "Status: refused, account not verified".into();
        }
        match self.status {
            Some(status) if self.follow_paused => format!("Status: {} · set by hand", status.label()),
            Some(status) => format!("Status: {}", status.label()),
            None => "Status: connecting…".into(),
        }
    }
}

/// What the tray shows; a change outside it does not rebuild the menu.
fn tray_view(status: &PresenceStatus) -> (String, Option<PresenceChoice>, bool, bool, bool) {
    (status.tray_label(), status.status, status.following, status.settings.follow_game, status.connected)
}

/// A status change for the socket: `duration` in seconds, `None` for none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    pub status: Presence,
    pub duration: Option<u32>,
}

/// The pure decisions: what the app asserts, when it renews, and when it lets
/// go. Every input that can change them is a method; each returns the status
/// to send, if any.
#[derive(Debug, Clone)]
pub struct Brain {
    settings: PresenceSettings,
    game_running: bool,
    follow_paused: bool,
    /// The status the app keeps up and renews, if any.
    maintained: Option<Presence>,
    last_sent: Option<Instant>,
}

impl Brain {
    pub fn new(settings: PresenceSettings) -> Self {
        Self { settings, game_running: false, follow_paused: false, maintained: None, last_sent: None }
    }

    fn following(&self) -> bool {
        self.settings.follow_game && !self.follow_paused
    }

    fn maintain(&mut self, status: Presence, now: Instant) -> Change {
        self.maintained = Some(status);
        self.last_sent = Some(now);
        Change { status, duration: Some(MANAGED_DURATION_SECS) }
    }

    fn release(&mut self, status: Presence, duration: Option<u32>) -> Change {
        self.maintained = None;
        self.last_sent = None;
        Change { status, duration }
    }

    /// The game started or ended. A new session resumes following.
    pub fn game(&mut self, running: bool, now: Instant) -> Option<Change> {
        if running == self.game_running {
            return None;
        }
        self.game_running = running;
        if running {
            self.follow_paused = false;
            return self.following().then(|| self.maintain(Presence::Ingame, now));
        }
        if !self.following() {
            return None;
        }
        Some(match self.settings.when_closed {
            PresenceChoice::Online => self.maintain(Presence::Online, now),
            _ => self.release(Presence::Invisible, None),
        })
    }

    /// A status picked by hand in TennoWorth. It pauses following until the
    /// next game session, like a change made on the website.
    pub fn manual(&mut self, choice: PresenceChoice, now: Instant) -> Change {
        if self.settings.follow_game {
            self.follow_paused = true;
        }
        match (choice, self.settings.keep_for_minutes) {
            (PresenceChoice::Invisible, _) => self.release(Presence::Invisible, None),
            (choice, None) => self.maintain(choice.into(), now),
            (choice, Some(minutes)) => self.release(choice.into(), Some(minutes.saturating_mul(60))),
        }
    }

    /// "Follow the game now": resume without waiting for the next session.
    pub fn follow_now(&mut self, now: Instant) -> Option<Change> {
        self.follow_paused = false;
        (self.settings.follow_game && self.game_running).then(|| self.maintain(Presence::Ingame, now))
    }

    pub fn settings_changed(&mut self, settings: PresenceSettings, now: Instant) -> Option<Change> {
        let was_following = self.following();
        self.settings = settings;
        if !settings.follow_game {
            self.follow_paused = false;
        }
        (!was_following && self.following() && self.game_running).then(|| self.maintain(Presence::Ingame, now))
    }

    /// The server reports a change TennoWorth did not make. It wins: the app
    /// stops keeping its own status up, and following waits for the next session.
    pub fn external_change(&mut self) {
        self.maintained = None;
        self.last_sent = None;
        if self.settings.follow_game {
            self.follow_paused = true;
        }
    }

    /// The server refused what the app sent, so there is nothing to keep up.
    pub fn refused(&mut self) {
        self.maintained = None;
        self.last_sent = None;
    }

    /// The renewal, when one is due.
    pub fn due(&mut self, now: Instant) -> Option<Change> {
        let status = self.maintained?;
        let renew = self.last_sent.is_none_or(|at| now.saturating_duration_since(at) >= RENEW_EVERY);
        renew.then(|| self.maintain(status, now))
    }

    /// After a reconnect: assert again what the app keeps up, in case its
    /// duration lapsed while the socket was down.
    pub fn reconnected(&mut self, now: Instant) -> Option<Change> {
        let status = self.maintained?;
        Some(self.maintain(status, now))
    }

    /// Leaving (quit or sign-out): what the app kept up must not outlive it.
    pub fn leaving(&mut self) -> Option<Change> {
        self.maintained.is_some().then(|| self.release(Presence::Invisible, None))
    }

    pub fn managed(&self) -> bool {
        self.maintained.is_some()
    }
    pub fn is_following(&self) -> bool {
        self.following()
    }
    pub fn follow_paused(&self) -> bool {
        self.settings.follow_game && self.follow_paused
    }
    pub fn game_running(&self) -> bool {
        self.game_running
    }
}

/// Which game session is live, from EE.log lines and the process list. The
/// shutdown line arrives while the process is still exiting, so the pid it
/// ended is remembered and not counted as running again.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GameWatch {
    ended_pid: Option<u32>,
}

impl GameWatch {
    pub fn running(&mut self, pid: Option<u32>) -> bool {
        match pid {
            None => {
                self.ended_pid = None;
                false
            }
            Some(pid) => self.ended_pid != Some(pid),
        }
    }

    pub fn ended(&mut self, pid: Option<u32>) {
        self.ended_pid = pid;
    }

    pub fn logged_in(&mut self) {
        self.ended_pid = None;
    }
}

/// A game-session line, if this log line is one.
pub fn session_line(line: &str) -> Option<bool> {
    if line.contains(LOGGED_IN) {
        Some(true)
    } else if SHUTDOWN.iter().any(|marker| line.contains(marker)) {
        Some(false)
    } else {
        None
    }
}

/// Where an IPC call waits for warframe.market's answer.
type Reply = mpsc::Sender<Result<(), CmdError>>;

enum Command {
    Set(PresenceChoice, Reply),
    FollowNow,
    Settings(PresenceSettings),
    GameLine(bool),
    Quit(mpsc::Sender<()>),
}

/// Process-wide presence state, managed by the shell.
pub struct PresenceState {
    status: Mutex<PresenceStatus>,
    commands: Mutex<Option<Sender<Command>>>,
}

impl PresenceState {
    pub fn from_db(db: &Db) -> Self {
        let mut status = PresenceStatus::new(load_settings(db));
        status.following = status.settings.follow_game;
        Self { status: Mutex::new(status), commands: Mutex::new(None) }
    }

    pub fn status(&self) -> PresenceStatus {
        wfm_core::poison::guard(&self.status).clone()
    }

    fn send(&self, command: Command) -> bool {
        wfm_core::poison::guard(&self.commands)
            .as_ref()
            .is_some_and(|tx| tx.send(command).is_ok())
    }

    /// Pick a status. Waits for warframe.market's answer.
    pub fn set(&self, choice: PresenceChoice) -> Result<(), CmdError> {
        let status = self.status();
        if !status.signed_in {
            return Err(CmdError::needs_unlock());
        }
        if !status.connected {
            return Err(CmdError::of("presence_unavailable", "The status channel is not connected yet. Try again in a moment."));
        }
        let (tx, rx) = mpsc::channel();
        if !self.send(Command::Set(choice, tx)) {
            return Err(CmdError::of("presence_unavailable", "Status changes are not available in this session."));
        }
        rx.recv_timeout(REPLY_TIMEOUT + Duration::from_secs(1))
            .unwrap_or_else(|_| Err(CmdError::of("presence_timeout", "warframe.market did not answer in time. Your status may not have changed.")))
    }

    pub fn follow_now(&self) {
        self.send(Command::FollowNow);
    }

    /// The loop applies and publishes it, so the strip and the tray follow.
    pub fn settings_changed(&self, settings: PresenceSettings) {
        self.send(Command::Settings(settings));
    }

    /// Hand a game-session line to the loop. Called on the tailer's thread, so
    /// it only queues.
    pub fn game_line(&self, line: &str) {
        if let Some(started) = session_line(line) {
            self.send(Command::GameLine(started));
        }
    }

    /// Say Invisible for whatever the app kept up, then let the app exit.
    pub fn shutdown(&self) {
        let (tx, rx) = mpsc::channel();
        if self.send(Command::Quit(tx)) {
            let _ = rx.recv_timeout(QUIT_TIMEOUT);
        }
    }
}

/// Start the loop. Never takes the app down; every failure logs and backs off.
/// `on_change` runs after each change the tray shows: the shell passes its
/// Status refresh, which this layer cannot import.
pub fn start(app: AppHandle, on_change: fn(&AppHandle)) {
    let (tx, rx) = mpsc::channel();
    if let Some(state) = app.try_state::<PresenceState>() {
        *wfm_core::poison::guard(&state.commands) = Some(tx);
    }
    std::thread::Builder::new()
        .name("presence".into())
        .spawn(move || {
            wfm_client::governor::with_context(
                wfm_client::governor::Context { background: true, ..Default::default() },
                || run(app, rx, on_change),
            )
        })
        .map(|_| ())
        .unwrap_or_else(|e| eprintln!("tennoworth: presence thread failed to start: {e}"));
}

struct Loop {
    app: AppHandle,
    on_change: fn(&AppHandle),
    brain: Brain,
    game: GameWatch,
    last_game_check: Option<Instant>,
    /// Sent commands awaiting the server, with the reply channel for an IPC call.
    pending: Vec<(String, Instant, Option<Reply>)>,
    /// The newest revision the app itself caused.
    own_revision: u64,
    quitting: Option<mpsc::Sender<()>>,
}

impl Loop {
    fn publish(&self, update: impl FnOnce(&mut PresenceStatus)) {
        let Some(state) = self.app.try_state::<PresenceState>() else { return };
        let (changed, tray_changed) = {
            let mut status = wfm_core::poison::guard(&state.status);
            let before = status.clone();
            update(&mut status);
            status.managed = self.brain.managed();
            status.following = self.brain.is_following();
            status.follow_paused = self.brain.follow_paused();
            status.game_running = self.brain.game_running();
            ((*status != before).then(|| status.clone()), tray_view(&status) != tray_view(&before))
        };
        if let Some(status) = changed {
            let _ = self.app.emit(EVENT_PRESENCE_CHANGED, &status);
        }
        // A renewal moves `status_until` every few minutes; the tray shows
        // none of that, so it is not touched.
        if tray_changed {
            (self.on_change)(&self.app);
        }
    }

    fn check_game(&mut self, now: Instant) -> Option<Change> {
        if self.last_game_check.is_some_and(|at| now.saturating_duration_since(at) < GAME_CHECK) {
            return None;
        }
        self.last_game_check = Some(now);
        let running = self.game.running(wfm_core::acquisition::scan::find_wf_pid());
        self.brain.game(running, now)
    }

    fn game_line(&mut self, started: bool, now: Instant) -> Option<Change> {
        let pid = wfm_core::acquisition::scan::find_wf_pid();
        if started {
            self.game.logged_in();
        } else {
            self.game.ended(pid);
        }
        let running = self.game.running(pid);
        self.last_game_check = Some(now);
        self.brain.game(running, now)
    }

    /// Apply everything that does not need the socket. Returns sends to make.
    fn handle(&mut self, command: Command, now: Instant, sends: &mut Vec<(Change, Option<Reply>)>) {
        match command {
            Command::Set(choice, reply) => sends.push((self.brain.manual(choice, now), Some(reply))),
            Command::FollowNow => {
                if let Some(send) = self.brain.follow_now(now) {
                    sends.push((send, None));
                }
            }
            Command::Settings(settings) => {
                if let Some(send) = self.brain.settings_changed(settings, now) {
                    sends.push((send, None));
                }
                self.publish(|s| s.settings = settings);
            }
            Command::GameLine(started) => {
                if let Some(send) = self.game_line(started, now) {
                    sends.push((send, None));
                }
            }
            Command::Quit(done) => {
                if let Some(send) = self.brain.leaving() {
                    sends.push((send, None));
                }
                self.quitting = Some(done);
            }
        }
    }
}

fn detail_problem(reason: &str) -> PresenceProblem {
    if reason.contains("NotVerified") {
        PresenceProblem::NotVerified
    } else {
        PresenceProblem::Refused
    }
}

fn refusal(reason: &str) -> CmdError {
    match detail_problem(reason) {
        PresenceProblem::NotVerified => CmdError::of(
            "presence_not_verified",
            "warframe.market refused the change: this account is not verified. Verify it on warframe.market, then try again.",
        ),
        _ => CmdError::of("presence_refused", format!("warframe.market refused the change ({reason}).")),
    }
}

fn run(app: AppHandle, rx: Receiver<Command>, on_change: fn(&AppHandle)) {
    let settings = app.try_state::<PresenceState>().map(|s| s.status().settings).unwrap_or_default();
    let mut lp = Loop {
        app,
        on_change,
        brain: Brain::new(settings),
        game: GameWatch::default(),
        last_game_check: None,
        pending: Vec::new(),
        own_revision: 0,
        quitting: None,
    };
    let mut backoff = BACKOFF_MIN;
    let mut retry_at: Option<Instant> = None;
    let mut refused_generation: Option<u64> = None;

    loop {
        // Offline: keep following the game and answering commands, but there is
        // no socket to send through.
        let now = Instant::now();
        let mut sends = Vec::new();
        match rx.recv_timeout(TICK) {
            Ok(command) => lp.handle(command, now, &mut sends),
            Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
        let _ = lp.check_game(now);
        for (_, reply) in sends {
            if let Some(reply) = reply {
                let _ = reply.send(Err(CmdError::of("presence_unavailable", "The status channel is not connected.")));
            }
        }
        if let Some(done) = lp.quitting.take() {
            let _ = done.send(());
            return;
        }

        let session = lp.app.state::<Arc<WfmSession>>();
        let generation = session.session_generation();
        let unlocked = session.require_unlocked().ok();
        let paused = wfm_client::governor::process()
            .check(wfm_client::governor::Kind::WebSocket, &wfm_client::governor::context())
            .is_err();
        lp.publish(|s| {
            s.signed_in = unlocked.is_some();
            s.connected = false;
            if unlocked.is_none() {
                s.status = None;
                s.status_until = None;
                s.status_set_at = None;
                s.problem = None;
                s.detail = None;
            } else if paused {
                s.problem = Some(PresenceProblem::Paused);
            }
        });
        let Some(unlocked) = unlocked else { continue };
        if paused || refused_generation == Some(generation) || retry_at.is_some_and(|at| Instant::now() < at) {
            continue;
        }

        let connected_at = Instant::now();
        match PresenceSocket::connect(&unlocked.jwt, TICK) {
            Ok((socket, current)) => {
                drop(unlocked);
                backoff = BACKOFF_MIN;
                retry_at = None;
                lp.publish(|s| {
                    s.connected = true;
                    s.problem = None;
                    s.detail = None;
                    if let Some(current) = &current {
                        apply_rich(s, current);
                    }
                });
                if let Some(rev) = current.as_ref().and_then(|c| c.revision) {
                    lp.own_revision = lp.own_revision.max(rev);
                }
                let outcome = connected(&mut lp, socket, &rx, generation);
                lp.publish(|s| s.connected = false);
                match outcome {
                    Ended::Quit => return,
                    Ended::SignedOut => {}
                    Ended::Dropped(error) => {
                        if connected_at.elapsed() > Duration::from_secs(300) {
                            backoff = BACKOFF_MIN;
                        }
                        eprintln!("tennoworth: presence channel dropped: {error}; reconnecting in {backoff:?}");
                        lp.publish(|s| s.problem = Some(PresenceProblem::Unreachable));
                        retry_at = Some(Instant::now() + backoff + wfm_client::governor::jitter(5000));
                        backoff = (backoff * 2).min(BACKOFF_MAX);
                    }
                }
            }
            Err(SignInError::Refused(reason)) => {
                // The same credentials will be refused again; wait for a new session.
                eprintln!("tennoworth: presence sign-in refused: {reason}");
                refused_generation = Some(generation);
                lp.publish(|s| {
                    s.problem = Some(detail_problem(&reason));
                    s.detail = Some(reason);
                });
            }
            Err(SignInError::Unavailable(error)) => {
                eprintln!("tennoworth: presence channel unavailable: {error:#}; retrying in {backoff:?}");
                lp.publish(|s| s.problem = Some(PresenceProblem::Unreachable));
                retry_at = Some(Instant::now() + backoff + wfm_client::governor::jitter(5000));
                backoff = (backoff * 2).min(BACKOFF_MAX);
            }
        }
    }
}

fn apply_rich(status: &mut PresenceStatus, rich: &RichStatus) {
    status.status = Some(rich.status.into());
    status.status_until = rich.until.clone();
    status.status_set_at = rich.set_at.clone();
}

enum Ended {
    Quit,
    SignedOut,
    Dropped(anyhow::Error),
}

fn connected(lp: &mut Loop, mut socket: PresenceSocket, rx: &Receiver<Command>, generation: u64) -> Ended {
    let mut queued: Vec<(Change, Option<Reply>)> = Vec::new();
    let now = Instant::now();
    if let Some(send) = lp.brain.reconnected(now) {
        queued.push((send, None));
    }
    if let Some(send) = lp.check_game(now) {
        queued.push((send, None));
    }
    loop {
        let now = Instant::now();
        while let Ok(command) = rx.try_recv() {
            lp.handle(command, now, &mut queued);
        }
        if let Some(send) = lp.check_game(now) {
            queued.push((send, None));
        }
        if let Some(send) = lp.brain.due(now) {
            queued.push((send, None));
        }

        // Signed out, or a different session: let go of what the app kept up
        // while this socket is still signed in, then reconnect as the new one.
        let session = lp.app.state::<Arc<WfmSession>>();
        let signed_out = !session.is_unlocked() || session.session_generation() != generation;
        if signed_out {
            if let Some(send) = lp.brain.leaving() {
                queued.push((send, None));
            }
        }

        let ws_paused = wfm_client::governor::process()
            .check(wfm_client::governor::Kind::WebSocket, &wfm_client::governor::context())
            .is_err();
        for (send, reply) in queued.drain(..) {
            match socket.set_status(send.status, send.duration) {
                Ok(id) => lp.pending.push((id, now, reply)),
                Err(error) => {
                    lp.brain.refused();
                    if let Some(reply) = reply {
                        let _ = reply.send(Err(CmdError::of("presence_unavailable", "The status channel is not available right now.")));
                    }
                    if ws_paused {
                        lp.publish(|s| s.problem = Some(PresenceProblem::Paused));
                    }
                    eprintln!("tennoworth: presence send failed: {error:#}");
                }
            }
        }
        lp.publish(|_| {});

        if let Some(done) = lp.quitting.take() {
            // Give the final send a moment to reach the server before closing.
            let deadline = Instant::now() + Duration::from_secs(2);
            while !lp.pending.is_empty() && Instant::now() < deadline {
                if let Ok(Some(event)) = socket.poll() {
                    on_event(lp, event);
                } else {
                    break;
                }
            }
            socket.close();
            let _ = done.send(());
            return Ended::Quit;
        }
        if signed_out {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !lp.pending.is_empty() && Instant::now() < deadline {
                match socket.poll() {
                    Ok(Some(event)) => on_event(lp, event),
                    Ok(None) => {}
                    Err(_) => break,
                }
            }
            socket.close();
            return Ended::SignedOut;
        }
        if ws_paused {
            // The access policy closes WebSockets and suppresses reconnection.
            socket.close();
            lp.publish(|s| s.problem = Some(PresenceProblem::Paused));
            return Ended::SignedOut;
        }

        match socket.poll() {
            Ok(Some(event)) => on_event(lp, event),
            Ok(None) => {}
            Err(error) => {
                fail_pending(lp);
                return Ended::Dropped(error);
            }
        }
        expire_pending(lp, Instant::now());
    }
}

fn take_pending(lp: &mut Loop, id: &str) -> Option<Option<Reply>> {
    let index = lp.pending.iter().position(|(pending, _, _)| pending == id)?;
    Some(lp.pending.remove(index).2)
}

fn on_event(lp: &mut Loop, event: PresenceEvent) {
    match event {
        PresenceEvent::StatusSet { id, status } => {
            if let Some(rev) = status.revision {
                lp.own_revision = lp.own_revision.max(rev);
            }
            if let Some(Some(reply)) = take_pending(lp, &id) {
                let _ = reply.send(Ok(()));
            }
            lp.publish(|s| {
                apply_rich(s, &status);
                s.problem = None;
                s.detail = None;
            });
        }
        PresenceEvent::StatusRefused { id, reason } => refused(lp, Some(id), reason),
        PresenceEvent::Protected { id, reason } => refused(lp, id, reason),
        PresenceEvent::StatusChanged(status) => {
            let newer = status.revision.is_none_or(|rev| rev > lp.own_revision);
            if !newer {
                return;
            }
            if let Some(rev) = status.revision {
                lp.own_revision = rev;
            }
            lp.brain.external_change();
            lp.publish(|s| apply_rich(s, &status));
        }
        PresenceEvent::SignedIn { .. } | PresenceEvent::SignInRefused { .. } | PresenceEvent::Other => {}
    }
}

fn refused(lp: &mut Loop, id: Option<String>, reason: String) {
    lp.brain.refused();
    let reply = id.and_then(|id| take_pending(lp, &id)).flatten();
    if let Some(reply) = reply {
        let _ = reply.send(Err(refusal(&reason)));
    }
    eprintln!("tennoworth: presence change refused: {reason}");
    lp.publish(|s| {
        s.problem = Some(detail_problem(&reason));
        s.detail = Some(reason);
    });
}

fn fail_pending(lp: &mut Loop) {
    for (_, _, reply) in lp.pending.drain(..) {
        if let Some(reply) = reply {
            let _ = reply.send(Err(CmdError::of("presence_unavailable", "The status channel dropped before warframe.market answered. Your status may not have changed.")));
        }
    }
}

fn expire_pending(lp: &mut Loop, now: Instant) {
    lp.pending.retain_mut(|(_, sent, reply)| {
        if now.saturating_duration_since(*sent) < REPLY_TIMEOUT {
            return true;
        }
        if let Some(reply) = reply.take() {
            let _ = reply.send(Err(CmdError::of("presence_timeout", "warframe.market did not answer in time. Your status may not have changed.")));
        }
        false
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn following() -> PresenceSettings {
        PresenceSettings { follow_game: true, ..PresenceSettings::default() }
    }

    fn later(base: Instant, secs: u64) -> Instant {
        base + Duration::from_secs(secs)
    }

    fn managed(status: Presence) -> Option<Change> {
        Some(Change { status, duration: Some(MANAGED_DURATION_SECS) })
    }

    #[test]
    fn following_is_in_game_from_login_to_quit_then_invisible() {
        let t = Instant::now();
        let mut brain = Brain::new(following());
        assert_eq!(brain.game(true, t), managed(Presence::Ingame));
        assert_eq!(brain.game(true, t), None, "no change, no send");
        assert_eq!(brain.game(false, t), Some(Change { status: Presence::Invisible, duration: None }));
        assert!(!brain.managed(), "Invisible is not kept up");
    }

    #[test]
    fn online_when_closed_is_kept_up_while_the_app_runs() {
        let t = Instant::now();
        let mut brain = Brain::new(PresenceSettings { when_closed: PresenceChoice::Online, ..following() });
        brain.game(true, t);
        assert_eq!(brain.game(false, t), managed(Presence::Online));
        assert!(brain.managed());
    }

    #[test]
    fn without_following_the_game_changes_nothing() {
        let t = Instant::now();
        let mut brain = Brain::new(PresenceSettings::default());
        assert_eq!(brain.game(true, t), None);
        assert_eq!(brain.game(false, t), None);
    }

    #[test]
    fn a_change_on_the_website_wins_until_the_next_session() {
        let t = Instant::now();
        let mut brain = Brain::new(following());
        brain.game(true, t);
        brain.external_change();
        assert!(brain.follow_paused());
        assert!(!brain.managed());
        assert_eq!(brain.due(later(t, 3600)), None, "nothing is renewed over the user's choice");
        assert_eq!(brain.game(false, t), None, "closing while paused leaves the website's choice");
        assert_eq!(brain.game(true, t), managed(Presence::Ingame), "the next session follows again");
        assert!(!brain.follow_paused());
    }

    #[test]
    fn a_status_picked_here_pauses_following_and_follow_now_resumes() {
        let t = Instant::now();
        let mut brain = Brain::new(following());
        brain.game(true, t);
        assert_eq!(Some(brain.manual(PresenceChoice::Online, t)), managed(Presence::Online));
        assert!(brain.follow_paused());
        assert_eq!(brain.follow_now(t), managed(Presence::Ingame));
        assert!(!brain.follow_paused());
    }

    #[test]
    fn keep_for_mirrors_the_website_choices() {
        let t = Instant::now();
        let mut brain = Brain::new(PresenceSettings { keep_for_minutes: Some(60), ..PresenceSettings::default() });
        assert_eq!(brain.manual(PresenceChoice::Ingame, t), Change { status: Presence::Ingame, duration: Some(3600) });
        assert!(!brain.managed(), "a timed status is left to the server");
        let mut running = Brain::new(PresenceSettings::default());
        assert_eq!(Some(running.manual(PresenceChoice::Online, t)), managed(Presence::Online));
        assert_eq!(running.manual(PresenceChoice::Invisible, t), Change { status: Presence::Invisible, duration: None });
        assert!(!running.managed());
    }

    #[test]
    fn what_the_app_keeps_up_is_renewed_before_it_lapses() {
        let t = Instant::now();
        let mut brain = Brain::new(following());
        brain.game(true, t);
        assert_eq!(brain.due(later(t, 299)), None);
        assert_eq!(brain.due(later(t, 300)), managed(Presence::Ingame));
        assert_eq!(brain.due(later(t, 301)), None, "the renewal restarts the clock");
        assert!(u64::from(MANAGED_DURATION_SECS) > RENEW_EVERY.as_secs(), "renewal must land before the duration ends");
    }

    #[test]
    fn leaving_says_invisible_only_for_what_the_app_kept_up() {
        let t = Instant::now();
        let mut brain = Brain::new(following());
        assert_eq!(brain.leaving(), None);
        brain.game(true, t);
        assert_eq!(brain.leaving(), Some(Change { status: Presence::Invisible, duration: None }));
        let mut timed = Brain::new(PresenceSettings { keep_for_minutes: Some(240), ..PresenceSettings::default() });
        timed.manual(PresenceChoice::Online, t);
        assert_eq!(timed.leaving(), None, "a status kept for 4h outlives the app, as on the website");
    }

    #[test]
    fn turning_following_on_mid_session_takes_effect_at_once() {
        let t = Instant::now();
        let mut brain = Brain::new(PresenceSettings::default());
        brain.game(true, t);
        assert_eq!(brain.settings_changed(following(), t), managed(Presence::Ingame));
        assert_eq!(brain.settings_changed(following(), t), None);
    }

    #[test]
    fn reconnect_reasserts_only_what_the_app_keeps_up() {
        let t = Instant::now();
        let mut brain = Brain::new(following());
        assert_eq!(brain.reconnected(t), None);
        brain.game(true, t);
        assert_eq!(brain.reconnected(later(t, 30)), managed(Presence::Ingame));
    }

    #[test]
    fn a_refused_change_is_not_renewed() {
        let t = Instant::now();
        let mut brain = Brain::new(PresenceSettings::default());
        brain.manual(PresenceChoice::Ingame, t);
        brain.refused();
        assert_eq!(brain.due(later(t, 600)), None);
    }

    #[test]
    fn the_shutdown_line_ends_the_session_before_the_process_does() {
        let mut watch = GameWatch::default();
        assert!(watch.running(Some(7)));
        watch.ended(Some(7));
        assert!(!watch.running(Some(7)), "the exiting process is not a new session");
        assert!(!watch.running(None));
        assert!(watch.running(Some(7)), "the same pid number later is a new game");
        watch.ended(Some(9));
        watch.logged_in();
        assert!(watch.running(Some(9)), "a login in the same process is a session");
    }

    #[test]
    fn session_lines_come_from_the_real_log_markers() {
        assert_eq!(session_line("15.924 Sys [Info]: Logged in Tenno"), Some(true));
        assert_eq!(session_line("8723.157 Sys [Info]: Main Shutdown Initiated."), Some(false));
        assert_eq!(session_line("8723.058 Sys [Info]: ===[ Exiting main loop ]==="), Some(false));
        assert_eq!(session_line("19.353 Script [Info]: ThemedSquadOverlay.lua: OnLoginComplete"), None);
    }

    #[test]
    fn settings_snap_onto_the_offered_choices() {
        let odd = PresenceSettings { follow_game: true, when_closed: PresenceChoice::Ingame, keep_for_minutes: Some(50) };
        assert_eq!(
            odd.sanitized(),
            PresenceSettings { follow_game: true, when_closed: PresenceChoice::Invisible, keep_for_minutes: Some(60) }
        );
        assert_eq!(PresenceSettings::default().when_closed, PresenceChoice::Invisible);
        assert!(!PresenceSettings::default().follow_game);
    }

    #[test]
    fn the_tray_line_names_the_status() {
        let mut status = PresenceStatus::new(PresenceSettings::default());
        assert_eq!(status.tray_label(), "Status: unlock TennoWorth to change");
        status.signed_in = true;
        status.status = Some(PresenceChoice::Ingame);
        assert_eq!(status.tray_label(), "Status: Online in game");
        status.follow_paused = true;
        assert_eq!(status.tray_label(), "Status: Online in game · set by hand");
        status.problem = Some(PresenceProblem::NotVerified);
        assert_eq!(status.tray_label(), "Status: refused, account not verified");
    }

    #[test]
    fn refusal_codes_map_to_actionable_messages() {
        assert_eq!(refusal("app.errors.userNotVerified").code, "presence_not_verified");
        assert_eq!(refusal("duration: app.field.tooSmall").code, "presence_refused");
    }
}
