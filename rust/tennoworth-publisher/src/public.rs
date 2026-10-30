//! Whether visitors actually receive the feed the publisher last exported.
//!
//! The dashboard's CSP only allows `connect-src 'self'`, so the browser cannot
//! ask tennoworth.app itself; the publisher fetches the public URL instead and
//! compares the served `updated_at` with its own. Writing the export only proves
//! the file changed on the box, not that Caddy and the edge serve it.

use crate::contracts::{PublicFeedCheck, PublicFeedState};
use std::{path::PathBuf, time::Duration};

const LIMIT: usize = 65536;

pub enum PublicFeed {
    Url {
        url: String,
        client: reqwest::Client,
    },
    /// The local dashboard has no public site; it reads its own export.
    File(PathBuf),
}

impl PublicFeed {
    pub fn url(url: String) -> anyhow::Result<Self> {
        let parsed = url::Url::parse(&url)?;
        if parsed.scheme() != "https" {
            anyhow::bail!("the public feed URL must use HTTPS");
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self::Url { url, client })
    }

    fn label(&self) -> String {
        match self {
            Self::Url { url, .. } => url.clone(),
            Self::File(path) => path.display().to_string(),
        }
    }

    async fn served(&self) -> Result<String, String> {
        match self {
            Self::Url { url, client } => {
                let mut response = client
                    .get(url)
                    .header("cache-control", "no-cache")
                    .send()
                    .await
                    .map_err(|_| "The public site did not answer.".to_owned())?;
                if !response.status().is_success() {
                    return Err(format!(
                        "The public site answered HTTP {}.",
                        response.status().as_u16()
                    ));
                }
                let mut body = Vec::new();
                while let Some(chunk) = response
                    .chunk()
                    .await
                    .map_err(|_| "The public feed download was interrupted.".to_owned())?
                {
                    body.extend_from_slice(&chunk);
                    if body.len() > LIMIT {
                        return Err("The public feed is larger than any valid feed.".into());
                    }
                }
                String::from_utf8(body).map_err(|_| "The public feed is not text.".into())
            }
            Self::File(path) => std::fs::read_to_string(path)
                .map_err(|_| "The local export could not be read.".into()),
        }
    }

    /// `expected` is the `updated_at` of the newest published revision.
    pub async fn check(&self, expected: &str, now: i64) -> PublicFeedCheck {
        let (state, served_updated_at, error) = match self.served().await {
            Err(error) => (PublicFeedState::Unreachable, None, Some(error)),
            Ok(raw) => match announcement_feed::parse(&raw) {
                None => (
                    PublicFeedState::Unreachable,
                    None,
                    Some("The public site serves a feed that does not validate.".into()),
                ),
                Some(feed) => (
                    compare(&feed.updated_at, expected),
                    Some(feed.updated_at),
                    None,
                ),
            },
        };
        PublicFeedCheck {
            url: self.label(),
            checked_at: crate::store::utc(now).unwrap_or_default(),
            expected_updated_at: expected.to_owned(),
            served_updated_at,
            state,
            error,
        }
    }
}

fn compare(served: &str, expected: &str) -> PublicFeedState {
    match (
        announcement_feed::stamp(served),
        announcement_feed::stamp(expected),
    ) {
        (Some(served), Some(expected)) if served == expected => PublicFeedState::Live,
        (Some(served), Some(expected)) if served < expected => PublicFeedState::Behind,
        // Newer than anything this publisher exported: a restore or another writer.
        _ => PublicFeedState::Different,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(updated_at: &str) -> String {
        format!(r#"{{"version":1,"updated_at":"{updated_at}","messages":[]}}"#)
    }

    #[tokio::test]
    async fn served_feed_is_compared_by_its_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("announcements.json");
        let source = PublicFeed::File(path.clone());
        let expected = "2026-10-12T10:00:00Z";

        let check = source.check(expected, 0).await;
        assert!(matches!(check.state, PublicFeedState::Unreachable));
        assert!(check.served_updated_at.is_none());

        for (served, state) in [
            ("2026-10-12T10:00:00Z", PublicFeedState::Live),
            ("2026-10-12T09:59:59Z", PublicFeedState::Behind),
            ("2026-10-12T10:00:01Z", PublicFeedState::Different),
        ] {
            std::fs::write(&path, feed(served)).unwrap();
            let check = source.check(expected, 0).await;
            assert_eq!(
                std::mem::discriminant(&check.state),
                std::mem::discriminant(&state),
                "{served}"
            );
            assert_eq!(check.served_updated_at.as_deref(), Some(served));
        }

        std::fs::write(&path, r#"{"version":2}"#).unwrap();
        let check = source.check(expected, 0).await;
        assert!(matches!(check.state, PublicFeedState::Unreachable));
        assert!(check.error.unwrap().contains("does not validate"));
    }

    #[test]
    fn only_https_urls_are_accepted() {
        assert!(PublicFeed::url("http://tennoworth.app/announcements.json".into()).is_err());
        assert!(PublicFeed::url("https://tennoworth.app/announcements.json".into()).is_ok());
    }
}
