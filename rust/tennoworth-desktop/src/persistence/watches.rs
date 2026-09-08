use super::{guard, Db, NewWatch, Watch};

impl Db {
    pub fn list_watches(&self) -> rusqlite::Result<Vec<Watch>> {
        let conn = guard(&self.conn);
        let mut stmt = conn.prepare(
            "SELECT id, slug, name, subtype, rank, side, threshold, created_at,
                    last_price, last_checked_at, last_fired_at
             FROM watch ORDER BY created_at DESC, id DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Watch {
                id: r.get(0)?,
                slug: r.get(1)?,
                name: r.get(2)?,
                subtype: r.get(3)?,
                rank: r.get(4)?,
                side: r.get(5)?,
                threshold: r.get(6)?,
                created_at: r.get(7)?,
                last_price: r.get(8)?,
                last_checked_at: r.get(9)?,
                last_fired_at: r.get(10)?,
            })
        })?;
        rows.collect()
    }

    /// Insert a watch; returns its id. `created_at = None` stamps now (UTC) in SQL.
    pub fn add_watch(&self, w: &NewWatch, created_at: Option<&str>) -> rusqlite::Result<i64> {
        let conn = guard(&self.conn);
        conn.execute(
            "INSERT INTO watch (slug, name, subtype, rank, side, threshold, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, COALESCE(?7, strftime('%Y-%m-%dT%H:%M:%SZ','now')))",
            (
                &w.slug,
                &w.name,
                &w.subtype,
                w.rank,
                &w.side,
                w.threshold,
                created_at,
            ),
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn delete_watch(&self, id: i64) -> rusqlite::Result<()> {
        let conn = guard(&self.conn);
        conn.execute("DELETE FROM watch WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Record what a check saw (unix seconds). `fired_at` is set only when it
    /// notified.
    pub fn record_watch_check(
        &self,
        id: i64,
        last_price: Option<i64>,
        checked_at: i64,
        fired_at: Option<i64>,
    ) -> rusqlite::Result<()> {
        let conn = guard(&self.conn);
        conn.execute(
            "UPDATE watch SET last_price = ?2, last_checked_at = ?3,
                    last_fired_at = COALESCE(?4, last_fired_at)
             WHERE id = ?1",
            (id, last_price, checked_at, fired_at),
        )?;
        Ok(())
    }
}
