//! Signed restrictions are verified before parsing and persisted before activation.
use crate::governor::{lock, process, AccessError, Restrictions};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

pub const POLICY_URL: &str = "https://tennoworth.app/wfm-policy.json";
pub const MAX_POLICY_BYTES: u64 = 65_536;
pub const PUBLIC_KEY: Option<&str> = option_env!("TENNOWORTH_WFM_POLICY_PUBLIC_KEY");

#[derive(Clone, Copy)]
pub enum Component {
    Desktop,
    Scraper,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub payload: String,
    pub signature: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub schema: u32,
    pub revision: u64,
    pub issued_at_ms: u64,
    pub reason: String,
    pub desktop: Restrictions,
    pub scraper: Restrictions,
}
pub fn verify(raw: &[u8], key: &str) -> Result<(Envelope, Policy), AccessError> {
    let invalid = |_| AccessError::InvalidPolicy;
    if raw.len() as u64 > MAX_POLICY_BYTES {
        return Err(AccessError::InvalidPolicy);
    }
    let envelope: Envelope = serde_json::from_slice(raw).map_err(invalid)?;
    let payload = base64::engine::general_purpose::STANDARD
        .decode(&envelope.payload)
        .map_err(|_| AccessError::InvalidPolicy)?;
    let signature = minisign_verify::Signature::decode(&envelope.signature)
        .map_err(|_| AccessError::InvalidPolicy)?;
    let key = minisign_verify::PublicKey::from_base64(key.trim())
        .map_err(|_| AccessError::InvalidPolicy)?;
    key.verify(&payload, &signature, false)
        .map_err(|_| AccessError::InvalidPolicy)?;
    let policy: Policy = serde_json::from_slice(&payload).map_err(invalid)?;
    if policy.schema != 1
        || policy.revision == 0
        || policy.revision > 9_007_199_254_740_991
        || policy.issued_at_ms == 0
        || policy.issued_at_ms > 253_402_300_799_999
        || policy.reason.len() > 500
        || policy.reason.chars().any(char::is_control)
    {
        return Err(AccessError::InvalidPolicy);
    }
    policy.desktop.validate()?;
    policy.scraper.validate()?;
    Ok((envelope, policy))
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(&temp, path)?;
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}
struct Verified {
    envelope: Envelope,
    policy: Policy,
}
pub struct PolicyStore {
    path: std::path::PathBuf,
    key: String,
    component: Component,
    current: Mutex<Option<Verified>>,
}
impl PolicyStore {
    pub fn load(
        path: std::path::PathBuf,
        key: String,
        component: Component,
    ) -> Result<Self, AccessError> {
        let store = Self {
            path,
            key,
            component,
            current: Mutex::new(None),
        };
        match std::fs::read(&store.path) {
            Ok(raw) => {
                let (envelope, policy) = verify(&raw, &store.key)?;
                store.activate(&policy)?;
                *lock(&store.current) = Some(Verified { envelope, policy });
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(AccessError::Persistence),
        }
        Ok(store)
    }
    fn activate(&self, p: &Policy) -> Result<(), AccessError> {
        let restrictions = match self.component {
            Component::Desktop => &p.desktop,
            Component::Scraper => &p.scraper,
        };
        process().apply_policy(p.revision, p.reason.clone(), restrictions.clone())
    }
    pub fn accept(&self, raw: &[u8]) -> Result<(), AccessError> {
        let (envelope, policy) = verify(raw, &self.key)?;
        let mut current = lock(&self.current);
        if let Some(prior) = current.as_ref() {
            if policy.revision < prior.policy.revision
                || (policy.revision == prior.policy.revision
                    && envelope.payload != prior.envelope.payload)
            {
                return Err(AccessError::InvalidPolicy);
            }
            if policy.revision == prior.policy.revision {
                return Ok(());
            }
        }
        atomic_write(&self.path, raw).map_err(|_| {
            process().persistence_failed();
            AccessError::Persistence
        })?;
        self.activate(&policy)?;
        *current = Some(Verified { envelope, policy });
        Ok(())
    }
    pub fn fetch(&self, etag: &mut Option<String>) -> Result<(), AccessError> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(crate::default_user_agent())
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| AccessError::Transport)?;
        let mut request = client.get(POLICY_URL);
        if let Some(tag) = etag.as_ref() {
            request = request.header(reqwest::header::IF_NONE_MATCH, tag);
        }
        let response = request.send().map_err(|_| AccessError::Transport)?;
        if response.status() == reqwest::StatusCode::NOT_MODIFIED && lock(&self.current).is_some() {
            return Ok(());
        }
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|n| n > MAX_POLICY_BYTES)
        {
            return Err(AccessError::InvalidPolicy);
        }
        let new_tag = response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .filter(|s| s.len() <= 512)
            .map(String::from);
        let mut raw = Vec::new();
        response
            .take(MAX_POLICY_BYTES + 1)
            .read_to_end(&mut raw)
            .map_err(|_| AccessError::Transport)?;
        self.accept(&raw)?;
        *etag = new_tag;
        Ok(())
    }
}

