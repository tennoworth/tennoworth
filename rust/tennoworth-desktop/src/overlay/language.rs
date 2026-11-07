//! Which language the reward screen is in.
//!
//! The game writes its language near the top of EE.log
//! (`Sys [Info]: Using language: _fr`, after `-language:fr` on the command
//! line), before the stretch the tailer keeps, so the head of the file is read
//! directly. A setting can override the detection.

use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OcrLanguage {
    /// warframe.market's language code: the settings value and the key of the
    /// snapshot's localized reward names.
    pub(crate) code: &'static str,
    /// Tesseract's model for it.
    pub(crate) tesseract: &'static str,
    /// The game's own codes for it, as `-language:` and `Using language: _`
    /// print them. DE's exports name simplified Chinese `zh` and traditional
    /// Chinese `tc`.
    game_codes: &'static [&'static str],
}

pub(crate) const ENGLISH: OcrLanguage = OcrLanguage {
    code: "en",
    tesseract: "eng",
    game_codes: &["en"],
};

/// Every language with both a warframe.market translation and a Tesseract
/// model. Thai, the remaining game language, has no market translation.
pub(crate) const LANGUAGES: &[OcrLanguage] = &[
    ENGLISH,
    OcrLanguage {
        code: "de",
        tesseract: "deu",
        game_codes: &["de"],
    },
    OcrLanguage {
        code: "es",
        tesseract: "spa",
        game_codes: &["es"],
    },
    OcrLanguage {
        code: "fr",
        tesseract: "fra",
        game_codes: &["fr"],
    },
    OcrLanguage {
        code: "it",
        tesseract: "ita",
        game_codes: &["it"],
    },
    OcrLanguage {
        code: "ja",
        tesseract: "jpn",
        game_codes: &["ja"],
    },
    OcrLanguage {
        code: "ko",
        tesseract: "kor",
        game_codes: &["ko"],
    },
    OcrLanguage {
        code: "pl",
        tesseract: "pol",
        game_codes: &["pl"],
    },
    OcrLanguage {
        code: "pt",
        tesseract: "por",
        game_codes: &["pt"],
    },
    OcrLanguage {
        code: "ru",
        tesseract: "rus",
        game_codes: &["ru"],
    },
    OcrLanguage {
        code: "tr",
        tesseract: "tur",
        game_codes: &["tr"],
    },
    OcrLanguage {
        code: "uk",
        tesseract: "ukr",
        game_codes: &["uk"],
    },
    OcrLanguage {
        code: "zh-hans",
        tesseract: "chi_sim",
        game_codes: &["zh", "zh-hans"],
    },
    OcrLanguage {
        code: "zh-hant",
        tesseract: "chi_tra",
        game_codes: &["tc", "zh-hant"],
    },
];

pub(crate) const AUTO: &str = "auto";

pub(crate) fn by_code(code: &str) -> Option<OcrLanguage> {
    LANGUAGES
        .iter()
        .copied()
        .find(|language| language.code == code)
}

fn by_game_code(code: &str) -> Option<OcrLanguage> {
    let code = code.trim().to_ascii_lowercase();
    LANGUAGES
        .iter()
        .copied()
        .find(|language| language.game_codes.contains(&code.as_str()))
}

/// The game code a log names, if any. A later `Using language` wins over the
/// command line, because it is what the client actually loaded.
pub(crate) fn game_code_in_log(text: &str) -> Option<String> {
    let mut found = None;
    for line in text.lines() {
        if let Some(rest) = line.split("Using language: _").nth(1) {
            found = rest.split_whitespace().next().map(str::to_string);
        } else if found.is_none() {
            if let Some(rest) = line.split(" -language:").nth(1) {
                found = rest.split_whitespace().next().map(str::to_string);
            }
        }
    }
    found.filter(|code| !code.is_empty())
}

/// The language the game at `log` is running in, from the head of its log.
pub(crate) fn detect(log: &Path) -> Option<String> {
    const HEAD: u64 = 256 * 1024;
    let mut head = Vec::new();
    std::fs::File::open(log)
        .ok()?
        .take(HEAD)
        .read_to_end(&mut head)
        .ok()?;
    game_code_in_log(&String::from_utf8_lossy(&head))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Resolved {
    Supported(OcrLanguage),
    /// The game reports a language with no names or model (Thai, or a code
    /// this build does not know). English is used, and the status says why.
    Unsupported(String),
}

/// The setting decides; `auto` follows the game and falls back to English
/// when the log has not said.
pub(crate) fn resolve(setting: &str, detected_game_code: Option<&str>) -> Resolved {
    if setting != AUTO {
        return Resolved::Supported(by_code(setting).unwrap_or(ENGLISH));
    }
    match detected_game_code {
        None => Resolved::Supported(ENGLISH),
        Some(code) => by_game_code(code)
            .map(Resolved::Supported)
            .unwrap_or_else(|| Resolved::Unsupported(code.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_loaded_language_wins_over_the_command_line() {
        let log = "0.098 Sys [Diag]: Process Command-line: -windowMode:2 -cluster:public -language:en -clienttype:Steam\n\
                   0.173 Sys [Info]: Cache languages enabled: _fr\n\
                   2.768 Sys [Info]: Using language: _fr\n";
        assert_eq!(game_code_in_log(log).as_deref(), Some("fr"));
        let early = "0.098 Sys [Diag]: Process Command-line: -windowMode:2 -language:de -clienttype:Steam\n";
        assert_eq!(game_code_in_log(early).as_deref(), Some("de"));
        assert_eq!(game_code_in_log("1.0 Script [Info]: nothing here"), None);
    }

    #[test]
    fn auto_follows_the_game_and_a_choice_overrides_it() {
        assert_eq!(resolve(AUTO, None), Resolved::Supported(ENGLISH));
        assert_eq!(resolve(AUTO, Some("fr")).code(), Some("fr"));
        assert_eq!(resolve(AUTO, Some("tc")).code(), Some("zh-hant"));
        assert_eq!(resolve(AUTO, Some("zh")).code(), Some("zh-hans"));
        assert_eq!(
            resolve(AUTO, Some("th")),
            Resolved::Unsupported("th".into())
        );
        assert_eq!(resolve("ru", Some("fr")).code(), Some("ru"));
        assert_eq!(resolve("xx", Some("fr")).code(), Some("en"));
    }

    #[test]
    fn every_language_but_english_has_a_pinned_model() {
        for language in LANGUAGES.iter().filter(|language| language.code != "en") {
            assert!(
                crate::vision::ocr_models::model(language.tesseract).is_some(),
                "{}",
                language.code
            );
        }
    }

    #[test]
    fn the_head_of_a_real_log_names_the_language() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("EE.log");
        std::fs::write(&path, "2.768 Sys [Info]: Using language: _ko\n").unwrap();
        assert_eq!(detect(&path).as_deref(), Some("ko"));
        assert_eq!(detect(&dir.path().join("missing.log")), None);
    }

    impl Resolved {
        fn code(&self) -> Option<&'static str> {
            match self {
                Resolved::Supported(language) => Some(language.code),
                Resolved::Unsupported(_) => None,
            }
        }
    }
}
