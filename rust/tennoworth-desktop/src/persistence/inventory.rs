use super::{guard, Db, ListingLogEntry, ListingLogRow, SnapshotItem, SnapshotSummary};

impl Db {
    /// Insert a whole snapshot (header + all item rows) in ONE transaction:
    /// either every row lands or none does. A mid-insert failure (e.g. the
    /// game returned two entries resolving to the same slug, tripping the
    /// `(snapshot_id, slug)` PK) rolls the whole thing back - no orphaned header.
    /// `taken_at = None` stamps the current UTC time in SQL. Returns the new id.
    pub fn insert_snapshot(
        &self,
        source: &str,
        taken_at: Option<&str>,
        game_version: Option<&str>,
        items: &[SnapshotItem],
    ) -> rusqlite::Result<i64> {
        let mut conn = guard(&self.conn);
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO snapshot (taken_at, source, game_version)
             VALUES (COALESCE(?1, strftime('%Y-%m-%dT%H:%M:%SZ','now')), ?2, ?3)",
            (taken_at, source, game_version),
        )?;
        let snapshot_id = tx.last_insert_rowid();
        {
            let mut stmt = tx.prepare(
                "INSERT INTO snapshot_item (snapshot_id, slug, count, leveled)
                 VALUES (?1, ?2, ?3, ?4)",
            )?;
            for it in items {
                stmt.execute((snapshot_id, &it.slug, it.count, it.leveled))?;
            }
        }
        tx.commit()?;
        Ok(snapshot_id)
    }

    /// Append one plan run's items to `listing_log`. One transaction per run,
    /// so a partial write can't leave half a batch recorded.
    ///
    /// `listed_at` is the DB's clock rather than the caller's: these rows are
    /// compared against each other over time, and one consistent clock is worth
    /// more here than matching whatever the plan started at.
    pub fn insert_listing_log(
        &self,
        plan_id: &str,
        rows: &[ListingLogRow],
    ) -> rusqlite::Result<usize> {
        if rows.is_empty() {
            return Ok(0);
        }
        let mut conn = guard(&self.conn);
        let tx = conn.transaction()?;
        let mut written = 0;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO listing_log
                   (plan_id, slug, listed_at, price, qty, status, action, order_id, message, plan_index)
                 VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(plan_id, plan_index) DO UPDATE SET
                   price=excluded.price, qty=excluded.qty, status=excluded.status,
                   action=excluded.action, order_id=excluded.order_id, message=excluded.message",
            )?;
            for (index, r) in rows.iter().enumerate() {
                if matches!(r.status.as_str(), "pending" | "uncertain_mutation") { continue; }
                written += stmt.execute((
                    plan_id,
                    &r.slug,
                    r.price,
                    r.qty,
                    &r.status,
                    &r.action,
                    &r.order_id,
                    &r.message,
                    i64::try_from(index).unwrap_or(i64::MAX),
                ))?;
            }
        }
        tx.commit()?;
        Ok(written)
    }

    /// Most recent `listing_log` rows, newest first.
    pub fn list_listing_log(&self, limit: i64) -> rusqlite::Result<Vec<ListingLogEntry>> {
        let conn = guard(&self.conn);
        let mut stmt = conn.prepare(
            "SELECT id, plan_id, slug, listed_at, price, qty, status, action, order_id,
                    message, outcome
               FROM listing_log
              ORDER BY id DESC
              LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |r| {
            Ok(ListingLogEntry {
                id: r.get(0)?,
                plan_id: r.get(1)?,
                slug: r.get(2)?,
                listed_at: r.get(3)?,
                price: r.get(4)?,
                qty: r.get(5)?,
                status: r.get(6)?,
                action: r.get(7)?,
                order_id: r.get(8)?,
                message: r.get(9)?,
                outcome: r.get(10)?,
            })
        })?;
        rows.collect()
    }

    /// The item rows of the most recent snapshot (highest id), or an empty vec
    /// when no snapshot exists yet. `slug` is the DE item path - the caller
    /// resolves it to a WFM slug for the market join (see `sellables`).
    pub fn latest_snapshot_items(&self) -> rusqlite::Result<Vec<SnapshotItem>> {
        let conn = guard(&self.conn);
        let mut stmt = conn.prepare(
            "SELECT slug, count, leveled FROM snapshot_item
             WHERE snapshot_id = (SELECT MAX(id) FROM snapshot)
             ORDER BY slug",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(SnapshotItem {
                slug: r.get(0)?,
                count: r.get(1)?,
                leveled: r.get(2)?,
            })
        })?;
        rows.collect()
    }

    pub fn list_snapshots(&self, limit: i64) -> rusqlite::Result<Vec<SnapshotSummary>> {
        let conn = guard(&self.conn);
        let mut stmt = conn.prepare(
            "SELECT s.id, s.taken_at, s.source, COUNT(si.snapshot_id)
             FROM snapshot s
             LEFT JOIN snapshot_item si ON si.snapshot_id = s.id
             GROUP BY s.id
             ORDER BY s.id DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |r| {
            Ok(SnapshotSummary {
                id: r.get(0)?,
                taken_at: r.get(1)?,
                source: r.get(2)?,
                item_count: r.get(3)?,
            })
        })?;
        rows.collect()
    }
}
