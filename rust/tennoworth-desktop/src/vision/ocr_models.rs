//! Non-English Tesseract models, fetched on first use.
//!
//! Only `eng.traineddata` ships in the installer. Another language's model is
//! downloaded from Tesseract's own `tessdata_fast` 4.1.0 release - the release
//! the bundled English model comes from - into the app-data directory, and is
//! used only when its size and SHA-256 match the pins below. A file that does
//! not match is never loaded, whoever served it.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

const MODEL_BASE_URL: &str = "https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/4.1.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ModelSpec {
    /// Tesseract's language code, also the file stem.
    pub(crate) code: &'static str,
    pub(crate) bytes: usize,
    pub(crate) sha256: &'static str,
}

/// tessdata_fast 4.1.0, hashed from the release files.
pub(crate) const MODELS: &[ModelSpec] = &[
    ModelSpec {
        code: "chi_sim",
        bytes: 2_469_156,
        sha256: "a5fcb6f0db1e1d6d8522f39db4e848f05984669172e584e8d76b6b3141e1f730",
    },
    ModelSpec {
        code: "chi_tra",
        bytes: 2_366_642,
        sha256: "529c5b5797d64b126065cd55f2bb4c7fd7b15790798091b1ff259941a829330b",
    },
    ModelSpec {
        code: "deu",
        bytes: 1_525_436,
        sha256: "19d219bbb6672c869d20a9636c6816a81eb9a71796cb93ebe0cb1530e2cdb22d",
    },
    ModelSpec {
        code: "fra",
        bytes: 1_130_365,
        sha256: "ced037562e8c80c13122dece28dd477d399af80911a28791a66a63ac1e3445ca",
    },
    ModelSpec {
        code: "ita",
        bytes: 2_701_314,
        sha256: "b8f89e1e785118dac4d51ae042c029a64edb5c3ee42ef73027a6d412748d8827",
    },
    ModelSpec {
        code: "jpn",
        bytes: 2_471_260,
        sha256: "1f5de9236d2e85f5fdf4b3c500f2d4926f8d9449f28f5394472d9e8d83b91b4d",
    },
    ModelSpec {
        code: "kor",
        bytes: 1_677_415,
        sha256: "6b85e11d9bbf07863b97b3523b1b112844c43e713df8b66418a081fd1060b3b2",
    },
    ModelSpec {
        code: "pol",
        bytes: 4_765_518,
        sha256: "c4476cdbc0e33d898d32345122b7be1cbf85ace15f920f06c7714756e1ef79b2",
    },
    ModelSpec {
        code: "por",
        bytes: 1_982_756,
        sha256: "c4932b937207a9514b7514d518b931a99938c02a28a5a5a553f8599ed58b7deb",
    },
    ModelSpec {
        code: "rus",
        bytes: 3_861_738,
        sha256: "e16e5e036cce1d9ec2b00063cf8b54472625b9e14d893a169e2b0dedeb4df225",
    },
    ModelSpec {
        code: "spa",
        bytes: 2_294_433,
        sha256: "6f2e04d02774a18f01bed44b1111f2cd7f3ba7ac9dc4373cd3f898a40ea6b464",
    },
    ModelSpec {
        code: "tur",
        bytes: 4_550_554,
        sha256: "7393381111e1152420fc4092cb44eef4237580d21b92bf30d7d221aad192c6b7",
    },
    ModelSpec {
        code: "ukr",
        bytes: 3_825_102,
        sha256: "d59e53e2bded32f4445f124b4b00240fcac7e8044c003ab822ccb94f0b3db59b",
    },
];

pub(crate) fn model(code: &str) -> Option<&'static ModelSpec> {
    MODELS.iter().find(|spec| spec.code == code)
}

