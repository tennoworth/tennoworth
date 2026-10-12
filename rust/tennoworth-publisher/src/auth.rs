use axum::http::HeaderMap;
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

#[derive(Deserialize)]
struct Claims {
    email: String,
}

struct Keys {
    set: Option<JwkSet>,
    fetched: Option<Instant>,
    attempted: Option<Instant>,
}

pub struct Access {
    issuer: String,
    audience: String,
    principals: BTreeSet<String>,
    client: reqwest::Client,
    keys: Mutex<Keys>,
}

impl Access {
    pub fn new(
        issuer: String,
        audience: String,
        principals: BTreeSet<String>,
    ) -> anyhow::Result<Self> {
        let url = url::Url::parse(&issuer)?;
        let host = url.host_str().unwrap_or_default();
        if url.scheme() != "https"
            || !host.ends_with(".cloudflareaccess.com")
            || url.origin().ascii_serialization() != issuer
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || audience.trim().is_empty()
            || principals.is_empty()
            || principals.iter().any(|p| p.trim().is_empty())
        {
            anyhow::bail!("invalid Access configuration");
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("TennoWorth publisher")
            .build()?;
        // Email addresses are compared without case, as the identity provider may vary it.
        let principals = principals.iter().map(|p| p.to_lowercase()).collect();
        Ok(Self {
            issuer,
            audience,
            principals,
            client,
            keys: Mutex::new(Keys {
                set: None,
                fetched: None,
                attempted: None,
            }),
        })
    }

    pub async fn principal(&self, headers: &HeaderMap) -> Option<String> {
        let token = headers.get("cf-access-jwt-assertion")?.to_str().ok()?;
        if token.len() > 16384 {
            return None;
        }
        let header = decode_header(token).ok()?;
        if header.alg != Algorithm::RS256 {
            return None;
        }
        let kid = header.kid?;
        let mut keys = self.keys.lock().await;
        let stale = keys
            .fetched
            .is_none_or(|at| at.elapsed() >= Duration::from_secs(3600));
        let missing = keys.set.as_ref().is_none_or(|set| set.find(&kid).is_none());
        if stale || missing {
            if keys
                .attempted
                .is_some_and(|at| at.elapsed() < Duration::from_secs(30))
            {
                return None;
            }
            keys.attempted = Some(Instant::now());
            let mut response = self
                .client
                .get(format!("{}/cdn-cgi/access/certs", self.issuer))
                .send()
                .await
                .ok()?
                .error_for_status()
                .ok()?;
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.ok()? {
                if bytes.len() + chunk.len() > 65536 {
                    return None;
                }
                bytes.extend_from_slice(&chunk);
            }
            keys.set = Some(serde_json::from_slice(&bytes).ok()?);
            keys.fetched = Some(Instant::now());
        }
        self.verify(token, keys.set.as_ref()?)
    }

    fn verify(&self, token: &str, keys: &JwkSet) -> Option<String> {
        let header = decode_header(token).ok()?;
        if header.alg != Algorithm::RS256 {
            return None;
        }
        let key = DecodingKey::from_jwk(keys.find(&header.kid?)?).ok()?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[&self.issuer]);
        validation.set_audience(&[&self.audience]);
        validation.set_required_spec_claims(&["iss", "aud", "exp", "sub"]);
        validation.validate_nbf = true;
        // Access issues tokens valid from the moment of sign-in; without a little
        // tolerance a host clock a second behind Cloudflare refuses a fresh login.
        validation.leeway = 60;
        let email = decode::<Claims>(token, &key, &validation)
            .ok()?
            .claims
            .email
            .to_lowercase();
        self.principals.contains(&email).then_some(email)
    }
}

pub enum Auth {
    Access(Access),
    #[cfg(any(test, feature = "dev-server"))]
    Local,
}

impl Auth {
    pub async fn principal(&self, headers: &HeaderMap) -> Option<String> {
        match self {
            Self::Access(access) => access.principal(headers).await,
            #[cfg(any(test, feature = "dev-server"))]
            Self::Local => Some("local-preview@example.invalid".into()),
        }
    }
}

