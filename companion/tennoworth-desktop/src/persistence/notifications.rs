use super::{guard, Db};
use rusqlite::OptionalExtension;

impl Db {
    pub fn notification_preferences(
        &self,
    ) -> Result<crate::services::notifications::Preferences, String> {
        self.get_setting("notifications-v1")
            .map_err(|e| e.to_string())?
            .map(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
            .unwrap_or_else(|| Ok(Default::default()))
    }

    pub fn list_notifications(
        &self,
    ) -> rusqlite::Result<Vec<crate::services::notifications::Notification>> {
        let conn = guard(&self.conn);
        let mut stmt = conn.prepare("SELECT id, category, title, body, target, created_at, read, delivery FROM notification ORDER BY id DESC LIMIT 1000")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(crate::services::notifications::Notification {
                    id: r.get(0)?,
                    category: r.get(1)?,
                    title: r.get(2)?,
                    body: r.get(3)?,
                    target: r.get(4)?,
                    created_at: r.get(5)?,
                    read: r.get(6)?,
                    delivery: r.get(7)?,
                })
            })?
            .collect();
        rows
    }

    pub fn mark_notifications_read(&self, id: Option<i64>) -> rusqlite::Result<()> {
        guard(&self.conn).execute(
            "UPDATE notification SET read = 1 WHERE (?1 IS NULL OR id = ?1)",
            [id],
        )?;
        Ok(())
    }

    pub fn clear_notifications(&self) -> rusqlite::Result<()> {
        guard(&self.conn).execute("DELETE FROM notification", [])?;
        Ok(())
    }

    pub fn notification_delivery(&self, id: i64, status: &str) -> rusqlite::Result<()> {
        guard(&self.conn).execute(
            "UPDATE notification SET delivery = ?2 WHERE id = ?1",
            rusqlite::params![id, status],
        )?;
        Ok(())
    }

    pub fn prune_notifications(&self, now: i64) -> rusqlite::Result<()> {
        let conn = guard(&self.conn);
        conn.execute("DELETE FROM notification WHERE created_at < ?1 OR id NOT IN (SELECT id FROM notification ORDER BY id DESC LIMIT 1000)", [now - 30 * 86400])?;
        conn.execute(
            "DELETE FROM notification_checkpoint WHERE expires_at < ?1",
            [now],
        )?;
        Ok(())
    }

    /// The checkpoint and inbox insert share a transaction: polling and streaming
    /// cannot both claim the same watch, even if they evaluated stale copies.
    pub fn insert_notification(
        &self,
        n: &crate::services::notifications::Candidate,
        now: i64,
        native: bool,
        enabled: bool,
    ) -> rusqlite::Result<Option<i64>> {
        let mut conn = guard(&self.conn);
        let tx = conn.transaction()?;
        let prior: Option<(i64, i64)> = tx
            .query_row(
                "SELECT stage, at FROM notification_checkpoint WHERE key = ?1",
                [&n.key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if prior.is_some_and(|(stage, at)| {
            if n.cooldown > 0 {
                now.saturating_sub(at) < n.cooldown
            } else {
                stage >= n.stage
            }
        }) {
            return Ok(None);
        }
        tx.execute("INSERT INTO notification_checkpoint(key, stage, at, expires_at) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(key) DO UPDATE SET stage=excluded.stage, at=excluded.at, expires_at=excluded.expires_at",
            rusqlite::params![n.key, n.stage, now, n.expires_at])?;
        let id = if enabled {
            tx.execute("INSERT INTO notification(category, title, body, target, created_at, delivery) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![n.category, n.title, n.body, n.target, now, if native { "pending" } else { "inbox_only" }])?;
            Some(tx.last_insert_rowid())
        } else {
            None
        };
        tx.commit()?;
        Ok(id)
    }
}