fn matches_pin(spec: &ModelSpec, body: &[u8]) -> bool {
    body.len() == spec.bytes && hex(&Sha256::digest(body)) == spec.sha256
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Make `<dir>/<code>.traineddata` present and verified, fetching it when it
/// is absent or does not match its pin. Returns the tessdata directory.
pub(crate) fn ensure_model(
    dir: &Path,
    spec: &ModelSpec,
    fetch: impl FnOnce(&str, usize) -> Result<Vec<u8>, String>,
) -> Result<PathBuf, String> {
    let path = dir.join(format!("{}.traineddata", spec.code));
    if std::fs::read(&path).is_ok_and(|body| matches_pin(spec, &body)) {
        return Ok(dir.to_path_buf());
    }
    let url = format!("{MODEL_BASE_URL}/{}.traineddata", spec.code);
    let body = fetch(&url, spec.bytes)?;
    if !matches_pin(spec, &body) {
        return Err(format!(
            "the downloaded {} OCR model does not match its pinned checksum",
            spec.code
        ));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let tmp = dir.join(format!(".{}.traineddata.tmp", spec.code));
    std::fs::write(&tmp, &body).map_err(|e| format!("saving the {} OCR model: {e}", spec.code))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("saving the {} OCR model: {e}", spec.code)
    })?;
    Ok(dir.to_path_buf())
}

/// The live fetch: no redirects (the release host serves the file directly)
/// and never more than the pinned size plus one byte.
pub(crate) fn download(url: &str, expected_bytes: usize) -> Result<Vec<u8>, String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(wfm_trading::user_agent())
        .timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| format!("building the model download client: {e}"))?;
    let response = client
        .get(url)
        .send()
        .map_err(|e| format!("downloading the OCR model: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "downloading the OCR model: HTTP {}",
            response.status()
        ));
    }
    let mut body = Vec::with_capacity(expected_bytes);
    response
        .take(expected_bytes as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|e| format!("downloading the OCR model: {e}"))?;
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec_for(body: &[u8]) -> ModelSpec {
        ModelSpec {
            code: "tst",
            bytes: body.len(),
            sha256: Box::leak(hex(&Sha256::digest(body)).into_boxed_str()),
        }
    }

    #[test]
    fn a_verified_download_is_saved_and_then_reused_without_fetching() {
        let dir = tempfile::tempdir().unwrap();
        let spec = spec_for(b"model");
        let got = ensure_model(dir.path(), &spec, |url, bytes| {
            assert!(url.ends_with("/4.1.0/tst.traineddata"));
            assert_eq!(bytes, 5);
            Ok(b"model".to_vec())
        })
        .unwrap();
        assert_eq!(got, dir.path());
        assert_eq!(
            std::fs::read(dir.path().join("tst.traineddata")).unwrap(),
            b"model"
        );
        ensure_model(dir.path(), &spec, |_, _| {
            panic!("a verified model is not fetched again")
        })
        .unwrap();
    }

    #[test]
    fn a_body_that_misses_its_pin_is_never_saved() {
        let dir = tempfile::tempdir().unwrap();
        let spec = spec_for(b"model");
        for body in [&b"MODEL"[..], &b"model!"[..], &b""[..]] {
            let error = ensure_model(dir.path(), &spec, |_, _| Ok(body.to_vec())).unwrap_err();
            assert!(error.contains("checksum"), "{error}");
            assert!(!dir.path().join("tst.traineddata").exists());
        }
    }

    #[test]
    fn a_tampered_saved_model_is_fetched_again() {
        let dir = tempfile::tempdir().unwrap();
        let spec = spec_for(b"model");
        std::fs::write(dir.path().join("tst.traineddata"), b"evil!").unwrap();
        ensure_model(dir.path(), &spec, |_, _| Ok(b"model".to_vec())).unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("tst.traineddata")).unwrap(),
            b"model"
        );
    }

    #[test]
    fn every_pin_is_a_sha256() {
        for spec in MODELS {
            assert_eq!(spec.sha256.len(), 64, "{}", spec.code);
            assert!(
                spec.sha256.chars().all(|c| c.is_ascii_hexdigit()),
                "{}",
                spec.code
            );
        }
    }
}
