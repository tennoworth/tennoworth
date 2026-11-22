//! Where the device keeps the key to its warframe.market sign-in file: the OS
//! keyring (Secret Service / Windows Credential Manager), or, where no keyring
//! answers (Steam Deck Gaming Mode starts none), a file beside the sign-in file
//! that only the user can read.
//!
//! The key is random per sign-in and useless without the sign-in file. The file
//! fallback protects the token the way browsers without a keyring protect their
//! cookies: by the user account's file permissions, so anything running as the
//! user can read it. The keyring adds the login session's own lock on top.
//!
//! A keyring entry from the passphrase era holds that era's PBKDF2-derived key,
//! which opens the old file the same way; the session rewrites such a file
//! under a fresh key the first time it opens it.
//!
//! Failures never carry the key itself.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use keyring::Entry;
use std::path::Path;

const SERVICE: &str = "tennoworth";
const ACCOUNT: &str = "wfm-jwt-key";

/// KDE Plasma ships `ksecretd` DBus-activatable ONLY as
/// `org.kde.secretservicecompat` - no activation file claims
/// `org.freedesktop.secrets` (verified on CachyOS/Plasma, 2026-07) - so the
/// first Secret Service call in a session fails with "name is not
/// activatable" unless the daemon happens to be up. A blocking ping on the
/// compat name activates it; on GNOME (where gnome-keyring owns the name)
/// the dest doesn't exist and this fails harmlessly.
#[cfg(target_os = "linux")]
fn nudge_kde_secret_service() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = std::process::Command::new("dbus-send")
            .args([
                "--session",
                "--print-reply",
                "--reply-timeout=2000",
                "--dest=org.kde.secretservicecompat",
                "/org/freedesktop/secrets",
                "org.freedesktop.DBus.Peer.Ping",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    });
}

fn entry() -> Result<Entry, keyring::Error> {
    #[cfg(target_os = "linux")]
    nudge_kde_secret_service();
    Entry::new(SERVICE, ACCOUNT)
}

/// Store the key in the keyring. The caller falls back to the key file when
/// this fails.
pub fn store_key(key: &[u8; 32]) -> Result<(), String> {
    let encoded = B64.encode(key);
    entry()
        .and_then(|e| e.set_password(&encoded))
        .map_err(|e| e.to_string())
}

/// The keyring's key: `Ok(None)` when it has no entry (or one that is not a
/// key), `Err` when the keyring did not answer - a locked wallet or no daemon,
/// which may pass, so the caller must not treat it as "no key".
pub fn load_key() -> Result<Option<[u8; 32]>, String> {
    match entry().and_then(|e| e.get_password()) {
        Ok(encoded) => Ok(decode_key(&encoded)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn decode_key(encoded: &str) -> Option<[u8; 32]> {
    let bytes = B64.decode(encoded.trim()).ok()?;
    <[u8; 32]>::try_from(bytes.as_slice()).ok()
}

/// The key file's key, `Ok(None)` when there is no file. An unreadable file is
/// an error rather than "no key", for the same reason as [`load_key`].
pub fn read_key_file(path: &Path) -> Result<Option<[u8; 32]>, String> {
    match std::fs::read_to_string(path) {
        Ok(encoded) => Ok(decode_key(&encoded)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("reading {}: {e}", path.display())),
    }
}

/// Write the key file, readable by the user only.
pub fn write_key_file(path: &Path, key: &[u8; 32]) -> Result<(), String> {
    wfm_trading::platform::write_restricted(path, B64.encode(key).as_bytes())
        .map_err(|e| format!("writing {}: {e}", path.display()))?;
    wfm_trading::platform::chown_to_real_user(path);
    Ok(())
}

pub fn remove_key_file(path: &Path) {
    if let Err(e) = std::fs::remove_file(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            eprintln!("tennoworth: removing the sign-in key file failed: {e}");
        }
    }
}

/// Drop the keyring entry. Called on logout, when the key moves to the file,
/// and when no saved key opens the sign-in file any more.
pub fn forget_key() {
    match entry().and_then(|e| e.delete_credential()) {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(e) => eprintln!("tennoworth: keyring delete failed: {e}"),
    }
}
