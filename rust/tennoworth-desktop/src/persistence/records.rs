/// A ledger row, as handed to the SPA.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct TradeRow {
    pub id: i64,
    pub at: i64,
    pub partner: String,
    pub kind: String,
    pub plat: i64,
    pub items: Vec<crate::services::eelog::TradeItem>,
    pub log_stamp: Option<String>,
    pub wfm_closed: bool,
}

/// A price watch, as stored and as handed to the SPA.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct Watch {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub subtype: Option<String>,
    pub rank: Option<i64>,
    /// 'sell' = watch the lowest ask (fires at or below threshold);
    /// 'buy' = watch the highest bid (fires at or above threshold).
    pub side: String,
    pub threshold: i64,
    pub created_at: String,
    pub last_price: Option<i64>,
    /// Unix seconds.
    pub last_checked_at: Option<i64>,
    /// Unix seconds.
    pub last_fired_at: Option<i64>,
}

/// What the SPA sends to create a watch.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct NewWatch {
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub subtype: Option<String>,
    #[serde(default)]
    pub rank: Option<i64>,
    pub side: String,
    pub threshold: i64,
}

/// One aggregated inventory row for a snapshot: `slug` is the DE item path
/// (`/Lotus/...`), `count` the total owned, `leveled` the number of owned copies
/// DE has flagged untradeable (XP > 0). See `snapshot::extract_items`.
pub struct SnapshotItem {
    pub slug: String,
    pub count: i64,
    pub leveled: i64,
}

/// A per-slug reserve ("keep N copies of this item"). Serialized to the SPA.
#[derive(serde::Serialize)]
pub struct Reserve {
    pub slug: String,
    pub keep: i64,
}

/// A row for the snapshot-history list. `item_count` is the number of
/// `snapshot_item` rows joined to this snapshot.
#[derive(serde::Serialize)]
pub struct SnapshotSummary {
    pub id: i64,
    pub taken_at: String,
    pub source: String,
    pub item_count: i64,
}

/// What to record for one item of a plan run. Built by the listing command
/// layer from the plan's own items joined to their results - wfm-core stays
/// storage-free, so the DB write happens here rather than inside the executor.
pub struct ListingLogRow {
    pub slug: String,
    pub price: i64,
    pub qty: i64,
    /// "ok" | "skipped" | "error", verbatim from the plan result.
    pub status: String,
    /// "created" | "updated" on success, None otherwise.
    pub action: Option<String>,
    pub order_id: Option<String>,
    /// WFM's own error text on failure - the evidence that used to be lost.
    pub message: Option<String>,
}

/// A stored `listing_log` row, as handed to the SPA.
#[derive(serde::Serialize)]
pub struct ListingLogEntry {
    pub id: i64,
    pub plan_id: Option<String>,
    pub slug: String,
    pub listed_at: String,
    pub price: i64,
    pub qty: i64,
    pub status: String,
    pub action: Option<String>,
    pub order_id: Option<String>,
    pub message: Option<String>,
    /// NULL until a later orders-diff observes the listing sold or cancelled.
    pub outcome: Option<String>,
}
