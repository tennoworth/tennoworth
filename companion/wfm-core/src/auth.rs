//! warframe.market auth + JWT-at-rest.
//!
//! Auth flow (discovered May 2026 by inspecting the WFM frontend bundle — the
//! API spec doesn't document it): GET the signin page to populate a session
//! cookie + read the `<meta name="csrf-token">`, then POST `/v1/auth/signin`
//! with `auth_type: "cookie"` and that CSRF token. WFM bakes a `csrf_token`
//! claim into the returned JWT — v2 endpoints reject header-auth JWTs, so the
//! cookie flow is the only one that works. The JWT arrives via `Set-Cookie`.
//!
//! At rest the JWT is AES-256-GCM encrypted, key derived via PBKDF2-HMAC-SHA256
//! (600k iterations, OWASP 2023). **The on-disk envelope shape and its default
//! path (`~/.config/wfminv/wfm-jwt.enc`) are a compatibility contract** — do not
//! change field names, the format tag, or the KDF without a migration; existing
//! users' files must keep decrypting.
//!
//! No terminal I/O lives here: the caller reads the passphrase (from a TTY, a
//! pipe, or a desktop dialog) and hands the plaintext to `decrypt_jwt` /
//! `encrypt_jwt`.

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use hmac::Hmac;
use pbkdf2::pbkdf2;
use rand::RngCore;
use regex::bytes::Regex;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::time::Duration;

use crate::util::chrono_now_iso;

const WFM_SIGNIN_URL: &str = "https://api.warframe.market/v1/auth/signin";
const WFM_BOOTSTRAP_URL: &str = "https://warframe.market/auth/signin";

pub const JWT_FORMAT: &str = "wfminv-jwt-v1";
pub const JWT_KDF_ITERATIONS: u32 = 600_000;

/// WFM account platforms. `pc` covers Steam & Epic.
pub const PLATFORMS: [&str; 4] = ["pc", "ps4", "xbox", "switch"];

/// The on-disk encrypted-JWT envelope. Field names + shape are a compat
/// contract — see the module docs. Mirrors the web app's encrypted-export
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

/// Reject a mistyped platform up front — an unknown value would otherwise be
/// baked into the encrypted JWT and silently authenticate against the wrong
/// (or a non-existent) WFM market on every later serve.
pub fn validate_platform(platform: &str) -> Result<()> {
    if !PLATFORMS.contains(&platform) {
        bail!(
            "Unknown --platform '{}'. Use one of: {}. (pc covers Steam & Epic.)",
            platform,
            PLATFORMS.join(", ")
        );
    }
    Ok(())
}

