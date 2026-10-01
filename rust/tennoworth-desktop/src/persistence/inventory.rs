use super::{guard, Db, SnapshotItem, SnapshotSummary};

impl Db {
    /// Insert a whole snapshot (header + all item rows) in ONE transaction:
    /// either every row lands or none does. A mid-insert failure (e.g. the
    /// game returned two entries resolving to the same slug, tripping the
    /// `(snapshot_id, slug)` PK) rolls the whole thing back - no orphaned header.
    /// `taken_at = None` stamps the current UTC time in SQL. Returns the new id.
    /// Production records through [`Db::record_snapshot`]; tests use this to
    /// place snapshots at explicit times.
    #[cfg(test)]
    pub fn insert_snapshot(
        &self,
        source: &str,
        taken_at: Option<&str>,
        game_version: Option<&str>,
        items: &[SnapshotItem],
    ) -> rusqlite::Result<i64> {
        let mut conn = guard(&self.conn);
        let tx = conn.transaction()?;
        let snapshot_id = insert_snapshot_rows(&tx, source, taken_at, game_version, items)?;
        tx.commit()?;
        Ok(snapshot_id)
    }

    /// Record a scan or import, reusing the latest snapshot when this one holds
    /// exactly the same items from the same source. Returns the snapshot id.
    ///
    /// Each snapshot is ~2.6k rows (~400 KB), and a game left open with automatic
    /// scanning repeats the same inventory every 15-60 minutes - half the local
    /// history was duplicates. A repeat only moves the reused snapshot's
    /// `taken_at` to now, so it still reads as "last confirmed" to reminder
    /// freshness, and a change is bracketed by the previous snapshot's last
    /// confirmation and the new one's first. The id stays the same, which is
    /// right for the latest-snapshot gates: the inventory a batch was reviewed
    /// against has not changed. Any difference - one count - inserts a new one.
    pub fn record_snapshot(
        &self,
        source: &str,
        game_version: Option<&str>,
        items: &[SnapshotItem],
    ) -> rusqlite::Result<i64> {
        let mut conn = guard(&self.conn);
        let tx = conn.transaction()?;
        let latest: Option<i64> = tx.query_row(
            "SELECT MAX(id) FROM snapshot WHERE id = (SELECT MAX(id) FROM snapshot) AND source = ?1",
            [source],
            |r| r.get(0),
        )?;
        if let Some(id) = latest {
            let stored = {
                let mut stmt = tx.prepare(
                    "SELECT slug, count, leveled FROM snapshot_item WHERE snapshot_id = ?1 ORDER BY slug",
                )?;
                let rows = stmt.query_map([id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
                })?;
                rows.collect::<rusqlite::Result<Vec<_>>>()?
            };
            let mut scanned: Vec<_> = items
                .iter()
                .map(|it| (it.slug.clone(), it.count, it.leveled))
                .collect();
            scanned.sort();
            if stored == scanned {
                tx.execute(
                    "UPDATE snapshot SET taken_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                     game_version = COALESCE(?2, game_version) WHERE id = ?1",
                    (id, game_version),
                )?;
                tx.commit()?;
                return Ok(id);
            }
        }
        let snapshot_id = insert_snapshot_rows(&tx, source, None, game_version, items)?;
        tx.commit()?;
        Ok(snapshot_id)
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

/// The snapshot header and its item rows, inside the caller's transaction so a
/// failing row (a duplicate slug trips the primary key) leaves no header behind.
fn insert_snapshot_rows(
    tx: &rusqlite::Transaction<'_>,
    source: &str,
    taken_at: Option<&str>,
    game_version: Option<&str>,
    items: &[SnapshotItem],
) -> rusqlite::Result<i64> {
    tx.execute(
        "INSERT INTO snapshot (taken_at, source, game_version)
         VALUES (COALESCE(?1, strftime('%Y-%m-%dT%H:%M:%SZ','now')), ?2, ?3)",
        (taken_at, source, game_version),
    )?;
    let snapshot_id = tx.last_insert_rowid();
    let mut stmt = tx.prepare(
        "INSERT INTO snapshot_item (snapshot_id, slug, count, leveled)
         VALUES (?1, ?2, ?3, ?4)",
    )?;
    for it in items {
        stmt.execute((snapshot_id, &it.slug, it.count, it.leveled))?;
    }
    Ok(snapshot_id)
}
