//! Desktop warframe.market session - the in-memory decrypted-JWT credential
//! lifecycle, mirroring serve's `ServeState` listing auth minus the terminal.
//!
//! The passphrase arrives from the webview (the `unlock_jwt` / `wfm_login`
//! commands) instead of a TTY prompt; wfm-core takes it as a parameter, exactly
//! as designed for this second adapter. The plaintext JWT lives ONLY inside this
//! process's memory for the session - never on disk (only the AES-GCM envelope,
//! whose format is unchanged), never in a log line, and never in a value handed
//! back to the SPA. `CmdError` carries a `code` + a human message and nothing
//! else; the raw password and the JWT never appear in it.
//!
//! Unlock is lazy and terminal-free: a listing command with no unlocked session
//! does NOT try to prompt (there is nowhere to prompt). It returns a typed
//! `needs_login` (no login file on this machine) or `needs_unlock` (login file
//! present, session locked) so the SPA can raise the login or passphrase modal -
//! the desktop analogue of serve's 401 `needs_login:true` vs 503 split.
#![allow(
    clippy::unreachable,
    reason = "tauri::command injects unreachable code into async wrappers"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::State;
use wfm_core::poison::guard;

use wfm_core::paths::{config_dir_for, default_jwt_path};
use wfm_core::platform::{chown_to_real_user, restrict_dir_perms, write_restricted};
use wfm_core::trading::auth::{
    decrypt_jwt_with_key, derive_jwt_key, encrypt_jwt, validate_passphrase, validate_platform,
    EncryptedJwt,
};
use wfm_core::trading::listing::{warm_unlocked, Unlocked};
use wfm_core::trading::plan::PlanGuard;
use zeroize::{Zeroize, Zeroizing};

/// Typed command error serialized to the webview as `{ code, message }`. The SPA
/// maps `code` to its own error classes:
///   - `needs_login`   - no login on this machine → open the login modal.
///   - `needs_unlock`  - login present, session locked → open the passphrase modal.
///   - `bad_passphrase`- wrong passphrase in the unlock/login modal.
///   - `no_pending` / `busy` - pending-plan resume edge cases.
///   - `wfm` / `internal` - everything else, message shown verbatim.
///
/// Never carries the JWT, the passphrase, or the WFM password.
#[derive(Debug, serde::Serialize, ts_rs::TS)]
pub struct CmdError {
    pub code: &'static str,
    pub message: String,
}

