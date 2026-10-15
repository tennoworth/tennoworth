use super::{guard, Db, ListedItem};

/// Listings older than this are deleted on every write, so the record cannot
/// grow without bound. The summary reads 30 days; the rest is headroom for a
/// longer comparison without keeping history nobody looks at.
pub(crate) const RETENTION_SECS: i64 = 180 * 86_400;

impl Db {
    /// Idempotent per plan item: a resumed batch reports its earlier items again.
    pub fn record_listings(&self, items: &[ListedItem], now: i64) -> rusqlite::Result<()> {
        let mut conn = guard(&self.conn);
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM listing_outcome WHERE listed_at < ?1",
            [now.saturating_sub(RETENTION_SECS)],
        )?;
        for item in items {
            tx.execute(
                "INSERT OR IGNORE INTO listing_outcome
                 (plan_id, plan_index, slug, rank, subtype, platinum, quantity, origin, listed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                (
                    &item.plan_id,
                    item.plan_index,
                    &item.slug,
                    item.rank,
                    &item.subtype,
                    item.platinum,
                    item.quantity,
                    item.origin.as_str(),
                    item.listed_at,
                ),
            )?;
        }
        tx.commit()
    }

    /// Oldest first, which is the order sales are allocated in.
    pub fn listings_since(&self, since: i64) -> rusqlite::Result<Vec<ListedItem>> {
        let conn = guard(&self.conn);
        let mut stmt = conn.prepare(
            "SELECT plan_id, plan_index, slug, rank, subtype, platinum, quantity, origin, listed_at
             FROM listing_outcome WHERE listed_at >= ?1 ORDER BY listed_at, id",
        )?;
        let rows = stmt.query_map([since], |r| {
            let origin: String = r.get(7)?;
            Ok(ListedItem {
                plan_id: r.get(0)?,
                plan_index: r.get(1)?,
                slug: r.get(2)?,
                rank: r.get(3)?,
                subtype: r.get(4)?,
                platinum: r.get(5)?,
                quantity: r.get(6)?,
                origin: if origin == "session" {
                    super::ListingOrigin::Session
                } else {
                    super::ListingOrigin::Manual
                },
                listed_at: r.get(8)?,
            })
        })?;
        rows.collect()
    }

    pub fn clear_listings(&self) -> rusqlite::Result<()> {
        guard(&self.conn).execute("DELETE FROM listing_outcome", [])?;
        Ok(())
    }

    /// Sales recorded at or after `since`, oldest first.
    pub fn sales_since(&self, since: i64) -> rusqlite::Result<Vec<super::TradeRow>> {
        self.query_trades(
            "SELECT id, at, partner, kind, plat, items, log_stamp, wfm_closed FROM trade
             WHERE kind = 'sale' AND at >= ?1 ORDER BY at, id",
            [since],
        )
    }
}
