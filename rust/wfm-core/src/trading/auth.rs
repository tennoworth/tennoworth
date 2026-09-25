//! warframe.market auth + JWT-at-rest.
//!
//! Sign-in happens on warframe.market itself, in a desktop webview the user
//! drives. Since 2026-09 Cloudflare puts every warframe.market page behind an
//! interactive bot challenge, so the old scripted flow - scrape the signin
//! page's CSRF meta tag, then POST `/v1/auth/signin` - fails at its first GET
//! and cannot be repaired from an HTTP client. The API host is not challenged:
//! once the user signs in, the site's `JWT` cookie (cookie-style, which the v2
//! endpoints require) is the credential this module stores. The site also sets
//! an anonymous `JWT` cookie before sign-in; [`jwt_is_signed_in`] tells the
//! two apart.
//!
//! At rest the JWT is AES-256-GCM encrypted, key derived via PBKDF2-HMAC-SHA256
//! (600k iterations, OWASP 2023). **The on-disk envelope shape and its default
//! path (`~/.config/wfminv/wfm-jwt.enc`) are a compatibility contract** - do not
//! change field names, the format tag, or the KDF without a migration; existing
//! users' files must keep decrypting.
//!
//! No terminal I/O lives here: the caller reads the passphrase (from a TTY, a
//! pipe, or a desktop dialog) and hands the plaintext to `decrypt_jwt` /
//! `encrypt_jwt`.

use wfm_client::transport::GovernedRequest;
use wfm_client::governor::Kind;
use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use hmac::Hmac;
use pbkdf2::pbkdf2;
use rand::RngCore;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::time::chrono_now_iso;

pub const JWT_FORMAT: &str = "wfminv-jwt-v1";
pub const JWT_KDF_ITERATIONS: u32 = 600_000;

/// The on-disk encrypted-JWT envelope. Field names + shape are a compat
/// contract - see the module docs. Mirrors the web app's encrypted-export
/// format so a single human can reason about both.
#[derive(Serialize, Deserialize)]
pub struct EncryptedJwt {
    pub format: String,
    pub created: String,
    pub platform: String,
    pub kdf: KdfParams,
    pub cipher: CipherParams,
    pub ciphertext: String,
}

#[derive(Serialize, Deserialize)]
pub struct KdfParams {
    pub name: String,
    pub hash: String,
    pub iterations: u32,
    pub salt: String,
}

#[derive(Serialize, Deserialize)]
pub struct CipherParams {
    pub name: String,
    pub iv: String,
}

/// Reject a mistyped platform up front - an unknown value would otherwise be
/// baked into the encrypted JWT and silently authenticate against the wrong
/// (or a non-existent) WFM market on every later session. Thin wrapper over
/// wfm-client's canonical validator, kept anyhow-flavored for this crate's
/// existing callers.
pub fn validate_platform(platform: &str) -> Result<()> {
    wfm_client::validate_platform(platform).map_err(|e| anyhow!(e))
}

/// Minimum passphrase length, in **characters** - the unit the error message
/// promises the user.
pub const MIN_PASSPHRASE_CHARS: usize = 12;

/// The single passphrase-length gate for every entry point that sets one.
///
/// This lives here because it previously did not: the old CLI's `login`
/// counted `passphrase.len()` (bytes) while the desktop dialog counted
/// `chars().count()`, under a comment asserting the two were the same floor.
/// They were not - a 4-character CJK passphrase is 12 bytes, so the CLI
/// accepted what the desktop app rejected. The desktop shell is the only
/// caller now, and both sides share this function, so the floor cannot
/// drift again.
pub fn validate_passphrase(passphrase: &str) -> Result<()> {
    if passphrase.chars().count() < MIN_PASSPHRASE_CHARS {
        bail!(
            "Passphrase must be at least {MIN_PASSPHRASE_CHARS} characters - it guards your multi-month WFM token against offline brute force."
        );
    }
    Ok(())
}

