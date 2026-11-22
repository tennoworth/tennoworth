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
//! At rest the JWT is AES-256-GCM encrypted under a random per-sign-in key that
//! the caller keeps (the desktop: the OS keyring, else a file only the user can
//! read). There is no passphrase: one guarded a token whose site cookie already
//! sits unencrypted in the user's browser, and it made sign-in unusable on a
//! controller. **The on-disk envelope shape and its default path
//! (`~/.config/wfminv/wfm-jwt.enc`) are a compatibility contract** - do not
//! change field names or format tags without a migration; files already on
//! users' machines must keep decrypting.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rand::{rngs::SysRng, TryRng};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use wfm_client::governor::Kind;
use wfm_client::transport::GovernedRequest;

use crate::time::chrono_now_iso;

/// The current envelope: the JWT under a random key the device keeps.
pub const JWT_FORMAT: &str = "wfminv-jwt-v2";
/// The passphrase-era envelope: the key was PBKDF2-derived from a passphrase,
/// and the only copy the app can still use is the derived key "remember on
/// this device" put in the keyring. It decrypts with that key exactly as a
/// current file does; the desktop rewrites it as [`JWT_FORMAT`] once opened.
pub const JWT_FORMAT_V1: &str = "wfminv-jwt-v1";
/// AES-GCM's nonce size. `Nonce::from_slice` panics on any other length, and
/// release builds abort on panic, so a damaged file must be refused first.
const JWT_IV_BYTES: usize = 12;

/// The on-disk encrypted-JWT envelope. Field names + shape are a compat
/// contract - see the module docs. A v1 file also carries a `kdf` block,
/// which nothing reads any more; serde skips it.
#[derive(Serialize, Deserialize)]
pub struct EncryptedJwt {
    pub format: String,
    pub created: String,
    pub platform: String,
    pub cipher: CipherParams,
    pub ciphertext: String,
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

fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0u8; N];
    SysRng
        .try_fill_bytes(&mut bytes)
        .map_err(|e| anyhow!("system randomness unavailable: {e}"))?;
    Ok(bytes)
}

/// A fresh AES-256 key for one sign-in. Every sign-in gets its own, so a key
/// left behind by an earlier one cannot open the current file.
pub fn new_jwt_key() -> Result<[u8; 32]> {
    random_bytes()
}

pub fn encrypt_jwt(jwt: &str, key: &[u8; 32], platform: &str) -> Result<EncryptedJwt> {
    let iv: [u8; JWT_IV_BYTES] = random_bytes()?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&iv), jwt.as_bytes())
        .map_err(|e| anyhow!("AES-GCM encrypt failed: {e}"))?;
    Ok(EncryptedJwt {
        format: JWT_FORMAT.into(),
        created: chrono_now_iso(),
        platform: platform.into(),
        cipher: CipherParams {
            name: "AES-GCM".into(),
            iv: B64.encode(iv),
        },
        ciphertext: B64.encode(&ciphertext),
    })
}

pub fn decrypt_jwt_with_key(blob: &EncryptedJwt, key_bytes: &[u8; 32]) -> Result<String> {
    if blob.format != JWT_FORMAT && blob.format != JWT_FORMAT_V1 {
        bail!("Unknown JWT blob format: {}", blob.format);
    }
    let iv = B64.decode(&blob.cipher.iv).context("decoding IV")?;
    if iv.len() != JWT_IV_BYTES {
        bail!(
            "The saved WFM sign-in file is damaged (IV length {}).",
            iv.len()
        );
    }
    let ciphertext = B64
        .decode(&blob.ciphertext)
        .context("decoding ciphertext")?;

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key_bytes));
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&iv), ciphertext.as_ref())
        .map_err(|_| anyhow!("The saved key does not open this sign-in file."))?;
    String::from_utf8(plaintext).context("JWT plaintext was not valid UTF-8")
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
    let body = resp.text().context("reading /v2/me")?;
    username_from_me(status, &body)
}

