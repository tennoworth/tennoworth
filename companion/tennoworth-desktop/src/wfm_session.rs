//! Desktop warframe.market session — the in-memory decrypted-JWT credential
//! lifecycle, mirroring serve's `ServeState` listing auth minus the terminal.
//!
//! The passphrase arrives from the webview (the `unlock_jwt` / `wfm_login`
//! commands) instead of a TTY prompt; wfm-core takes it as a parameter, exactly
//! as designed for this second adapter. The plaintext JWT lives ONLY inside this
//! process's memory for the session — never on disk (only the AES-GCM envelope,
//! whose format is unchanged), never in a log line, and never in a value handed
//! back to the SPA. `CmdError` carries a `code` + a human message and nothing
//! else; the raw password and the JWT never appear in it.
//!
//! Unlock is lazy and terminal-free: a listing command with no unlocked session
//! does NOT try to prompt (there is nowhere to prompt). It returns a typed
//! `needs_login` (no login file on this machine) or `needs_unlock` (login file
//! present, session locked) so the SPA can raise the login or passphrase modal —
//! the desktop analogue of serve's 401 `needs_login:true` vs 503 split.

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use wfm_core::auth::{
    bootstrap_session, decrypt_jwt_with_key, derive_jwt_key, encrypt_jwt, fetch_wfm_me, signin,
    validate_platform,
    EncryptedJwt,
};
use wfm_core::listing::{fetch_wfm_catalog, Unlocked};
use wfm_core::platform::{chown_to_real_user, restrict_dir_perms, write_restricted};
use wfm_core::util::{browser_client, default_jwt_path, default_pending_path};
use zeroize::Zeroize;

/// Typed command error serialized to the webview as `{ code, message }`. The SPA
/// maps `code` to its own error classes:
///   - `needs_login`   — no login on this machine → open the login modal.
///   - `needs_unlock`  — login present, session locked → open the passphrase modal.
///   - `bad_passphrase`— wrong passphrase in the unlock/login modal.
///   - `no_api_key` / `upstream` / `rate_limited` / `too_large` — the assistant relay.
///   - `no_pending` / `busy` — pending-plan resume edge cases.
///   - `wfm` / `internal` — everything else, message shown verbatim.
///
/// Never carries the JWT, the passphrase, or the WFM password.
#[derive(Debug, serde::Serialize)]
pub struct CmdError {
    pub code: &'static str,
    pub message: String,
}

impl CmdError {
    pub fn of(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
    pub fn needs_login() -> Self {
        Self::of("needs_login", "Log in to warframe.market to create or edit listings.")
    }
    pub fn needs_unlock() -> Self {
        Self::of("needs_unlock", "Enter your passphrase to unlock warframe.market listing.")
    }
    pub fn bad_passphrase() -> Self {
        Self::of("bad_passphrase", "Wrong passphrase, or the login file was modified.")
    }
    pub fn wfm(e: impl std::fmt::Display) -> Self {
        Self::of("wfm", e.to_string())
    }
    pub fn internal(e: impl std::fmt::Display) -> Self {
        Self::of("internal", e.to_string())
    }
}

/// Clears the plan-in-flight flag on scope exit (incl. early return / panic) so
/// a rejected or crashed listing command can't leave the session wedged.
/// Mirrors serve's `PlanGuard`.
pub struct PlanGuard<'a>(&'a AtomicBool);
impl Drop for PlanGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// The desktop WFM credential session. One instance is managed by Tauri; every
/// listing command borrows it via `State`.
pub struct WfmSession {
    /// Encrypted-JWT path. `TENNOWORTH_JWT_PATH` overrides it (a test/probe seam
    /// so a hermetic run controls whether a login file "exists").
    jwt_path: PathBuf,
    /// Pending-plan path. `TENNOWORTH_PENDING_PATH` overrides it so a probe
    /// doesn't touch the real `~/.config/wfminv/pending_plan.json`.
    pending_path: PathBuf,
    /// Directory the DeepSeek key file (`deepseek-key`) is read from — the JWT's
    /// own config dir, resolved once (mirrors serve's `deepseek_key_dir`).
    key_dir: PathBuf,
    /// The unlocked credentials, or `None` when locked/unavailable. The plaintext
    /// JWT lives ONLY inside this `Arc<Unlocked>` for the session's lifetime.
    inner: Mutex<Option<Arc<Unlocked>>>,
    /// Serializes plan execution: a second concurrent `execute_plan` /
    /// `resume_pending_plan` gets `busy` instead of racing on the pending file.
    plan_running: AtomicBool,
    /// Sliding-window timestamps of recent assistant calls — same budget as
    /// serve's `ServeState.assistant_calls` (≤ 20 DeepSeek calls / 60 s).
    pub assistant_calls: Mutex<VecDeque<Instant>>,
    /// "Remember on this device" is only offered against the REAL login file:
    /// any `TENNOWORTH_JWT_PATH` override (the probe/test seam) turns the OS
    /// keyring off entirely, so hermetic runs can never pollute — or unlock
    /// via — the user's actual keyring entry.
    use_keyring: bool,
}

