//! "Remember on this device" — the PBKDF2-derived JWT unlock key in the OS
//! keyring (Secret Service / Windows Credential Manager).
//!
//! Deliberately NOT the passphrase: the derived key is salt-bound to the
//! current wfm-jwt.enc (a re-login rotates the salt, so a stale entry fails
//! GCM auth and is detectable), it is useless without the file, and unlike a
//! human passphrase it cannot have been reused on other sites.
//!
//! Every operation is best-effort: no keyring daemon, a locked wallet, or a
//! DBus hiccup must never break the passphrase-modal path. Failures are
//! logged to stderr (never the secret itself) and read as "no entry".

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use keyring::Entry;

const SERVICE: &str = "tennoworth";
const ACCOUNT: &str = "wfm-jwt-key";

/// KDE Plasma ships `ksecretd` DBus-activatable ONLY as
/// `org.kde.secretservicecompat` — no activation file claims
/// `org.freedesktop.secrets` (verified on CachyOS/Plasma, 2026-07) — so the
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

/// Store the derived key. Best-effort — a failure only costs the user a
/// passphrase prompt next launch.
pub fn store_key(key: &[u8; 32]) {
    let encoded = B64.encode(key);
    match entry().and_then(|e| e.set_password(&encoded)) {
        Ok(()) => {}
        Err(e) => eprintln!("tennoworth: keyring store failed (remember-on-device off): {e}"),
    }
}

/// Fetch the stored key, or None. Transient store errors also read as None —
/// the caller falls back to the passphrase modal and the entry is kept.
pub fn load_key() -> Option<[u8; 32]> {
    let encoded = match entry().and_then(|e| e.get_password()) {
        Ok(s) => s,
        Err(keyring::Error::NoEntry) => return None,
        Err(e) => {
            eprintln!("tennoworth: keyring read failed (falling back to passphrase): {e}");
            return None;
        }
    };
    let bytes = B64.decode(encoded.trim()).ok()?;
    <[u8; 32]>::try_from(bytes.as_slice()).ok()
}

/// Drop the entry. Called on logout and when a stored key fails GCM auth
/// against the current login file (stale after a re-login).
pub fn forget_key() {
    match entry().and_then(|e| e.delete_credential()) {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(e) => eprintln!("tennoworth: keyring delete failed: {e}"),
    }
}
