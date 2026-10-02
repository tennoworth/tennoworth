use super::find_root;
use crate::ingest::{FixtureHttp, Http, LiveHttp};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub(super) struct BuildInputs {
    pub(super) http: Box<dyn Http>,
    pub(super) csv_path: PathBuf,
    pub(super) json_out: PathBuf,
    pub(super) catalog_out: PathBuf,
    pub(super) prior: serde_json::Value,
}

pub(super) fn prepare(fixtures_dir: Option<&Path>) -> Result<BuildInputs, String> {
    let (http, csv_path, json_out, catalog_out, prior): (
        Box<dyn Http>,
        PathBuf,
        PathBuf,
        PathBuf,
        serde_json::Value,
    ) = if let Some(fd) = fixtures_dir {
        let resp_path = fd.join("fixture_responses.json");
        let raw =
            std::fs::read_to_string(&resp_path).map_err(|e| format!("read {resp_path:?}: {e}"))?;
        let responses: HashMap<String, serde_json::Value> =
            serde_json::from_str(&raw).map_err(|e| format!("parse {resp_path:?}: {e}"))?;
        let http = FixtureHttp { responses };
        let csv = fd.join("wfm_results.csv");
        let out = fd.join("market.json");
        let cat = fd.join("wfstat-catalog.json");
        let prior_path = fd.join("prior-market.json");
        let prior = if prior_path.exists() {
            let s = std::fs::read_to_string(&prior_path).map_err(|e| format!("read prior: {e}"))?;
            serde_json::from_str(&s).unwrap_or(serde_json::Value::Object(serde_json::Map::new()))
        } else {
            serde_json::Value::Object(serde_json::Map::new())
        };
        let prior_catalog = fd.join("prior-catalog.json");
        if prior_catalog.exists() && !cat.exists() {
            eprintln!("  preserving prior wfstat-catalog");
        }
        (Box::new(http), csv, out, cat, prior)
    } else {
        let root = find_root()?;
        let csv = root.join("wfm_results.csv");
        let out = root.join("frontend").join("public").join("market.json");
        let cat = root
            .join("frontend")
            .join("public")
            .join("wfstat-catalog.json");

        let client = reqwest::blocking::Client::builder()
            .retry(reqwest::retry::never())
            .redirect(wfm_client::redirect_policy())
            .user_agent(wfm_client::user_agent(
                "wfm-scrape",
                env!("CARGO_PKG_VERSION"),
            ))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("build HTTP client: {e}"))?;
        let http = LiveHttp { client };

        let prior = if out.exists() {
            let s = std::fs::read_to_string(&out).map_err(|e| format!("read prior: {e}"))?;
            serde_json::from_str(&s).unwrap_or(serde_json::Value::Object(serde_json::Map::new()))
        } else {
            serde_json::Value::Object(serde_json::Map::new())
        };

        (Box::new(http), csv, out, cat, prior)
    };

    if !csv_path.exists() {
        return Err(format!(
            "{} not found - run `wfm-scrape scrape` first.",
            csv_path.display()
        ));
    }

    Ok(BuildInputs {
        http,
        csv_path,
        json_out,
        catalog_out,
        prior,
    })
}
