use announcement_feed::{Announcement, AnnouncementFeed, OfficialEvent};
use serde::{Deserialize, Serialize};
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
    /// Set instead of `content` for the official-event operations.
    pub event: Option<EventContent>,
}

/// An official DE date as the maintainer enters it; the publisher assigns the ID.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct EventContent {
    pub kind: String,
    pub title: String,
    pub starts_at: String,
    pub ends_at: Option<String>,
    pub source_url: String,
    pub note: Option<String>,
}

impl EventContent {
    pub fn event(&self, id: &str) -> OfficialEvent {
        OfficialEvent {
            id: id.into(),
            kind: self.kind.clone(),
            title: self.title.clone(),
            starts_at: self.starts_at.clone(),
            ends_at: self.ends_at.clone(),
            source_url: self.source_url.clone(),
            note: self.note.clone().filter(|note| !note.trim().is_empty()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum EventOperation {
    Publish,
    Remove,
}

/// Publishes a new or corrected official event straight into the feed, or removes one.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct EventChange {
    pub request_id: String,
    pub expected_revision: i64,
    pub operation: EventOperation,
    pub id: Option<String>,
    pub event: Option<EventContent>,
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

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PublicFeedState {
    /// Visitors receive the newest published revision.
    Live,
    /// Visitors still receive an older feed, usually until caches expire.
    Behind,
    /// The served feed is newer than anything this publisher published.
    Different,
    /// The public feed could not be fetched or did not validate.
    Unreachable,
}

#[derive(Debug, Serialize, TS)]
pub struct PublicFeedCheck {
    pub url: String,
    pub checked_at: String,
    pub expected_updated_at: String,
    pub served_updated_at: Option<String>,
    pub state: PublicFeedState,
    pub error: Option<String>,
}

/// What the box last reported about one systemd unit. Times are UTC RFC 3339.
#[derive(Clone, Debug, Serialize, TS)]
pub struct UnitStatus {
    pub unit: String,
    /// False when systemd does not know the unit, i.e. it is not installed.
    pub installed: bool,
    pub active_state: String,
    pub sub_state: String,
    pub result: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    Ok,
    Failing,
}

#[derive(Debug, Serialize, TS)]
pub struct ServiceStatus {
    pub label: String,
    pub service: UnitStatus,
    /// Started by a timer and expected to be idle between runs, rather than always running.
    pub scheduled: bool,
    /// When its timer fires next; absent for long-running services.
    pub next_run: Option<String>,
    /// The loopback health probe, for services that answer one.
    pub health: Option<Health>,
}

/// The parts of the hourly host-check report the dashboard shows. A field the
/// report did not carry stays absent rather than reading as zero.
#[derive(Debug, Serialize, TS)]
pub struct HostCheck {
    pub ready: bool,
    pub checked_at: String,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub csv_rows: Option<i64>,
    pub snapshot_age_seconds: Option<i64>,
    pub deploy_revision: Option<String>,
    pub checkout_head: Option<String>,
    pub deploy_lag: Option<String>,
    pub pending_scraper_commits: Option<i64>,
    pub disk_free_bytes: Option<i64>,
    pub disk_floor_bytes: Option<i64>,
}

#[derive(Debug, Serialize, TS)]
pub struct AlertEntry {
    pub at: String,
    pub unit: String,
    pub result: String,
    pub context: Vec<String>,
}

#[derive(Debug, Serialize, TS)]
pub struct BackupStatus {
    pub latest_at: Option<String>,
    pub count: i64,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, TS)]
pub struct JobRequest {
    pub at: String,
    pub actor: String,
}

#[derive(Debug, Serialize, TS)]
pub struct JobStatus {
    pub id: String,
    pub label: String,
    pub description: String,
    pub unit: UnitStatus,
    /// Why the job cannot be requested right now; absent when it can.
    pub blocked: Option<String>,
    pub last_request: Option<JobRequest>,
    pub cooldown_until: Option<String>,
}

#[derive(Debug, Serialize, TS)]
pub struct BoxStatus {
    pub collected_at: String,
    /// Sources the publisher could not read; the rest of the page still loads.
    pub problems: Vec<String>,
    pub host_check: Option<HostCheck>,
    pub services: Vec<ServiceStatus>,
    pub failed_units: Vec<String>,
    pub alerts: Vec<AlertEntry>,
    pub backup: BackupStatus,
    pub web_bundle: Option<String>,
    pub jobs: Vec<JobStatus>,
}

#[derive(Serialize, TS)]
pub struct StatusState {
    pub actor: String,
    pub status: BoxStatus,
}

#[derive(Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct RunJob {
    pub job: String,
}

#[cfg(test)]
#[test]
fn publisher_bindings_match_rust() {
    let mut types = market_domain::bindings::TypeScript::default();
    types.add::<DashboardState>();
    types.add::<Change>();
    types.add::<EventChange>();
    types.add::<StatusState>();
    types.add::<RunJob>();
    types.add::<PublicFeedCheck>();
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