impl CmdError {
    pub fn of(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
    pub fn needs_login() -> Self {
        Self::of(
            "needs_login",
            "Log in to warframe.market to create or edit listings.",
        )
    }
    pub fn needs_unlock() -> Self {
        Self::of(
            "needs_unlock",
            "Enter your passphrase to unlock warframe.market listing.",
        )
    }
    pub fn bad_passphrase() -> Self {
        Self::of(
            "bad_passphrase",
            "Wrong passphrase, or the login file was modified.",
        )
    }
    pub fn wfm(e: anyhow::Error) -> Self {
        if let Some(access) = e.downcast_ref::<wfm_client::governor::AccessError>() { return Self::of(access.code(), access.to_string()); }
        Self::of("wfm", e.to_string())
    }
    pub fn internal(e: impl std::fmt::Display) -> Self {
        Self::of("internal", e.to_string())
    }
}

/// The session slot: the unlocked credentials plus the generation of the slot
/// they belong to. `generation` advances on every logout, so an installer that
/// began before the logout can recognise its own result as stale. It lives inside
/// the same mutex as the session on purpose - a separate atomic could be read
/// outside the lock, which is exactly the check/publication split this prevents.
#[derive(Default)]
struct SessionState {
    generation: u64,
    unlocked: Option<Arc<Unlocked>>,
}

/// Test-only stand-in for the catalog warm. The fixture performs the slow work
/// itself and returns the prepared session, so a test can place a logout between
/// authentication and the publication the real installer still performs.
#[cfg(test)]
type WarmHook = fn(&WfmSession, String, String) -> Result<Unlocked, CmdError>;

/// Which persisted trace a keyring change would leave: the derived key stored
/// for silent unlock, or the entry removed.
#[derive(Clone, Copy)]
enum KeyringIntent<'a> {
    Remember(&'a [u8; 32]),
    Forget,
}

/// One keyring decision, recorded instead of performed so a test can observe the
/// ordering rule without a Secret Service daemon.
#[cfg(test)]
type KeyringHook = fn(KeyringIntent<'_>, bool);

/// Stands in for reading the keyring entry, so a test can change what it holds
/// between two reads.
#[cfg(test)]
type KeyReadHook = fn() -> Option<[u8; 32]>;

/// The desktop WFM credential session. One instance is managed by Tauri; every
/// listing command borrows it via `State`.
pub struct WfmSession {
    /// Encrypted-JWT path. `TENNOWORTH_JWT_PATH` overrides it (a test/probe seam
    /// so a hermetic run controls whether a login file "exists").
    jwt_path: PathBuf,
    /// Pending-plan path. `TENNOWORTH_PENDING_PATH` overrides it so a probe
    /// doesn't touch the real `~/.config/wfminv/pending_plan.json`.
    pending_path: PathBuf,
    /// The unlocked credentials, or `None` when locked/unavailable, with the
    /// generation of the slot. The plaintext JWT lives ONLY inside this
    /// `Arc<Unlocked>` for the session's lifetime.
    inner: Mutex<SessionState>,
    /// Serializes plan execution: a second concurrent `execute_plan` /
    /// `resume_pending_plan` gets `busy` instead of racing on the pending file.
    plan_running: AtomicBool,
    plan_requests: Arc<Mutex<PlanRequests>>,
    /// "Remember on this device" is only offered against the REAL login file:
    /// any `TENNOWORTH_JWT_PATH` override (the probe/test seam) turns the OS
    /// keyring off entirely, so hermetic runs can never pollute - or unlock
    /// via - the user's actual keyring entry.
    use_keyring: bool,
    /// Replaces the network warm in tests only. A release build has no such
    /// field, so no shipping path can bypass authentication.
    #[cfg(test)]
    warm_hook: Option<WarmHook>,
    /// Reports each keyring intent to a test instead of performing it. The
    /// decision itself is real code; only the OS call is replaced.
    #[cfg(test)]
    keyring_hook: Option<KeyringHook>,
    #[cfg(test)]
    key_read_hook: Option<KeyReadHook>,
}

#[derive(Default)]
struct PlanRequests {
    pending: std::collections::HashMap<String, (Arc<AtomicBool>, bool)>,
    completed: std::collections::VecDeque<String>,
}

pub struct PlanRequestScope {
    requests: Arc<Mutex<PlanRequests>>,
    id: String,
    cancelled: Arc<AtomicBool>,
}
impl PlanRequestScope {
    pub fn context(&self) -> wfm_client::governor::Context {
        wfm_client::governor::Context { cancelled: Some(self.cancelled.clone()), ..Default::default() }
    }
}
impl Drop for PlanRequestScope {
    fn drop(&mut self) {
        let mut requests = guard(&self.requests);
        requests.pending.remove(&self.id);
        if requests.completed.len() >= wfm_client::governor::MAX_QUEUE { requests.completed.pop_front(); }
        requests.completed.push_back(self.id.clone());
    }
}

impl WfmSession {
    pub fn new() -> Self {
        let overridden = std::env::var_os("TENNOWORTH_JWT_PATH");
        let use_keyring = overridden.is_none();
        let jwt_path = overridden
            .map(PathBuf::from)
            .unwrap_or_else(default_jwt_path);
        // Relocating credentials also relocates recovery state unless the
        // probe explicitly overrides the pending-plan path.
        let key_dir = config_dir_for(&jwt_path);
        let pending_path = std::env::var_os("TENNOWORTH_PENDING_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| key_dir.join("pending_plan.json"));
        Self {
            jwt_path,
            pending_path,
            inner: Mutex::new(SessionState::default()),
            plan_running: AtomicBool::new(false),
            plan_requests: Arc::new(Mutex::new(PlanRequests::default())),
            use_keyring,
            #[cfg(test)]
            warm_hook: None,
            #[cfg(test)]
            keyring_hook: None,
            #[cfg(test)]
            key_read_hook: None,
        }
    }

    /// A session rooted at explicit paths, with the OS keyring off.
    ///
    /// [`Self::new`] reads `TENNOWORTH_JWT_PATH` and friends from the
    /// environment, so two tests constructing a session concurrently could
    /// observe each other's overrides. Production keeps using `new`.
    #[cfg(test)]
    pub(crate) fn for_test(jwt_path: PathBuf) -> Self {
        let key_dir = config_dir_for(&jwt_path);
        Self {
            jwt_path,
            pending_path: key_dir.join("pending_plan.json"),
            inner: Mutex::new(SessionState::default()),
            plan_running: AtomicBool::new(false),
            plan_requests: Arc::new(Mutex::new(PlanRequests::default())),
            use_keyring: false,
            warm_hook: None,
            keyring_hook: None,
            key_read_hook: None,
        }
    }

    pub fn pending_path(&self) -> &Path {
        &self.pending_path
    }

    pub fn is_unlocked(&self) -> bool {
        guard(&self.inner).unlocked.is_some()
    }

    /// The generation any installer must capture before it starts reading
    /// credentials or authenticating, and present again to publish.
    pub(crate) fn session_generation(&self) -> u64 {
        guard(&self.inner).generation
    }

    /// Publish a prepared session, unless a logout superseded the work that
    /// produced it. Checking and publishing inside one critical section is what
    /// makes "logout wins" hold: a logout either ran first (this sees a newer
    /// generation and discards) or runs after (and takes the session away again).
    fn install_unlocked(&self, generation: u64, mut unlocked: Unlocked) -> Result<(), CmdError> {
        let mut state = guard(&self.inner);
        if state.generation != generation {
            drop(state);
            unlocked.jwt.zeroize();
            return Err(CmdError::of(
                "session_changed",
                "The session changed while signing in. Sign in again.",
            ));
        }
        state.unlocked = Some(Arc::new(unlocked));
        Ok(())
    }

    /// `(logged_in, unlocked)` for the desktop UI's login affordance:
    /// `logged_in` = a login file exists on disk; `unlocked` = this session
    /// holds the decrypted JWT.
    pub fn auth_status(&self) -> (bool, bool) {
        (self.jwt_path.exists(), self.is_unlocked())
    }

    /// Apply a keyring change that belongs to the session `generation`, unless a
    /// logout has superseded it.
    ///
    /// The persisted traces are the second half of "logout wins": an unlock or
    /// login publishes its session and then reaches for the OS keyring, and a
    /// logout landing in that gap would clear the session and forget the key only
    /// for the straggler to store it again - leaving the next launch able to
    /// silently re-unlock a session the user discarded. A logout advances the
    /// generation, so a mismatched generation means the decision is void.
    ///
    /// This is a check-then-write, not a transaction. Making it atomic would mean
    /// holding the session lock across Secret Service I/O, which on a locked or
    /// absent wallet blocks every command behind a timeout - worse than the
    /// window it closes. The residual window is the store call itself, and what
    /// it leaves is a stale keyring entry rather than a stale session: the entry
    /// is salt-bound to the login file logout deletes, so a later silent unlock
    /// finds no file and falls back to the passphrase modal.
    fn apply_keyring(&self, generation: u64, intent: KeyringIntent) {
        if !self.use_keyring {
            return;
        }
        // The hook stands in for the two OS calls below, not for this decision:
        // it receives the intent even when the decision refuses it, so a test
        // observes what would have been attempted.
        #[cfg(test)]
        let hook = self.keyring_hook;
        if self.session_generation() != generation {
            #[cfg(test)]
            if let Some(hook) = hook {
                hook(intent, false);
            }
            return;
        }
        #[cfg(test)]
        if let Some(hook) = hook {
            hook(intent, true);
            return;
        }
        match intent {
            KeyringIntent::Remember(key) => crate::persistence::keyring_store::store_key(key),
            KeyringIntent::Forget => crate::persistence::keyring_store::forget_key(),
        }
    }

    fn load_keyring_key(&self) -> Option<[u8; 32]> {
        #[cfg(test)]
        if let Some(hook) = self.key_read_hook {
            return hook();
        }
        crate::persistence::keyring_store::load_key()
    }

    /// Log out, scrub the in-memory JWT, and remove the encrypted login saved on
    /// this device. Best-effort scrub:
    /// if a listing call is in flight it holds a clone of the Arc, so we can't be
    /// the sole owner; dropping still frees the plaintext, just without an
    /// explicit overwrite first. Also forgets the remembered device key -
    /// an explicit logout that silently re-unlocked itself wouldn't be one.
    pub fn logout(&self) -> Result<(), CmdError> {
        let _logout_guard = self.begin_plan().ok_or_else(|| {
            CmdError::of(
                "busy",
                "Wait for the active listing batch to finish before logging out.",
            )
        })?;
        match fs::remove_file(&self.pending_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(CmdError::internal(format!(
                    "could not discard the interrupted listing batch at {}: {error}",
                    self.pending_path.display()
                )))
            }
        }
        if let Err(error) = fs::remove_file(&self.jwt_path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                return Err(CmdError::internal(format!(
                    "could not remove the saved warframe.market login at {}: {error}",
                    self.jwt_path.display()
                )));
            }
        }
        wfm_client::transport::invalidate_reads();
        // Advance the generation and take the session inside one critical
        // section, so an unlock still authenticating cannot publish into the slot
        // it started from. Advanced even when the slot is already empty: that is
        // exactly the startup case where a silent unlock is in flight.
        let (taken, generation) = {
            let mut state = guard(&self.inner);
            state.generation = state.generation.checked_add(1).ok_or_else(|| {
                CmdError::internal("Session generation exhausted; restart the application.")
            })?;
            (state.unlocked.take(), state.generation)
        };
        if let Some(arc) = taken {
            if let Ok(mut unlocked) = Arc::try_unwrap(arc) {
                unlocked.jwt.zeroize();
            }
        }
        // The generation this logout established, so the forget is keyed to the
        // session state it created rather than the one it replaced.
        self.apply_keyring(generation, KeyringIntent::Forget);
        Ok(())
    }

    /// The unlocked credentials, or a typed error WITHOUT attempting an unlock -
    /// there is no passphrase at a listing call site (no terminal), so the SPA
    /// must drive `unlock_jwt` first. `needs_login` vs `needs_unlock` is decided
    /// by whether a login file exists (serve's `NeedsLogin` vs a present-but-
    /// locked blob).
    pub fn require_unlocked(&self) -> Result<Arc<Unlocked>, CmdError> {
        if let Some(u) = guard(&self.inner).unlocked.as_ref() {
            return Ok(Arc::clone(u));
        }
        if self.jwt_path.exists() {
            Err(CmdError::needs_unlock())
        } else {
            Err(CmdError::needs_login())
        }
    }

    pub fn begin_plan(&self) -> Option<PlanGuard<'_>> {
        PlanGuard::acquire(&self.plan_running)
    }