/// GET the signin page: build a cookie-storing client (the session cookie set
/// here must ride the later signin POST), and scrape the CSRF token out of the
/// page. Returns the client so `signin` reuses the same cookie jar.
pub fn bootstrap_session() -> Result<(Client, String)> {
    let client = Client::builder()
        .user_agent(crate::BROWSER_UA)
        .cookie_store(true)
        .timeout(Duration::from_secs(30))
        .build()
        .context("building HTTP client")?;

    let bootstrap = client
        .get(WFM_BOOTSTRAP_URL)
        .send()
        .context("bootstrap GET failed (Cloudflare may have blocked us)")?;
    if !bootstrap.status().is_success() {
        bail!(
            "Bootstrap GET returned HTTP {} — Cloudflare or WFM may have changed.",
            bootstrap.status()
        );
    }
    let bootstrap_html = bootstrap.text().context("reading bootstrap response")?;

    // Cheap regex — we only care about the meta tag, no HTML parsing needed.
    let csrf_re = Regex::new(r#"name="csrf-token"\s+content="([^"]+)""#)
        .expect("static regex");
    let csrf_token = csrf_re
        .captures(bootstrap_html.as_bytes())
        .and_then(|c| c.get(1))
        .map(|m| std::str::from_utf8(m.as_bytes()).unwrap_or("").to_string())
        .ok_or_else(|| {
            anyhow!(
                "Couldn't find <meta name=\"csrf-token\"> on the signin page. \
                 WFM may have changed their auth flow."
            )
        })?;
    Ok((client, csrf_token))
}

/// POST the credentials with the CSRF token on the same (cookie-storing) client
/// from `bootstrap_session`, and pull the JWT out of the `Set-Cookie` response.
pub fn signin(
    client: &Client,
    email: &str,
    password: &str,
    platform: &str,
    csrf_token: &str,
) -> Result<String> {
    // We sign in with auth_type=cookie. WFM bakes a `csrf_token` claim into
    // the resulting JWT — v2 endpoints (like /v2/order) require this claim
    // type, header-auth JWTs are rejected. The JWT is returned via Set-Cookie.
    let body = serde_json::json!({
        "email": email,
        "password": password,
        "auth_type": "cookie",
    });
    let resp = client
        .post(WFM_SIGNIN_URL)
        .header("Platform", platform)
        .header("Language", "en")
        .header("auth_type", "cookie")
        .header("X-CSRFToken", csrf_token)
        .json(&body)
        .send()
        .context("signin request failed")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        bail!(
            "Signin failed: HTTP {status}\nResponse body:\n{}",
            &body[..body.len().min(400)]
        );
    }

    // The post-signin JWT comes back in Set-Cookie. Reqwest's cookie store
    // keeps it for subsequent requests, but we need the raw value to encrypt
    // and persist — pull it out of the response headers.
    let jwt = resp
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|hv| hv.to_str().ok())
        .find_map(|s| {
            // Set-Cookie: JWT=<token>; Domain=...; Path=/; ...
            s.split(';').next()?.strip_prefix("JWT=").map(|s| s.to_string())
        })
        .ok_or_else(|| anyhow!(
            "Signin succeeded but no JWT cookie in response. \
             WFM may have rotated the auth flow."
        ))?;
    if jwt.is_empty() {
        bail!("Empty JWT in Set-Cookie.");
    }
    Ok(jwt)
}

pub fn encrypt_jwt(jwt: &str, passphrase: &str, platform: &str) -> Result<EncryptedJwt> {
    let mut salt = [0u8; 16];
    let mut iv = [0u8; 12];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut iv);

    let mut key_bytes = [0u8; 32];
    pbkdf2::<Hmac<Sha256>>(passphrase.as_bytes(), &salt, JWT_KDF_ITERATIONS, &mut key_bytes)
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
/// keyring for silent unlock — the key is salt-bound (a re-login rotates the
/// salt, so a stale key fails GCM auth) and useless without the .enc file,
/// unlike the passphrase, which users may reuse elsewhere.
pub fn derive_jwt_key(blob: &EncryptedJwt, passphrase: &str) -> Result<[u8; 32]> {
    let salt = B64.decode(&blob.kdf.salt).context("decoding salt")?;
    let mut key_bytes = [0u8; 32];
    pbkdf2::<Hmac<Sha256>>(passphrase.as_bytes(), &salt, blob.kdf.iterations, &mut key_bytes)
        .map_err(|e| anyhow!("PBKDF2 failed: {e}"))?;
    Ok(key_bytes)
}

pub fn decrypt_jwt_with_key(blob: &EncryptedJwt, key_bytes: &[u8; 32]) -> Result<String> {
    if blob.format != JWT_FORMAT {
        bail!("Unknown JWT blob format: {}", blob.format);
    }
    let iv = B64.decode(&blob.cipher.iv).context("decoding IV")?;
    let ciphertext = B64.decode(&blob.ciphertext).context("decoding ciphertext")?;

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

/// Resolve the WFM username (`data.slug`) for a decrypted JWT. Used when
/// warming listing credentials.
pub fn fetch_wfm_me(client: &Client, jwt: &str, platform: &str) -> Result<String> {
    let resp = client
        .get("https://api.warframe.market/v2/me")
        .header("Platform", platform)
        .header("Language", "en")
        .header("Crossplay", "true")
        .header("Cookie", format!("JWT={jwt}"))
        .header("Origin", "https://warframe.market")
        .header("Referer", "https://warframe.market/")
        .send()
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
}