pub fn encrypt_jwt(jwt: &str, passphrase: &str, platform: &str) -> Result<EncryptedJwt> {
    let mut salt = [0u8; 16];
    let mut iv = [0u8; 12];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut iv);

    let mut key_bytes = [0u8; 32];
    pbkdf2::<Hmac<Sha256>>(
        passphrase.as_bytes(),
        &salt,
        JWT_KDF_ITERATIONS,
        &mut key_bytes,
    )
    .map_err(|e| anyhow!("PBKDF2 failed: {e}"))?;

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&iv), jwt.as_bytes())
        .map_err(|e| anyhow!("AES-GCM encrypt failed: {e}"))?;

    Ok(EncryptedJwt {
        format: JWT_FORMAT.into(),
        created: chrono_now_iso(),
        platform: platform.into(),
        kdf: KdfParams {
            name: "PBKDF2".into(),
            hash: "SHA-256".into(),
            iterations: JWT_KDF_ITERATIONS,
            salt: B64.encode(salt),
        },
        cipher: CipherParams {
            name: "AES-GCM".into(),
            iv: B64.encode(iv),
        },
        ciphertext: B64.encode(&ciphertext),
    })
}

/// Run the blob's KDF over `passphrase`, yielding the raw AES-256 key. Split
/// from `decrypt_jwt` so the desktop can hold the derived key in the OS
/// keyring for silent unlock - the key is salt-bound (a re-login rotates the
/// salt, so a stale key fails GCM auth) and useless without the .enc file,
/// unlike the passphrase, which users may reuse elsewhere.
pub fn derive_jwt_key(blob: &EncryptedJwt, passphrase: &str) -> Result<[u8; 32]> {
    let salt = B64.decode(&blob.kdf.salt).context("decoding salt")?;
    let mut key_bytes = [0u8; 32];
    pbkdf2::<Hmac<Sha256>>(
        passphrase.as_bytes(),
        &salt,
        blob.kdf.iterations,
        &mut key_bytes,
    )
    .map_err(|e| anyhow!("PBKDF2 failed: {e}"))?;
    Ok(key_bytes)
}

pub fn decrypt_jwt_with_key(blob: &EncryptedJwt, key_bytes: &[u8; 32]) -> Result<String> {
    if blob.format != JWT_FORMAT {
        bail!("Unknown JWT blob format: {}", blob.format);
    }
    let iv = B64.decode(&blob.cipher.iv).context("decoding IV")?;
    let ciphertext = B64
        .decode(&blob.ciphertext)
        .context("decoding ciphertext")?;

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key_bytes));
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&iv), ciphertext.as_ref())
        .map_err(|_| anyhow!("Wrong passphrase, or the JWT file was modified."))?;
    String::from_utf8(plaintext).context("JWT plaintext was not valid UTF-8")
}

pub fn decrypt_jwt(blob: &EncryptedJwt, passphrase: &str) -> Result<String> {
    if blob.format != JWT_FORMAT {
        bail!("Unknown JWT blob format: {}", blob.format);
    }
    let key_bytes = derive_jwt_key(blob, passphrase)?;
    decrypt_jwt_with_key(blob, &key_bytes)
}

/// Whether `jwt` belongs to a signed-in account. warframe.market issues an
/// anonymous `JWT` cookie to every visitor, so a cookie's presence proves
/// nothing; `/v2/me` answering 401 is the "not signed in yet" answer. Any other
/// failure is an error, so a caller polling during sign-in can retry it instead
/// of discarding a real credential over a network blip.
pub fn jwt_is_signed_in(jwt: &str, platform: &str) -> Result<bool> {
    let client = crate::http::browser_client(30)?;
    let resp = wfm_client::wfm_authed_headers(
        client.get("https://api.warframe.market/v2/me"),
        platform,
        jwt,
    )
    .send_governed(Kind::Read)
    .context("/v2/me request failed")?;
    signed_in_from_status(resp.status())
}

fn signed_in_from_status(status: reqwest::StatusCode) -> Result<bool> {
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Ok(false);
    }
    if !status.is_success() {
        bail!("/v2/me returned {status}");
    }
    Ok(true)
}

