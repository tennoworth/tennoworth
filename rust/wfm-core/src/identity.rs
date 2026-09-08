use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};

/// URL-safe, unpadded base64 of `bytes` random bytes. Used for plan ids and
/// other opaque identifiers.
pub fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    OsRng.fill_bytes(&mut buf);
    B64.encode(&buf)
        .replace('+', "-")
        .replace('/', "_")
        .trim_end_matches('=')
        .to_string()
}

/// Domain-separated local identity without retaining the source credentials or log text.
pub fn local_fingerprint(domain: &str, bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(domain.as_bytes());
    hash.update([0]);
    hash.update(bytes);
    format!("{:x}", hash.finalize())
}