impl WfmSession {
    pub fn new() -> Self {
        let overridden = std::env::var_os("TENNOWORTH_JWT_PATH");
        let use_keyring = overridden.is_none();
        let jwt_path = overridden
            .map(PathBuf::from)
            .unwrap_or_else(default_jwt_path);
        let pending_path = std::env::var_os("TENNOWORTH_PENDING_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(default_pending_path);
        let key_dir = jwt_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            jwt_path,
            pending_path,
            key_dir,
            inner: Mutex::new(None),
            plan_running: AtomicBool::new(false),
            assistant_calls: Mutex::new(VecDeque::new()),
            use_keyring,
        }
    }

    pub fn pending_path(&self) -> &Path {
        &self.pending_path
    }

    pub fn key_dir(&self) -> &Path {
        &self.key_dir
    }

    pub fn is_unlocked(&self) -> bool {
        self.inner.lock().expect("session mutex poisoned").is_some()
    }

    /// `(logged_in, unlocked)` for the desktop UI's login affordance:
    /// `logged_in` = a login file exists on disk; `unlocked` = this session
    /// holds the decrypted JWT.
    pub fn auth_status(&self) -> (bool, bool) {
        (self.jwt_path.exists(), self.is_unlocked())
    }

    /// Lock the session and scrub the in-memory JWT. Does NOT delete the on-disk
    /// login — the user can re-unlock with their passphrase. Best-effort scrub:
    /// if a listing call is in flight it holds a clone of the Arc, so we can't be
    /// the sole owner; dropping still frees the plaintext, just without an
    /// explicit overwrite first. Also forgets the remembered device key —
    /// an explicit logout that silently re-unlocked itself wouldn't be one.
    pub fn logout(&self) {
        let mut guard = self.inner.lock().expect("session mutex poisoned");
        if let Some(arc) = guard.take() {
            if let Ok(mut unlocked) = Arc::try_unwrap(arc) {
                unlocked.jwt.zeroize();
            }
        }
        if self.use_keyring {
            crate::keyring_store::forget_key();
        }
    }

    /// The unlocked credentials, or a typed error WITHOUT attempting an unlock —
    /// there is no passphrase at a listing call site (no terminal), so the SPA
    /// must drive `unlock_jwt` first. `needs_login` vs `needs_unlock` is decided
    /// by whether a login file exists (serve's `NeedsLogin` vs a present-but-
    /// locked blob).
    pub fn require_unlocked(&self) -> Result<Arc<Unlocked>, CmdError> {
        if let Some(u) = self.inner.lock().expect("session mutex poisoned").as_ref() {
            return Ok(Arc::clone(u));
        }
        if self.jwt_path.exists() {
            Err(CmdError::needs_unlock())
        } else {
            Err(CmdError::needs_login())
        }
    }