    fn request_token(&self, id: &str, claim: bool) -> Result<Option<Arc<AtomicBool>>, CmdError> {
        if id.is_empty() || id.len() > 64 || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') {
            return Err(CmdError::of("wfm", "Invalid listing request identity."));
        }
        let mut requests = guard(&self.plan_requests);
        if requests.completed.iter().any(|completed| completed == id) { return Ok(None); }
        if requests.pending.len() >= wfm_client::governor::MAX_QUEUE && !requests.pending.contains_key(id) {
            return Err(CmdError::of("busy", "Too many listing requests are waiting."));
        }
        let entry = requests.pending.entry(id.into()).or_insert_with(|| (Arc::new(AtomicBool::new(false)), false));
        if claim {
            if entry.1 { return Err(CmdError::of("busy", "This listing request is already running.")); }
            entry.1 = true;
        }
        Ok(Some(entry.0.clone()))
    }
    pub fn claim_plan_request(&self, id: String) -> Result<PlanRequestScope, CmdError> {
        let cancelled = self.request_token(&id, true)?.ok_or_else(|| CmdError::of("busy", "This listing request already finished."))?;
        Ok(PlanRequestScope { requests: self.plan_requests.clone(), id, cancelled })
    }
    pub fn cancel_plan(&self, id: &str) -> Result<(), CmdError> {
        if let Some(token) = self.request_token(id, false)? { token.store(true, std::sync::atomic::Ordering::Release); }
        Ok(())
    }

    /// Read + parse the on-disk envelope - shared by the passphrase and
    /// silent-unlock paths, with the same error mapping (missing file →
    /// `needs_login`).
    fn read_blob(&self) -> Result<EncryptedJwt, CmdError> {
        let bytes = match fs::read(&self.jwt_path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(CmdError::needs_login())
            }
            Err(e) => {
                return Err(CmdError::internal(format!(
                    "reading login file {}: {e}",
                    self.jwt_path.display()
                )))
            }
        };
        serde_json::from_slice(&bytes)
            .map_err(|e| CmdError::internal(format!("login file is unreadable: {e}")))
    }

    /// Read + decrypt the on-disk JWT with `passphrase` - the offline half of
    /// `unlock`. Returns `(jwt_plaintext, platform, derived_key)`. Split out so
    /// the error mapping (missing file → `needs_login`, wrong passphrase →
    /// `bad_passphrase`) is unit-testable without the network catalog warm.
    fn decrypt_from_disk(&self, passphrase: &str) -> Result<(String, String, [u8; 32]), CmdError> {
        let blob = self.read_blob()?;
        let platform = blob.platform.clone();
        // Any decrypt failure (wrong key or tampered ciphertext) reads as a bad
        // passphrase - the only actionable cause from the user's side.
        let key = derive_jwt_key(&blob, passphrase).map_err(|_| CmdError::bad_passphrase())?;
        let jwt = decrypt_jwt_with_key(&blob, &key).map_err(|_| CmdError::bad_passphrase())?;
        Ok((jwt, platform, key))
    }

    /// The pre-publication work every installer shares: warm the WFM catalog with
    /// the JWT it already holds. Split out so a test can interleave a logout
    /// between authentication and publication without touching the network.
    fn warm_session(&self, jwt: String, platform: String) -> Result<Unlocked, CmdError> {
        #[cfg(test)]
        if let Some(hook) = self.warm_hook {
            return hook(self, jwt, platform);
        }
        warm(jwt, platform)
    }

    /// Decrypt the on-disk JWT and warm the WFM catalog, populating the session.
    /// Network: `/v2/items` + `/v2/me`. On success the plaintext JWT is held only
    /// inside the session `Arc`; with `remember`, the salt-bound derived key
    /// (never the passphrase) also goes to the OS keyring for silent unlock.
    /// Remember only on FULL success - an unlock the user abandons after a
    /// network failure should leave no trace.
    pub fn unlock(&self, passphrase: &str, remember: bool) -> Result<(), CmdError> {
        let generation = self.session_generation();
        let (jwt, platform, key) = self.decrypt_from_disk(passphrase)?;
        let unlocked = self.warm_session(jwt, platform)?;
        wfm_client::transport::invalidate_reads();
        self.install_unlocked(generation, unlocked)?;
        // A silent re-unlock must not outlive the logout that ended the session
        // it belongs to, so the keyring change is tied to `generation` too.
        self.apply_keyring(
            generation,
            if remember {
                KeyringIntent::Remember(&key)
            } else {
                // Unticking the box is an explicit "stop remembering".
                KeyringIntent::Forget
            },
        );
        Ok(())
    }

    /// Try the OS-keyring key against the current login file - the silent
    /// analogue of `unlock`, called by the SPA before it raises the passphrase
    /// modal. Never fails the caller: every miss (no entry, no daemon, network
    /// warm failure) is `Ok(false)` → the modal opens as before. The entry is
    /// deleted ONLY on a definitive GCM auth failure (stale after a re-login),
    /// never on transient store errors.
    pub fn try_silent_unlock(&self) -> bool {
        if self.is_unlocked() {
            return true;
        }
        let generation = self.session_generation();
        if !self.use_keyring || !self.jwt_path.exists() {
            return false;
        }
        let Some(key) = self.load_keyring_key() else {
            return false;
        };
        let Ok(blob) = self.read_blob() else {
            return false;
        };
        let platform = blob.platform.clone();
        let jwt = match decrypt_jwt_with_key(&blob, &key) {
            Ok(jwt) => jwt,
            Err(_) => {
                // The key no longer opens the file, so the entry is stale - but
                // only if it is still the entry that failed. A sign-in that
                // landed meanwhile rotated the salt and stored a different key,
                // and the generation cannot reveal it: only logout advances it.
                // What is left is the gap between this read and the delete.
                if self.load_keyring_key() == Some(key) {
                    self.apply_keyring(generation, KeyringIntent::Forget);
                }
                return false;
            }
        };
        match self.warm_session(jwt, platform) {
            Ok(unlocked) => {
                wfm_client::transport::invalidate_reads();
                // A logout during the warm supersedes this unlock; report a miss,
                // exactly as any other silent-unlock miss does.
                self.install_unlocked(generation, unlocked).is_ok()
            }
            Err(e) => {
                // Key is good; the warm (network) failed. Keep the entry and
                // let the passphrase modal surface the error on retry.
                eprintln!("tennoworth: silent unlock warm failed: {}", e.message);
                false
            }
        }
    }

    /// The checks a sign-in must pass before the sign-in window opens, so a
    /// short passphrase is refused before the user types their WFM password.
    pub fn validate_login(passphrase: &str, platform: &str) -> Result<(), CmdError> {
        validate_platform(platform).map_err(CmdError::internal)?;
        // Shared with wfm-core so the floor cannot drift (it had: bytes in the
        // old CLI, chars here).
        validate_passphrase(passphrase).map_err(|e| CmdError::internal(e.to_string()))
    }

    /// Persist the JWT from a completed warframe.market sign-in (unchanged
    /// on-disk format) and populate the session with it - so the first listing
    /// action doesn't re-prompt for the passphrase the user just set. The
    /// passphrase only encrypts and is not retained here.
    pub fn login(
        &self,
        generation: u64,
        jwt: String,
        passphrase: &str,
        platform: &str,
        remember: bool,
    ) -> Result<(), CmdError> {
        Self::validate_login(passphrase, platform)?;

        let encrypted = encrypt_jwt(&jwt, passphrase, platform).map_err(CmdError::internal)?;
        // A logout that completed while this login was authenticating removed the
        // saved credential; recreating it here would hand the next launch a login
        // the user just discarded. Checked as late as possible, but this is still
        // a check-then-write: a logout landing between the two re-adds the file,
        // and the session stays locked out until the next explicit sign-in.
        if self.session_generation() != generation {
            return Err(CmdError::of(
                "session_changed",
                "The session changed while signing in. Sign in again.",
            ));
        }
        self.persist(&encrypted)?;

        // Warm the session with the in-hand JWT (no redundant decrypt). If the
        // catalog warm fails the JWT is already saved, so a later listing action
        // unlocks via the passphrase modal - surface the network error either way.
        let unlocked = self.warm_session(jwt, platform.to_string())?;
        wfm_client::transport::invalidate_reads();
        self.install_unlocked(generation, unlocked)?;
        if remember {
            // A fresh login rotated the salt, so derive against the blob we just
            // persisted - any older keyring entry is overwritten. A derivation
            // failure leaves the session usable and costs a passphrase prompt
            // next launch, so it is not worth failing the sign-in over.
            match derive_jwt_key(&encrypted, passphrase) {
                Ok(key) => self.apply_keyring(generation, KeyringIntent::Remember(&key)),
                Err(e) => eprintln!("tennoworth: deriving remember-key failed: {e}"),
            }
        } else {
            self.apply_keyring(generation, KeyringIntent::Forget);
        }
        Ok(())
    }

    fn persist(&self, encrypted: &EncryptedJwt) -> Result<(), CmdError> {
        if let Some(parent) = self.jwt_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| CmdError::internal(format!("creating config dir: {e}")))?;
            restrict_dir_perms(parent);
            chown_to_real_user(parent);
        }
        let serialized = serde_json::to_vec_pretty(encrypted).map_err(CmdError::internal)?;
        write_restricted(&self.jwt_path, &serialized).map_err(CmdError::internal)?;
        chown_to_real_user(&self.jwt_path);
        Ok(())
    }

    /// Seed an unlocked session with an already-built bundle, skipping the WFM
    /// network warm. Lets a hermetic run flip `is_unlocked` and exercise the
    /// listing command path without a live warframe.market. Reachable in a
    /// release build only through the `debug_seed_unlocked` command, which is
    /// itself runtime-gated behind `TENNOWORTH_PROBE=1` (same pattern as the
    /// other `debug_*` probe commands).
    pub fn debug_set_unlocked(&self, unlocked: Unlocked) {
        wfm_client::transport::invalidate_reads();
        // Seeds a fixture through the same slot a real install uses; a logout
        // still takes it away again, so the probe cannot pin a session past one.
        guard(&self.inner).unlocked = Some(Arc::new(unlocked));
    }

    /// Probe-only companion to `debug_set_unlocked`: write a real encrypted
    /// envelope (same `encrypt_jwt` + `persist` production code) at the
    /// session's jwt_path so a hermetic run can exercise the needs_unlock /
    /// bad_passphrase branches against a genuine AES-GCM blob, no WFM login.
    pub fn debug_write_login(&self, passphrase: &str) -> Result<(), CmdError> {
        let encrypted =
            encrypt_jwt("probe.jwt.value", passphrase, "pc").map_err(CmdError::internal)?;
        self.persist(&encrypted)
    }
}