/// Resolve the WFM username (`data.slug`) for a decrypted JWT. Used when
/// warming listing credentials.
pub fn fetch_wfm_me(client: &Client, jwt: &str, platform: &str) -> Result<String> {
    let resp = wfm_client::wfm_authed_headers(
        client.get("https://api.warframe.market/v2/me"),
        platform,
        jwt,
    )
    .send_governed(Kind::Read)
    .context("/v2/me request failed")?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().context("parsing /v2/me")?;
    if !status.is_success() {
        bail!("/v2/me returned {status}: {body}");
    }
    body.pointer("/data/slug")
        .or_else(|| body.pointer("/data/ingameName"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("/v2/me response shape unexpected: {body}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_then_decrypt_roundtrips_the_jwt() {
        let blob = encrypt_jwt("jwt.abc.123", "correct horse battery", "pc").unwrap();
        assert_eq!(blob.format, JWT_FORMAT);
        assert_eq!(blob.platform, "pc");
        assert_eq!(blob.kdf.iterations, JWT_KDF_ITERATIONS);
        let jwt = decrypt_jwt(&blob, "correct horse battery").unwrap();
        assert_eq!(jwt, "jwt.abc.123");
    }

    #[test]
    fn only_unauthorized_means_not_signed_in() {
        use reqwest::StatusCode;
        assert!(!signed_in_from_status(StatusCode::UNAUTHORIZED).unwrap());
        assert!(signed_in_from_status(StatusCode::OK).unwrap());
        // A throttled or failing check must not read as "anonymous": the
        // poller would then never look at that cookie again.
        assert!(signed_in_from_status(StatusCode::TOO_MANY_REQUESTS).is_err());
        assert!(signed_in_from_status(StatusCode::FORBIDDEN).is_err());
    }

    #[test]
    fn passphrase_floor_counts_characters_not_bytes() {
        // The regression this function exists to kill: 4 CJK characters are 12
        // UTF-8 bytes, so a byte-counting floor waved them through. `login`
        // counted bytes, the desktop dialog counted chars, and a comment
        // claimed the two floors matched.
        let four_cjk = "密码密码";
        assert_eq!(four_cjk.len(), 12);
        assert_eq!(four_cjk.chars().count(), 4);
        assert!(validate_passphrase(four_cjk).is_err());

        assert!(validate_passphrase("hunter2").is_err());
        assert!(validate_passphrase("correct horse battery").is_ok());
        // Exactly at the floor, in a script where chars != bytes.
        assert!(validate_passphrase("密码密码密码密码密码密码").is_ok());
    }

    #[test]
    fn decrypt_with_wrong_passphrase_fails() {
        let blob = encrypt_jwt("jwt.abc.123", "correct horse battery", "pc").unwrap();
        assert!(decrypt_jwt(&blob, "wrong passphrase!!").is_err());
    }

    #[test]
    fn derived_key_decrypts_without_the_passphrase() {
        let blob = encrypt_jwt("jwt.abc.123", "correct horse battery", "pc").unwrap();
        let key = derive_jwt_key(&blob, "correct horse battery").unwrap();
        assert_eq!(decrypt_jwt_with_key(&blob, &key).unwrap(), "jwt.abc.123");
    }

    #[test]
    fn derived_key_is_salt_bound_so_a_relogin_invalidates_it() {
        // Same passphrase, fresh envelope → fresh salt → the old derived key
        // must fail GCM auth (this is what makes a stale keyring entry
        // detectable instead of silently decrypting a rotated login).
        let old = encrypt_jwt("jwt.abc.123", "correct horse battery", "pc").unwrap();
        let old_key = derive_jwt_key(&old, "correct horse battery").unwrap();
        let new = encrypt_jwt("jwt.def.456", "correct horse battery", "pc").unwrap();
        assert!(decrypt_jwt_with_key(&new, &old_key).is_err());
    }

    #[test]
    fn validate_platform_accepts_known_rejects_unknown() {
        assert!(validate_platform("pc").is_ok());
        assert!(validate_platform("switch").is_ok());
        assert!(validate_platform("PC").is_err());
        assert!(validate_platform("playstation").is_err());
    }

    // Parity gate: frontend/src/lib/crypto.ts's KDF_ITERATIONS must match this
    // crate's JWT_KDF_ITERATIONS exactly - a mismatch bricks JWT decryption
    // with a false "wrong passphrase" error. Both sides read
    // tests/fixtures/jwt-kdf.json.
    #[test]
    fn jwt_kdf_iterations_matches_the_shared_fixture() {
        #[derive(serde::Deserialize)]
        struct Fixture {
            iterations: u32,
        }
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/jwt-kdf.json"
        );
        let raw = std::fs::read_to_string(path).expect("read the shared KDF fixture");
        let fx: Fixture = serde_json::from_str(&raw).expect("parse the KDF fixture");
        assert_eq!(JWT_KDF_ITERATIONS, fx.iterations);
    }
}