    pub fn begin_plan(&self) -> Option<PlanGuard<'_>> {
        match self
            .plan_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(_) => Some(PlanGuard(&self.plan_running)),
            Err(_) => None,
        }
    }

    /// Read + parse the on-disk envelope — shared by the passphrase and
    /// silent-unlock paths, with the same error mapping (missing file →
    /// `needs_login`).
    fn read_blob(&self) -> Result<EncryptedJwt, CmdError> {
        let bytes = match fs::read(&self.jwt_path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(CmdError::needs_login()),
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

    /// Read + decrypt the on-disk JWT with `passphrase` — the offline half of
    /// `unlock`. Returns `(jwt_plaintext, platform, derived_key)`. Split out so
    /// the error mapping (missing file → `needs_login`, wrong passphrase →
    /// `bad_passphrase`) is unit-testable without the network catalog warm.
    fn decrypt_from_disk(&self, passphrase: &str) -> Result<(String, String, [u8; 32]), CmdError> {
        let blob = self.read_blob()?;
        let platform = blob.platform.clone();
        // Any decrypt failure (wrong key or tampered ciphertext) reads as a bad
        // passphrase — the only actionable cause from the user's side.
        let key = derive_jwt_key(&blob, passphrase).map_err(|_| CmdError::bad_passphrase())?;
        let jwt = decrypt_jwt_with_key(&blob, &key).map_err(|_| CmdError::bad_passphrase())?;
        Ok((jwt, platform, key))
    }

    /// Decrypt the on-disk JWT and warm the WFM catalog, populating the session.
    /// Network: `/v2/items` + `/v2/me`. On success the plaintext JWT is held only
    /// inside the session `Arc`; with `remember`, the salt-bound derived key
    /// (never the passphrase) also goes to the OS keyring for silent unlock.
    /// Remember only on FULL success — an unlock the user abandons after a
    /// network failure should leave no trace.
    pub fn unlock(&self, passphrase: &str, remember: bool) -> Result<(), CmdError> {
        let (jwt, platform, key) = self.decrypt_from_disk(passphrase)?;
        let unlocked = warm(jwt, platform)?;
        *self.inner.lock().expect("session mutex poisoned") = Some(Arc::new(unlocked));
        if self.use_keyring {
            if remember {
                crate::keyring_store::store_key(&key);
            } else {
                // Unticking the box is an explicit "stop remembering".
                crate::keyring_store::forget_key();
            }
        }
        Ok(())
    }

    /// Try the OS-keyring key against the current login file — the silent
    /// analogue of `unlock`, called by the SPA before it raises the passphrase
    /// modal. Never fails the caller: every miss (no entry, no daemon, network
    /// warm failure) is `Ok(false)` → the modal opens as before. The entry is
    /// deleted ONLY on a definitive GCM auth failure (stale after a re-login),
    /// never on transient store errors.
    pub fn try_silent_unlock(&self) -> bool {
        if self.is_unlocked() {
            return true;
        }
        if !self.use_keyring || !self.jwt_path.exists() {
            return false;
        }
        let Some(key) = crate::keyring_store::load_key() else {
            return false;
        };
        let Ok(blob) = self.read_blob() else {
            return false;
        };
        let platform = blob.platform.clone();
        let jwt = match decrypt_jwt_with_key(&blob, &key) {
            Ok(jwt) => jwt,
            Err(_) => {
                crate::keyring_store::forget_key();
                return false;
            }
        };
        match warm(jwt, platform) {
            Ok(unlocked) => {
                *self.inner.lock().expect("session mutex poisoned") = Some(Arc::new(unlocked));
                true
            }
            Err(e) => {
                // Key is good; the warm (network) failed. Keep the entry and
                // let the passphrase modal surface the error on retry.
                eprintln!("tennoworth: silent unlock warm failed: {}", e.message);
                false
            }
        }
    }

    /// Sign in to warframe.market, persist the encrypted JWT (unchanged on-disk
    /// format), and populate the session with the fresh JWT — so the first
    /// listing action doesn't re-prompt for the passphrase the user just set. The
    /// raw password is used only for the signin POST; the passphrase only
    /// encrypts. Neither is retained here.
    pub fn login(
        &self,
        email: &str,
        password: &str,
        passphrase: &str,
        platform: &str,
        remember: bool,
    ) -> Result<(), CmdError> {
        validate_platform(platform).map_err(CmdError::internal)?;
        if email.trim().is_empty() {
            return Err(CmdError::internal("Email cannot be empty."));
        }
        if password.is_empty() {
            return Err(CmdError::internal("Password cannot be empty."));
        }
        // Same floor as the CLI `login` — the passphrase guards a multi-month
        // bearer token against offline brute force.
        if passphrase.chars().count() < 12 {
            return Err(CmdError::internal(
                "Passphrase must be at least 12 characters — it guards your multi-month WFM token against offline brute force.",
            ));
        }

        let (client, csrf) = bootstrap_session().map_err(CmdError::wfm)?;
        let jwt = signin(&client, email, password, platform, &csrf).map_err(CmdError::wfm)?;

        let encrypted = encrypt_jwt(&jwt, passphrase, platform).map_err(CmdError::internal)?;
        self.persist(&encrypted)?;

        // Warm the session with the in-hand JWT (no redundant decrypt). If the
        // catalog warm fails the JWT is already saved, so a later listing action
        // unlocks via the passphrase modal — surface the network error either way.
        let unlocked = warm(jwt, platform.to_string())?;
        *self.inner.lock().expect("session mutex poisoned") = Some(Arc::new(unlocked));
        if self.use_keyring {
            if remember {
                // A fresh login rotated the salt, so derive against the blob we
                // just persisted — any older keyring entry is overwritten.
                match derive_jwt_key(&encrypted, passphrase) {
                    Ok(key) => crate::keyring_store::store_key(&key),
                    Err(e) => eprintln!("tennoworth: deriving remember-key failed: {e}"),
                }
            } else {
                crate::keyring_store::forget_key();
            }
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
        *self.inner.lock().expect("session mutex poisoned") = Some(Arc::new(unlocked));
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
/// the only network in the session module — everything above is offline.
fn warm(jwt: String, platform: String) -> Result<Unlocked, CmdError> {
    let http = browser_client(60).map_err(CmdError::internal)?;
    let catalog = fetch_wfm_catalog(&http, &platform).map_err(CmdError::wfm)?;
    let id_to_name: BTreeMap<String, String> = catalog
        .values()
        .map(|c| (c.item_id.clone(), c.display_name.clone()))
        .collect();
    let username = fetch_wfm_me(&http, &jwt, &platform).map_err(CmdError::wfm)?;
    Ok(Unlocked {
        jwt,
        username,
        platform,
        catalog: Arc::new(catalog),
        id_to_name: Arc::new(id_to_name),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

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
        let key_dir = jwt_path.parent().map(Path::to_path_buf).unwrap();
        WfmSession {
            jwt_path,
            pending_path: pending,
            key_dir,
            inner: Mutex::new(None),
            plan_running: AtomicBool::new(false),
            assistant_calls: Mutex::new(VecDeque::new()),
            // Tests must never read or write the developer's real OS keyring.
            use_keyring: false,
        }
    }

    fn dummy_unlocked() -> Unlocked {
        Unlocked {
            jwt: "jwt.header.body.sig".into(),
            username: "tester".into(),
            platform: "pc".into(),
            catalog: Arc::new(BTreeMap::new()),
            id_to_name: Arc::new(BTreeMap::new()),
        }
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
        fs::write(&path, serde_json::to_vec(&encrypt_jwt("j", "correct horse battery", "pc").unwrap()).unwrap()).unwrap();
        let s = session_with(path.clone());
        let err = s.require_unlocked().err().expect("expected a typed error");
        assert_eq!(err.code, "needs_unlock");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn require_unlocked_returns_creds_when_session_is_unlocked() {
        // Unlocked takes priority over the on-disk check — even with no file.
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
        // The derived key it hands back must actually open the same envelope —
        // that key is what "remember on this device" stores.
        let blob: EncryptedJwt =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(decrypt_jwt_with_key(&blob, &key).unwrap(), "jwt.secret.value");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn decrypt_from_disk_corrupt_file_is_internal_not_bad_passphrase() {
        // A present-but-garbage file must not read as "wrong passphrase" — that
        // would send the user in circles retyping a correct passphrase.
        let path = tmp_path("decrypt-corrupt");
        fs::write(&path, b"{not valid json at all").unwrap();
        let s = session_with(path.clone());
        let err = s.decrypt_from_disk("anything").unwrap_err();
        assert_eq!(err.code, "internal");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn logout_locks_the_session() {
        let path = tmp_path("logout");
        let _ = fs::remove_file(&path);
        let s = session_with(path);
        s.debug_set_unlocked(dummy_unlocked());
        assert!(s.is_unlocked());
        s.logout();
        assert!(!s.is_unlocked());
        // After logout with no file on disk, listing steers back to login.
        assert_eq!(s.require_unlocked().err().expect("expected a typed error").code, "needs_login");
    }

    #[test]
    fn login_rejects_short_passphrase_before_any_network() {
        // The 12-char floor is checked before bootstrap_session, so this never
        // touches WFM.
        let path = tmp_path("login-short");
        let s = session_with(path);
        let err = s.login("me@example.com", "hunter2hunter2", "short", "pc", false).unwrap_err();
        assert_eq!(err.code, "internal");
        assert!(err.message.contains("12 characters"));
    }

    #[test]
    fn login_rejects_unknown_platform_before_any_network() {
        let path = tmp_path("login-plat");
        let s = session_with(path);
        let err = s
            .login("me@example.com", "pw", "a-long-enough-passphrase", "playstation", false)
            .unwrap_err();
        assert_eq!(err.code, "internal");
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
        // any keyring access, even with a perfectly good login file on disk —
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
        fs::write(&path, serde_json::to_vec(&encrypt_jwt("j", "correct horse battery", "pc").unwrap()).unwrap()).unwrap();
        assert_eq!(s.auth_status(), (true, false));
        s.debug_set_unlocked(dummy_unlocked());
        assert_eq!(s.auth_status(), (true, true));
        let _ = fs::remove_file(&path);
    }
}