impl Default for WfmSession {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the `Unlocked` bundle (catalog + username) for an already-decrypted
/// JWT. Shared by `unlock` (decrypt path) and `login` (fresh-JWT path). This is
/// the only network in the session module - everything above is offline.
///
/// The warm itself lives in wfm-core; this wrapper exists for the error
/// mapping, which is the only thing that ever differed from serve's copy.
/// wfm-core returns anyhow so it owes nothing to either shell, and the network
/// failures a user sees here are WFM's.
fn warm(jwt: String, platform: String) -> Result<Unlocked, CmdError> {
    warm_unlocked(jwt, platform).map_err(CmdError::wfm)
}

// ---- Tauri commands ---------------------------------------------------
//
// The desktop mirror of serve's auth routes: same lock-state machine, with
// the passphrase arriving from the webview (`wfm_login` / `unlock_jwt`)
// instead of a TTY prompt. `needs_login` / `needs_unlock` drive the SPA's
// login and passphrase dialogs - the desktop analogue of serve's 401
// needs_login:true vs 503 split.

#[derive(serde::Serialize)]
pub struct WfmAuthStatus {
    /// A login envelope exists on disk (encrypted; says nothing about the
    /// passphrase being known).
    logged_in: bool,
    /// This process holds the decrypted JWT in memory.
    unlocked: bool,
}

#[tauri::command]
pub fn wfm_auth_status(session: State<'_, Arc<WfmSession>>) -> WfmAuthStatus {
    let (logged_in, unlocked) = session.auth_status();
    WfmAuthStatus {
        logged_in,
        unlocked,
    }
}

/// Open the warframe.market sign-in window, wait for the user to sign in
/// there, then persist the encrypted JWT (unchanged envelope format) and unlock
/// the session. The WFM password is typed into warframe.market's own page and
/// never reaches the app. Async by necessity: reading webview cookies from a
/// synchronous command deadlocks on Windows.
#[tauri::command]
pub async fn wfm_login(
    app: tauri::AppHandle,
    session: State<'_, Arc<WfmSession>>,
    passphrase: String,
    platform: String,
    remember: bool,
) -> Result<(), CmdError> {
    // Zeroizing scrubs OUR copy of the passphrase when this ends - best-effort
    // (the IPC deserializer made its own transient copies).
    let passphrase = Zeroizing::new(passphrase);
    WfmSession::validate_login(&passphrase, &platform)?;
    let generation = session.session_generation();
    let jwt = super::wfm_signin::capture_signed_in_jwt(&app, &platform).await?;
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        s.login(generation, jwt, &passphrase, &platform, remember)
    })
    .await
    .map_err(|e| CmdError::internal(format!("login task failed to run: {e}")))?
}