/// Call only after claiming the application's single-instance/output lock.
pub fn start(directory: &Path, component: Component) -> Result<(), AccessError> {
    process().configure_persistence(directory.join("wfm-cooldown.json"))?;
    let Some(key) = PUBLIC_KEY else {
        return Ok(());
    };
    let store = PolicyStore::load(directory.join("wfm-policy.json"), key.to_owned(), component)?;
    let mut etag = None;
    if let Err(error) = store.fetch(&mut etag) {
        eprintln!("WFM policy refresh: {error}");
    }
    std::thread::Builder::new()
        .name("wfm-policy".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_secs(900) + crate::governor::jitter(60_000));
            if let Err(error) = store.fetch(&mut etag) {
                eprintln!("WFM policy refresh: {error}");
            }
        })
        .map_err(|_| AccessError::Persistence)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../../../tests/fixtures/wfm-access/policies.json"
        ))
        .unwrap()
    }
    #[test]
    fn signatures_are_verified_before_interpreting_restrictions() {
        let fixtures = fixture();
        let key = fixtures["public_key"].as_str().unwrap();
        let initial = serde_json::to_vec(&fixtures["initial"]).unwrap();
        assert_eq!(verify(&initial, key).unwrap().1.desktop.spacing_ms, 1000);
        let mut tampered = fixtures["initial"].clone();
        tampered["payload"] = fixtures["recovery"]["payload"].clone();
        assert!(verify(&serde_json::to_vec(&tampered).unwrap(), key).is_err());
        assert!(verify(
            &serde_json::to_vec(&fixtures["invalid_bounds"]).unwrap(),
            key
        )
        .is_err());
        assert!(verify(&vec![0; MAX_POLICY_BYTES as usize + 1], key).is_err());
    }
    #[test]
    fn verified_policy_survives_offline_restart_and_rejects_replays() {
        let fixtures = fixture();
        let path = std::env::temp_dir().join(format!("wfm-policy-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let key = fixtures["public_key"].as_str().unwrap().to_string();
        let store = PolicyStore::load(path.clone(), key.clone(), Component::Desktop).unwrap();
        let initial = serde_json::to_vec(&fixtures["initial"]).unwrap();
        store.accept(&initial).unwrap();
        store.accept(&initial).unwrap();
        assert!(store
            .accept(&serde_json::to_vec(&fixtures["same_revision_changed"]).unwrap())
            .is_err());
        store
            .accept(&serde_json::to_vec(&fixtures["recovery"]).unwrap())
            .unwrap();
        assert!(store.accept(&initial).is_err());
        let restarted = PolicyStore::load(path.clone(), key, Component::Desktop).unwrap();
        assert_eq!(
            lock(&restarted.current).as_ref().unwrap().policy.revision,
            2
        );
        assert!(restarted.accept(&initial).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
