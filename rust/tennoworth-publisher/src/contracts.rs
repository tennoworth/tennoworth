use serde::{Deserialize, Serialize};
use tennoworth_announcements::{Announcement, AnnouncementFeed};
use ts_rs::TS;

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Content {
    pub title: String,
    pub body: String,
    pub source_url: String,
    pub starts_at: String,
    pub ends_at: String,
}

impl Content {
    pub fn message(&self, id: &str) -> Announcement {
        Announcement {
            id: id.into(),
            title: self.title.clone(),
            body: self.body.clone(),
            source_url: self.source_url.clone(),
            starts_at: self.starts_at.clone(),
            ends_at: self.ends_at.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Save,
    Publish,
    Withdraw,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub request_id: String,
    pub expected_revision: i64,
    pub operation: Operation,
    pub id: Option<String>,
    pub content: Option<Content>,
}

#[derive(Debug, Serialize, TS)]
pub struct Message {
    pub id: String,
    pub draft: Content,
    pub published: Option<Content>,
    pub ever_published: bool,
}

#[derive(Debug, Serialize, TS)]
pub struct History {
    pub revision: i64,
    pub actor: String,
    pub at: String,
    pub operation: String,
    pub id: Option<String>,
    pub content: Option<Content>,
}

#[derive(Debug, Serialize, TS)]
pub struct Snapshot {
    pub revision: i64,
    pub feed_revision: i64,
    pub exported_revision: i64,
    pub feed: AnnouncementFeed,
    pub messages: Vec<Message>,
    pub history: Vec<History>,
}

#[derive(Serialize, TS)]
pub struct DashboardState {
    pub actor: String,
    pub snapshot: Snapshot,
    /// The announcement the answered change created or edited; absent on a plain read.
    pub message_id: Option<String>,
}

#[cfg(test)]
#[test]
fn publisher_bindings_match_rust() {
    let mut types = market_domain::bindings::TypeScript::default();
    types.add::<DashboardState>();
    types.add::<Change>();
    let expected = types.finish().expect("contracts");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../frontend/src/contracts/generated/publisher.ts");
    if std::env::var_os("TENNOWORTH_UPDATE_BINDINGS").is_some() {
        std::fs::write(&path, &expected).expect("write contracts");
    }
    assert_eq!(
        std::fs::read_to_string(path).expect("generate publisher contracts"),
        expected
    );
}
