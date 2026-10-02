/// A ledger row, as handed to the SPA.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct TradeRow {
    pub id: i64,
    pub at: i64,
    pub partner: String,
    pub kind: String,
    pub plat: i64,
    pub items: Vec<crate::trading_contract::TradeItem>,
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