/// Close the sign-in window from the app's login dialog; the pending
/// `wfm_login` then fails with `cancelled`.
#[tauri::command]
pub fn wfm_login_cancel(app: tauri::AppHandle) {
    super::wfm_signin::cancel(&app);
}

/// Decrypt the stored JWT with the passphrase from the SPA's unlock dialog and
/// warm the WFM catalog. Missing file → `needs_login`; wrong passphrase →
/// `bad_passphrase`; catalog/me failure → `wfm` (transient, retryable).
#[tauri::command]
pub async fn unlock_jwt(
    session: State<'_, Arc<WfmSession>>,
    passphrase: String,
    remember: bool,
) -> Result<(), CmdError> {
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let passphrase = Zeroizing::new(passphrase);
        s.unlock(&passphrase, remember)
    })
    .await
    .map_err(|e| CmdError::internal(format!("unlock task failed to run: {e}")))?
}

/// Try the OS-keyring "remember on this device" key before the SPA raises the
/// passphrase modal. Infallible by contract: any miss (no entry, no keyring
/// daemon, stale key, network warm failure) returns false and the modal opens
/// exactly as before. Network on success (catalog warm) - spawn_blocking.
#[tauri::command]
pub async fn try_silent_unlock(session: State<'_, Arc<WfmSession>>) -> Result<bool, CmdError> {
    let s = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || s.try_silent_unlock())
        .await
        .map_err(|e| CmdError::internal(format!("silent-unlock task failed to run: {e}")))
}

