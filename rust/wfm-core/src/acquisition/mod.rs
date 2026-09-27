pub mod error;
pub mod inventory;
pub mod scan;

/// The SPA reduces scan failures to fixed categories by reading their text, so
/// rewording a message here can silently move a real failure into
/// `unclassified_error`. tests/fixtures/scan-error-categories/cases.json holds
/// the text both sides check: this asserts the text is still what the scan
/// produces, and frontend/src/contracts/errors.test.ts asserts its category.
#[cfg(test)]
mod message_fixture_tests {
    use super::{error::ScanError, inventory, scan};

    #[derive(serde::Deserialize)]
    struct Fixture {
        cases: Vec<Case>,
    }

    #[derive(serde::Deserialize)]
    struct Case {
        source: String,
        platform: Option<String>,
        message: String,
    }

    fn produced(source: &str) -> Option<String> {
        Some(match source {
            "game_not_running" => inventory::NOT_RUNNING.to_string(),
            "no_credentials" => scan::no_creds_message(0),
            "no_credentials_unusable_pattern" => scan::no_creds_message(2),
            "endpoint_forbidden" => {
                inventory::short_response_message(reqwest::StatusCode::FORBIDDEN, 12)
            }
            "endpoint_not_found" => {
                inventory::short_response_message(reqwest::StatusCode::NOT_FOUND, 9)
            }
            "scan_busy" => ScanError::Busy.into_message(),
            #[cfg(target_os = "linux")]
            "ptrace_denied" => scan::ptrace_open_error(
                "/proc/1/mem",
                1,
                std::io::Error::from(std::io::ErrorKind::PermissionDenied),
            )
            .to_string(),
            #[cfg(windows)]
            "open_process_failed" => scan::OPEN_PROCESS_FAILED.to_string(),
            _ => return None,
        })
    }

    #[test]
    fn scan_failures_still_produce_the_text_the_spa_classifies() {
        let fixture: Fixture = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/scan-error-categories/cases.json"
        ))
        .expect("fixture parses");
        assert!(!fixture.cases.is_empty());
        for case in fixture.cases {
            let here = case
                .platform
                .as_deref()
                .is_none_or(|p| p == std::env::consts::OS);
            match produced(&case.source) {
                Some(text) => assert!(
                    text.starts_with(&case.message),
                    "{}: the scan now says {text:?}; update the fixture and the SPA's category",
                    case.source
                ),
                None => assert!(!here, "{}: no producer for this source", case.source),
            }
        }
    }
}