#[cfg(all(feature = "dev-server", not(debug_assertions)))]
compile_error!("The development publisher cannot be built in release mode.");

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use jsonwebtoken::{encode, EncodingKey, Header};
    use std::process::Command;

    #[tokio::test]
    async fn only_signed_current_tokens_for_the_configured_application_and_principal_are_accepted()
    {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key.pem");
        let generated = Command::new("openssl")
            .args([
                "genpkey",
                "-algorithm",
                "RSA",
                "-pkeyopt",
                "rsa_keygen_bits:2048",
                "-out",
            ])
            .arg(&path)
            .output()
            .unwrap();
        assert!(generated.status.success());
        let modulus = Command::new("openssl")
            .args(["rsa", "-modulus", "-noout", "-in"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(modulus.status.success());
        let hex = String::from_utf8(modulus.stdout).unwrap();
        let hex = hex.trim().strip_prefix("Modulus=").unwrap();
        let bytes: Vec<_> = hex
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| u8::from_str_radix(std::str::from_utf8(chunk).unwrap(), 16).unwrap())
            .collect();
        let keys: JwkSet = serde_json::from_value(serde_json::json!({"keys":[{"kty":"RSA","kid":"test-key","alg":"RS256","use":"sig","n":URL_SAFE_NO_PAD.encode(bytes),"e":"AQAB"}]})).unwrap();
        let der = Command::new("openssl")
            .args(["rsa", "-traditional", "-outform", "DER", "-in"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(der.status.success());
        let signer = EncodingKey::from_rsa_der(&der.stdout);
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some("test-key".into());
        let access = Access::new(
            "https://test.cloudflareaccess.com".into(),
            "dashboard-audience".into(),
            BTreeSet::from(["publisher@example.invalid".into()]),
        )
        .unwrap();
        let now = crate::store::now();
        let claims = serde_json::json!({"email":"publisher@example.invalid","sub":"operator-1","iss":"https://test.cloudflareaccess.com","aud":["dashboard-audience"],"exp":now+300,"nbf":now-1});
        let signed = encode(&header, &claims, &signer).unwrap();
        assert_eq!(
            access.verify(&signed, &keys),
            Some("publisher@example.invalid".into())
        );
        *access.keys.lock().await = Keys {
            set: Some(keys.clone()),
            fetched: Some(Instant::now()),
            attempted: Some(Instant::now()),
        };
        let mut headers = HeaderMap::new();
        headers.insert("cf-access-jwt-assertion", signed.parse().unwrap());
        assert_eq!(
            access.principal(&headers).await,
            Some("publisher@example.invalid".into())
        );
        access.keys.lock().await.fetched = Instant::now().checked_sub(Duration::from_secs(3601));
        assert!(access.principal(&headers).await.is_none());
        access.keys.lock().await.fetched = Some(Instant::now());
        let mut unknown = header.clone();
        unknown.kid = Some("unknown-key".into());
        let mut unknown_headers = HeaderMap::new();
        unknown_headers.insert(
            "cf-access-jwt-assertion",
            encode(&unknown, &claims, &signer).unwrap().parse().unwrap(),
        );
        assert!(access.principal(&unknown_headers).await.is_none());
        for (field, value) in [
            (
                "iss",
                serde_json::json!("https://attacker.cloudflareaccess.com"),
            ),
            ("aud", serde_json::json!(["another-app"])),
            ("exp", serde_json::json!(now - 61)),
            ("nbf", serde_json::json!(now + 300)),
            ("email", serde_json::json!("other@example.invalid")),
        ] {
            let mut bad = claims.clone();
            bad[field] = value;
            assert!(
                access
                    .verify(&encode(&header, &bad, &signer).unwrap(), &keys)
                    .is_none(),
                "{field}"
            );
        }
        for field in ["exp", "iss", "aud", "sub", "email"] {
            let mut bad = claims.clone();
            bad.as_object_mut().unwrap().remove(field);
            assert!(
                access
                    .verify(&encode(&header, &bad, &signer).unwrap(), &keys)
                    .is_none(),
                "missing {field}"
            );
        }
        let mut skewed = claims.clone();
        skewed["nbf"] = serde_json::json!(now + 30);
        skewed["email"] = serde_json::json!("Publisher@Example.Invalid");
        assert_eq!(
            access.verify(&encode(&header, &skewed, &signer).unwrap(), &keys),
            Some("publisher@example.invalid".into()),
            "a slightly fast Access clock and differently cased email"
        );
        let mut altered = signed.clone().into_bytes();
        let last = altered.len() - 10;
        altered[last] = if altered[last] == b'A' { b'B' } else { b'A' };
        assert!(access
            .verify(std::str::from_utf8(&altered).unwrap(), &keys)
            .is_none());
        let mut header = Header::new(Algorithm::HS256);
        header.kid = Some("test-key".into());
        assert!(access
            .verify(
                &encode(
                    &header,
                    &claims,
                    &EncodingKey::from_secret(b"test-secret-only")
                )
                .unwrap(),
                &keys
            )
            .is_none());
        use axum::{
            body::Body,
            http::{Request, StatusCode},
        };
        use tower::ServiceExt;
        let store = crate::store::Store::open(
            &dir.path().join("state.sqlite"),
            dir.path().join("announcements.json"),
            r#"{"version":1,"updated_at":"2026-01-01T00:00:00Z","messages":[]}"#,
        )
        .unwrap();
        let app = crate::App::new(
            store,
            Auth::Access(access),
            "https://admin.example.invalid".into(),
        )
        .unwrap();
        let response = crate::router(app)
            .oneshot(
                Request::builder()
                    .uri("/api/announcements")
                    .header("cf-access-jwt-assertion", signed)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
}