/// Log out and remove both the live session and its encrypted on-disk login.
#[tauri::command]
pub fn wfm_logout(session: State<'_, Arc<WfmSession>>) -> Result<(), CmdError> {
    session.logout()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU32, Ordering};

    // Unique temp paths per test so a parallel test run never collides on the
    // shared jwt/pending files.
    fn tmp_path(tag: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let mut p = std::env::temp_dir();
        p.push(format!(
            "wfmsession-{}-{}-{}.enc",
            std::process::id(),
            tag,
            N.fetch_add(1, Ordering::SeqCst)
        ));
        p
    }

    fn session_with(jwt_path: PathBuf) -> WfmSession {
        let pending = jwt_path.with_extension("pending.json");
        WfmSession {
            jwt_path,
            pending_path: pending,
            inner: Mutex::new(SessionState::default()),
            plan_running: AtomicBool::new(false),
            plan_requests: Arc::new(Mutex::new(PlanRequests::default())),
            // Tests must never read or write the developer's real OS keyring.
            use_keyring: false,
            warm_hook: None,
            keyring_hook: None,
            key_read_hook: None,
        }
    }

    fn dummy_unlocked() -> Unlocked {
        Unlocked {
            jwt: "jwt.header.body.sig".into(),
            username: "tester".into(),
            platform: "pc".into(),
            catalog: Arc::new(BTreeMap::new()),
            id_to_item: Arc::new(BTreeMap::new()),
        }
    }

    // --- keyring ordering -------------------------------------------------
    //
    // The keyring is the persisted half of a session. These drive the real
    // install path and record the intent instead of calling the OS, so the
    // ordering rule is observable on a build host with no Secret Service.

    static KEYRING_LOG: Mutex<Vec<(bool, bool)>> = Mutex::new(Vec::new());

    /// Serialises the tests that share `KEYRING_LOG`; each takes it for its whole
    /// body so another test cannot append between a reset and its assertion.
    static KEYRING_TESTS: Mutex<()> = Mutex::new(());

    /// Applies the decision in place of the OS call. `permitted` is computed by
    /// `apply_keyring` from the generation it was given, so a test can drive an
    /// interleaving and then ask what that decision would have done.
    fn record_keyring(intent: KeyringIntent<'_>, permitted: bool) {
        KEYRING_LOG
            .lock()
            .expect("keyring log")
            .push((matches!(intent, KeyringIntent::Remember(_)), permitted));
    }

    fn reset_keyring_log() {
        KEYRING_LOG.lock().expect("keyring log").clear();
    }

    fn keyring_log() -> Vec<(bool, bool)> {
        KEYRING_LOG.lock().expect("keyring log").clone()
    }

    /// A session with a real encrypted login on disk, the writing half of the
    /// keyring enabled, and its decisions recorded.
    fn keyring_session(tag: &str) -> WfmSession {
        let path = tmp_path(tag);
        fs::write(
            &path,
            serde_json::to_vec(
                &encrypt_jwt("jwt.header.body.sig", PASSPHRASE, "pc").expect("encrypt"),
            )
            .expect("serialize"),
        )
        .expect("write login file");
        let mut s = session_with(path);
        s.use_keyring = true;
        s.keyring_hook = Some(record_keyring);
        s
    }

    const PASSPHRASE: &str = "correct horse battery";

    /// A logout that lands after the session is published but before the keyring
    /// write must take the keyring entry with it.
    ///
    /// This is the window the publish guard cannot cover: the unlock genuinely
    /// succeeded, so only the keyring decision's own generation check stands
    /// between a discarded session and an entry that would silently re-unlock it
    /// on the next launch. The interleaving is driven between the guard and the
    /// action because that is where production separates them.
    #[test]
    fn a_logout_at_the_keyring_decision_stops_the_entry_being_restored() {
        let _serial = KEYRING_TESTS.lock().expect("keyring tests");
        reset_keyring_log();
        let s = keyring_session("keyring-logout-at-decision");

        // The logout runs first, then the interrupted unlock reaches the keyring
        // with the generation it captured before its session was discarded.
        let read_generation = s.session_generation();
        s.logout().expect("logout lands while the unlock is still in flight");
        s.apply_keyring(read_generation, KeyringIntent::Remember(&[7_u8; 32]));

        // The logout's own forget is permitted; the straggler's store is not.
        assert_eq!(keyring_log(), vec![(false, true), (true, false)]);
    }

    /// The positive control: with no logout in the way, the same path still
    /// stores the key. Without this the guard above could pass by refusing
    /// every write.
    #[test]
    fn an_uninterrupted_unlock_still_stores_the_keyring_entry() {
        let _serial = KEYRING_TESTS.lock().expect("keyring tests");
        reset_keyring_log();
        let mut s = keyring_session("keyring-store");
        s.warm_hook = Some(|_session, _jwt, _platform| Ok(dummy_unlocked()));

        s.unlock(PASSPHRASE, true).expect("unlock publishes");

        assert_eq!(keyring_log(), vec![(true, true)]);
    }

    /// A forget decided before a logout belongs to the session the logout
    /// discarded, so it is void.
    #[test]
    fn a_forget_decided_before_a_logout_is_void() {
        let _serial = KEYRING_TESTS.lock().expect("keyring tests");
        reset_keyring_log();
        let mut s = keyring_session("keyring-stale-forget");
        s.keyring_hook = Some(record_keyring);
        let read_generation = s.session_generation();
        s.logout().expect("logout");

        s.apply_keyring(read_generation, KeyringIntent::Forget);

        assert_eq!(keyring_log().last(), Some(&(false, false)));
    }

    /// The same decision made against the current generation is permitted, so
    /// the test above is not passing because `apply_keyring` refuses everything.
    #[test]
    fn a_stale_key_is_forgotten_while_its_session_still_stands() {
        let _serial = KEYRING_TESTS.lock().expect("keyring tests");
        reset_keyring_log();
        let mut s = keyring_session("keyring-forget");
        s.keyring_hook = Some(record_keyring);

        s.apply_keyring(s.session_generation(), KeyringIntent::Forget);

        assert_eq!(keyring_log(), vec![(false, true)]);
    }

    /// Keyring reads served to `try_silent_unlock`, in order.
    static KEY_READS: Mutex<Vec<[u8; 32]>> = Mutex::new(Vec::new());

    fn next_key_read() -> Option<[u8; 32]> {
        let mut reads = KEY_READS.lock().expect("key reads");
        (!reads.is_empty()).then(|| reads.remove(0))
    }

    fn silent_unlock_forgets(reads: [[u8; 32]; 2]) -> Vec<(bool, bool)> {
        let _serial = KEYRING_TESTS.lock().expect("keyring tests");
        reset_keyring_log();
        *KEY_READS.lock().expect("key reads") = reads.to_vec();
        let mut s = keyring_session("keyring-silent-forget");
        s.key_read_hook = Some(next_key_read);

        assert!(!s.try_silent_unlock(), "the key does not open the login file");
        keyring_log()
    }

    /// A sign-in that finishes while a silent unlock is decrypting replaces the
    /// login file and stores a new key. The unlock's failure is about the key it
    /// read, so it must not delete the one the sign-in stored. Sign-in does not
    /// advance the session generation, so only the entry itself can show this.
    #[test]
    fn a_silent_unlock_does_not_forget_a_key_a_sign_in_just_stored() {
        assert_eq!(silent_unlock_forgets([[1; 32], [2; 32]]), vec![]);
    }

    /// The positive control: an entry that still holds the failing key is stale
    /// and is removed.
    #[test]
    fn a_silent_unlock_forgets_a_key_that_no_longer_opens_the_login() {
        assert_eq!(silent_unlock_forgets([[1; 32], [1; 32]]), vec![(false, true)]);
    }

    #[test]
    fn require_unlocked_with_no_login_file_is_needs_login() {
        let path = tmp_path("no-login");
        let _ = fs::remove_file(&path);
        let s = session_with(path);
        let err = s.require_unlocked().err().expect("expected a typed error");
        assert_eq!(err.code, "needs_login");
    }

    #[test]
    fn require_unlocked_with_login_file_but_locked_is_needs_unlock() {
        let path = tmp_path("locked");
        // Any file at the path counts as "a login exists" for the classification.
        fs::write(
            &path,
            serde_json::to_vec(&encrypt_jwt("j", "correct horse battery", "pc").unwrap()).unwrap(),
        )
        .unwrap();
        let s = session_with(path.clone());
        let err = s.require_unlocked().err().expect("expected a typed error");
        assert_eq!(err.code, "needs_unlock");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn require_unlocked_returns_creds_when_session_is_unlocked() {
        // Unlocked takes priority over the on-disk check - even with no file.
        let path = tmp_path("unlocked");
        let _ = fs::remove_file(&path);
        let s = session_with(path);
        s.debug_set_unlocked(dummy_unlocked());
        let creds = s.require_unlocked().expect("unlocked session yields creds");
        assert_eq!(creds.username, "tester");
        assert!(s.is_unlocked());
    }

    #[test]
    fn decrypt_from_disk_missing_file_is_needs_login() {
        let path = tmp_path("decrypt-missing");
        let _ = fs::remove_file(&path);
        let s = session_with(path);
        let err = s.decrypt_from_disk("whatever passphrase").unwrap_err();
        assert_eq!(err.code, "needs_login");
    }

    #[test]
    fn decrypt_from_disk_wrong_passphrase_is_bad_passphrase() {
        let path = tmp_path("decrypt-wrong");
        let blob = encrypt_jwt("jwt.secret.value", "the-correct-passphrase", "pc").unwrap();
        fs::write(&path, serde_json::to_vec(&blob).unwrap()).unwrap();
        let s = session_with(path.clone());
        let err = s.decrypt_from_disk("the-WRONG-passphrase").unwrap_err();
        assert_eq!(err.code, "bad_passphrase");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn decrypt_from_disk_correct_passphrase_returns_jwt_and_platform() {
        let path = tmp_path("decrypt-ok");
        let blob = encrypt_jwt("jwt.secret.value", "the-correct-passphrase", "ps4").unwrap();
        fs::write(&path, serde_json::to_vec(&blob).unwrap()).unwrap();
        let s = session_with(path.clone());
        let (jwt, platform, key) = s.decrypt_from_disk("the-correct-passphrase").unwrap();
        assert_eq!(jwt, "jwt.secret.value");
        assert_eq!(platform, "ps4");
        // The derived key it hands back must actually open the same envelope -
        // that key is what "remember on this device" stores.
        let blob: EncryptedJwt = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            decrypt_jwt_with_key(&blob, &key).unwrap(),
            "jwt.secret.value"
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn decrypt_from_disk_corrupt_file_is_internal_not_bad_passphrase() {
        // A present-but-garbage file must not read as "wrong passphrase" - that
        // would send the user in circles retyping a correct passphrase.
        let path = tmp_path("decrypt-corrupt");
        fs::write(&path, b"{not valid json at all").unwrap();
        let s = session_with(path.clone());
        let err = s.decrypt_from_disk("anything").unwrap_err();
        assert_eq!(err.code, "internal");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn logout_clears_the_session_and_saved_login() {
        let path = tmp_path("logout");
        let pending = path.with_extension("pending.json");
        fs::write(&path, b"encrypted login placeholder").unwrap();
        fs::write(&pending, b"pending listing placeholder").unwrap();
        let s = session_with(path.clone());
        s.debug_set_unlocked(dummy_unlocked());
        assert!(s.is_unlocked());
        s.logout().unwrap();
        assert!(!s.is_unlocked());
        assert!(!path.exists());
        assert!(!pending.exists());
        assert_eq!(s.auth_status(), (false, false));
        assert_eq!(
            s.require_unlocked()
                .err()
                .expect("expected a typed error")
                .code,
            "needs_login"
        );
    }

    #[test]
    fn logout_reports_when_the_saved_login_cannot_be_removed() {
        let path = tmp_path("logout-remove-error");
        let pending = path.with_extension("pending.json");
        fs::create_dir(&path).unwrap();
        fs::write(&pending, b"pending listing placeholder").unwrap();
        let s = session_with(path.clone());
        s.debug_set_unlocked(dummy_unlocked());

        let error = s.logout().unwrap_err();

        assert_eq!(error.code, "internal");
        assert!(error
            .message
            .contains("could not remove the saved warframe.market login"));
        assert!(s.is_unlocked());
        assert!(path.exists());
        assert!(!pending.exists());
        fs::remove_dir(&path).unwrap();
    }

    #[test]
    fn logout_keeps_the_login_live_when_the_pending_batch_cannot_be_removed() {
        let path = tmp_path("logout-pending-error");
        let pending = path.with_extension("pending.json");
        fs::write(&path, b"encrypted login placeholder").unwrap();
        fs::create_dir(&pending).unwrap();
        let s = session_with(path.clone());
        s.debug_set_unlocked(dummy_unlocked());

        let error = s.logout().unwrap_err();

        assert_eq!(error.code, "internal");
        assert!(error
            .message
            .contains("could not discard the interrupted listing batch"));
        assert!(s.is_unlocked());
        assert!(path.exists());
        fs::remove_dir(&pending).unwrap();
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn logout_refuses_to_interrupt_an_active_listing_batch() {
        let path = tmp_path("logout-busy");
        fs::write(&path, b"encrypted login placeholder").unwrap();
        let s = session_with(path.clone());
        s.debug_set_unlocked(dummy_unlocked());
        let _plan = s.begin_plan().expect("listing batch should start");

        let error = s.logout().unwrap_err();

        assert_eq!(error.code, "busy");
        assert!(s.is_unlocked());
        assert!(path.exists());
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn login_rejects_short_passphrase_before_any_network() {
        // wfm_login runs validate_login before opening the sign-in window, so
        // the user is not sent to WFM with a passphrase that will be refused.
        let err = WfmSession::validate_login("short", "pc").unwrap_err();
        assert_eq!(err.code, "internal");
        assert!(err.message.contains("12 characters"));

        let path = tmp_path("login-short");
        let s = session_with(path.clone());
        let generation = s.session_generation();
        let err = s
            .login(generation, "header.payload.sig".into(), "short", "pc", false)
            .unwrap_err();
        assert!(err.message.contains("12 characters"));
        assert!(!path.exists());
    }

    #[test]
    fn login_rejects_unknown_platform_before_any_network() {
        assert!(WfmSession::validate_login("a-long-enough-passphrase", "playstation").is_err());
        let path = tmp_path("login-plat");
        let s = session_with(path);
        let generation = s.session_generation();
        let err = s
            .login(
                generation,
                "header.payload.sig".into(),
                "a-long-enough-passphrase",
                "playstation",
                false,
            )
            .unwrap_err();
        assert_eq!(err.code, "internal");
    }

    #[test]
    fn early_cancellation_is_retained_and_does_not_poison_explicit_resume() {
        let session = session_with(tmp_path("cancel"));
        session.cancel_plan("first").unwrap();
        let first = session.claim_plan_request("first".into()).unwrap();
        let running = session.begin_plan().unwrap();
        assert!(matches!(wfm_client::governor::process().check(wfm_client::governor::Kind::Mutation, &first.context()), Err(wfm_client::governor::AccessError::Cancelled)));
        assert!(session.claim_plan_request("first".into()).is_err());
        drop(running); drop(first);
        session.cancel_plan("first").unwrap();
        assert!(guard(&session.plan_requests).pending.is_empty());
        let resumed = session.claim_plan_request("resume".into()).unwrap();
        let _running = session.begin_plan().unwrap();
        assert!(wfm_client::governor::process().check(wfm_client::governor::Kind::Mutation, &resumed.context()).is_ok());
    }

    #[test]
    fn begin_plan_serializes_and_guard_releases_on_drop() {
        let s = session_with(tmp_path("busy"));
        let guard = s.begin_plan().expect("first plan starts");
        // A concurrent plan while one is running → busy (caller maps to the
        // `busy` code).
        assert!(s.begin_plan().is_none());
        drop(guard);
        // Guard drop (incl. early return / panic paths) releases the flag.
        assert!(s.begin_plan().is_some());
    }

    #[test]
    fn debug_write_login_roundtrips_through_real_decrypt() {
        let path = tmp_path("probe-login");
        let _ = fs::remove_file(&path);
        let s = session_with(path.clone());
        s.debug_write_login("probe-pass-123456").unwrap();
        assert_eq!(s.auth_status(), (true, false));
        // Wrong passphrase against the probe envelope is the same code path the
        // probe drives through the unlock dialog.
        assert_eq!(
            s.decrypt_from_disk("wrong-pass").unwrap_err().code,
            "bad_passphrase"
        );
        let (jwt, platform, _key) = s.decrypt_from_disk("probe-pass-123456").unwrap();
        assert_eq!(jwt, "probe.jwt.value");
        assert_eq!(platform, "pc");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn try_silent_unlock_is_inert_when_keyring_is_disabled() {
        // The probe/test seam (use_keyring: false) must short-circuit BEFORE
        // any keyring access, even with a perfectly good login file on disk -
        // a hermetic run must never unlock via the developer's real keyring.
        let path = tmp_path("silent-gated");
        let blob = encrypt_jwt("jwt.secret.value", "correct horse battery", "pc").unwrap();
        fs::write(&path, serde_json::to_vec(&blob).unwrap()).unwrap();
        let s = session_with(path.clone());
        assert!(!s.try_silent_unlock());
        assert!(!s.is_unlocked());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn try_silent_unlock_reports_true_when_already_unlocked() {
        let s = session_with(tmp_path("silent-already"));
        s.debug_set_unlocked(dummy_unlocked());
        assert!(s.try_silent_unlock());
    }

    #[test]
    fn auth_status_reflects_file_and_unlock_state() {
        let path = tmp_path("auth-status");
        let _ = fs::remove_file(&path);
        let s = session_with(path.clone());
        assert_eq!(s.auth_status(), (false, false));
        fs::write(
            &path,
            serde_json::to_vec(&encrypt_jwt("j", "correct horse battery", "pc").unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(s.auth_status(), (true, false));
        s.debug_set_unlocked(dummy_unlocked());
        assert_eq!(s.auth_status(), (true, true));
        let _ = fs::remove_file(&path);
    }

    /// The hook stands in for the network warm and logs out at exactly the point
    /// where an unlock is authenticated but not yet published, so the interleaving
    /// is deterministic rather than a race. A logout that completed during the
    /// unlock must not be undone by the unlock publishing afterwards.
    #[test]
    fn logout_during_unlock_leaves_no_authorized_session() {
        fn logout_during_warm(
            session: &WfmSession,
            jwt: String,
            platform: String,
        ) -> Result<Unlocked, CmdError> {
            let mut prepared = dummy_unlocked();
            prepared.jwt = jwt;
            prepared.platform = platform;
            session.logout().expect("logout succeeds");
            Ok(prepared)
        }

        let path = tmp_path("logout-during-unlock");
        let mut session = session_with(path.clone());
        session.warm_hook = Some(logout_during_warm);
        session.debug_write_login("probe-pass-123456").expect("login is written");

        let _ = session.unlock("probe-pass-123456", false);

        assert_eq!(
            session.auth_status(),
            (false, false),
            "a logout that completed during the unlock must win"
        );
        assert!(session.require_unlocked().is_err(), "no usable credentials remain");
        let _ = fs::remove_file(&path);
    }

    /// Positive control: the guard must reject only a superseded session, never
    /// every unlock, or the test above would pass for the wrong reason.
    #[test]
    fn unlock_without_a_logout_publishes_the_session() {
        fn successful_warm(
            _session: &WfmSession,
            jwt: String,
            platform: String,
        ) -> Result<Unlocked, CmdError> {
            let mut prepared = dummy_unlocked();
            prepared.jwt = jwt;
            prepared.platform = platform;
            Ok(prepared)
        }

        let path = tmp_path("unlock-without-logout");
        let mut session = session_with(path.clone());
        session.warm_hook = Some(successful_warm);
        session.debug_write_login("probe-pass-123456").expect("login is written");

        let result = session.unlock("probe-pass-123456", false);

        assert!(result.is_ok(), "an uninterrupted unlock still succeeds");
        assert_eq!(session.auth_status(), (true, true));
        assert!(session.require_unlocked().is_ok(), "the session is usable");
        let _ = fs::remove_file(&path);
    }
}

pub const WFM_ACCESS_EVENT: &str = "wfm-access-changed";
#[tauri::command]
pub fn wfm_access_status() -> wfm_client::governor::AccessStatus {
    wfm_client::governor::process().status()
}
pub fn publish_access_changes(app: tauri::AppHandle) {
    use tauri::Emitter;
    let _ = std::thread::Builder::new().name("wfm-access-status".into()).spawn(move || {
        let mut previous = None;
        loop {
            let current = wfm_access_status();
            if previous.as_ref() != Some(&current) {
                let _ = app.emit(WFM_ACCESS_EVENT, &current);
                previous = Some(current);
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
}