/// The username from a `/v2/me` answer. The status decides first: an error
/// page (Cloudflare serves HTML) is a failed request, not a JSON parse
/// failure. The body is never echoed into the error - on success it is the
/// user's profile, and errors reach logs.
fn username_from_me(status: reqwest::StatusCode, body: &str) -> Result<String> {
    if !status.is_success() {
        bail!("/v2/me returned {status}");
    }
    let body: serde_json::Value = serde_json::from_str(body).context("parsing /v2/me")?;
    body.pointer("/data/slug")
        .or_else(|| body.pointer("/data/ingameName"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("/v2/me response has no data.slug or data.ingameName"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [7; 32];

    #[test]
    fn a_damaged_envelope_is_refused_instead_of_aborting() {
        for iv in ["", "AAAA", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"] {
            let mut blob = encrypt_jwt("header.payload.sig", &KEY, "pc").unwrap();
            blob.cipher.iv = iv.into();
            let err = decrypt_jwt_with_key(&blob, &KEY).unwrap_err().to_string();
            assert!(err.contains("damaged"), "{iv:?}: {err}");
        }
    }

    #[test]
    fn a_me_error_page_reports_its_status_not_a_parse_failure() {
        let html = "<html><title>Just a moment...</title></html>";
        let err = username_from_me(reqwest::StatusCode::SERVICE_UNAVAILABLE, html).unwrap_err();
        assert_eq!(err.to_string(), "/v2/me returned 503 Service Unavailable");
    }

    #[test]
    fn a_me_answer_yields_the_slug_and_never_echoes_the_profile() {
        let ok = r#"{"data":{"slug":"tenno_one","ingameName":"Tenno One"}}"#;
        assert_eq!(
            username_from_me(reqwest::StatusCode::OK, ok).unwrap(),
            "tenno_one"
        );
        let named = r#"{"data":{"ingameName":"Tenno One"}}"#;
        assert_eq!(
            username_from_me(reqwest::StatusCode::OK, named).unwrap(),
            "Tenno One"
        );
        let odd = r#"{"data":{"email":"someone@example.invalid"}}"#;
        let err = username_from_me(reqwest::StatusCode::OK, odd).unwrap_err();
        assert!(!err.to_string().contains("example.invalid"), "{err}");
    }

    #[test]
    fn encrypt_then_decrypt_roundtrips_the_jwt() {
        let blob = encrypt_jwt("jwt.abc.123", &KEY, "pc").unwrap();
        assert_eq!(blob.format, JWT_FORMAT);
        assert_eq!(blob.platform, "pc");
        assert_eq!(decrypt_jwt_with_key(&blob, &KEY).unwrap(), "jwt.abc.123");
    }

    #[test]
    fn a_key_from_another_sign_in_does_not_open_the_file() {
        let blob = encrypt_jwt("jwt.abc.123", &new_jwt_key().unwrap(), "pc").unwrap();
        assert!(decrypt_jwt_with_key(&blob, &new_jwt_key().unwrap()).is_err());
    }

    /// A passphrase-era file, exactly as one sits on disk (its `kdf` block
    /// included), still opens with the derived key the keyring kept for it.
    #[test]
    fn a_passphrase_era_file_opens_with_its_remembered_key() {
        let current = encrypt_jwt("jwt.abc.123", &KEY, "ps4").unwrap();
        let mut v1 = serde_json::to_value(&current).unwrap();
        v1["format"] = JWT_FORMAT_V1.into();
        v1["kdf"] = serde_json::json!({
            "name": "PBKDF2", "hash": "SHA-256", "iterations": 600000, "salt": "c2FsdHNhbHRzYWx0c2FsdA==",
        });
        let blob: EncryptedJwt = serde_json::from_value(v1).unwrap();
        assert_eq!(blob.format, JWT_FORMAT_V1);
        assert_eq!(blob.platform, "ps4");
        assert_eq!(decrypt_jwt_with_key(&blob, &KEY).unwrap(), "jwt.abc.123");
    }

    #[test]
    fn an_unknown_format_is_refused() {
        let mut blob = encrypt_jwt("jwt.abc.123", &KEY, "pc").unwrap();
        blob.format = "wfminv-jwt-v9".into();
        assert!(decrypt_jwt_with_key(&blob, &KEY)
            .unwrap_err()
            .to_string()
            .contains("Unknown JWT blob format"));
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
    fn validate_platform_accepts_known_rejects_unknown() {
        assert!(validate_platform("pc").is_ok());
        assert!(validate_platform("switch").is_ok());
        assert!(validate_platform("PC").is_err());
        assert!(validate_platform("playstation").is_err());
    }
}
