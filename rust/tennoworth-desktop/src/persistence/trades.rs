use super::{guard, read_allowance, write_allowance, Db, TradeRow};

impl Db {
    /// Ledger, replay protection and allowance progress commit together.
    /// None means already recorded: callers must not notify or auto-close again.
    pub fn insert_trade(
        &self,
        t: &crate::services::eelog::TradeEvent,
        at: i64,
        position: &crate::services::eelog::LogPosition,
    ) -> rusqlite::Result<Option<i64>> {
        let mut conn = guard(&self.conn);
        let tx = conn.transaction()?;
        let duplicate: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM trade_log_event WHERE session = ?1 AND end_offset = ?2)",
            (&position.session, position.end),
            |r| r.get(0),
        )?;
        if duplicate {
            return Ok(None);
        }
        let items = serde_json::to_string(&t.items).unwrap_or_else(|_| "[]".into());
        tx.execute(
            "INSERT INTO trade (at, partner, kind, plat, items, log_stamp) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (at, &t.partner, &t.kind, t.plat, items, &t.log_stamp),
        )?;
        let id = tx.last_insert_rowid();
        let raw = serde_json::to_string(position)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        tx.execute("INSERT INTO trade_log_event (session, end_offset, position, trade_id) VALUES (?1, ?2, ?3, ?4)",
            (&position.session, position.end, raw, id))?;
        if let Some(mut observation) = read_allowance(&tx)? {
            if observation.run_id == self.run_id {
                observation.reconcile(position, at);
            } else {
                observation.mark_uncertain(
                    "Monitoring restarted; scan again to confirm the account and remaining trades.",
                );
            }
            write_allowance(&tx, &observation)?;
        }
        tx.commit()?;
        Ok(Some(id))
    }

    pub fn save_allowance(
        &self,
        mut observation: crate::services::allowance::Observation,
    ) -> rusqlite::Result<()> {
        let mut conn = guard(&self.conn);
        let tx = conn.transaction()?;
        observation.run_id = self.run_id.clone();
        if let Some(before) = &observation.before {
            // A callback can commit between capturing the scan's end cursor
            // and saving the observation. Reconcile that race under the DB lock.
            let mut stmt = tx.prepare("SELECT position FROM trade_log_event WHERE session = ?1 AND end_offset > ?2 ORDER BY end_offset")?;
            let rows = stmt.query_map((&before.session, before.end), |r| r.get::<_, String>(0))?;
            for row in rows {
                let position = serde_json::from_str(&row?).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                observation.reconcile(&position, observation.observed_at);
            }
        }
        write_allowance(&tx, &observation)?;
        tx.commit()
    }

    pub fn trade_allowance(
        &self,
        now: i64,
    ) -> rusqlite::Result<crate::services::allowance::AllowanceView> {
        let conn = guard(&self.conn);
        Ok(read_allowance(&conn)?
            .map(|o| o.view(&self.run_id, now))
            .unwrap_or_else(|| crate::services::allowance::unknown(now)))
    }

    pub fn session_allowance(
        &self,
        snapshot_id: i64,
        utc_day: i64,
        now: i64,
    ) -> Result<crate::services::allowance::AllowanceView, String> {
        let conn = guard(&self.conn);
        let observation = read_allowance(&conn)
            .map_err(|e| e.to_string())?
            .ok_or("Scan the game before submitting a Trade Session batch.")?;
        let latest: Option<i64> = conn
            .query_row("SELECT MAX(id) FROM snapshot", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if observation.run_id != self.run_id
            || observation.snapshot_id != snapshot_id
            || latest != Some(snapshot_id)
        {
            return Err("Inventory or account changed, or monitoring restarted. Scan and review a new batch.".into());
        }
        let view = observation.view(&self.run_id, now);
        if view.utc_day != utc_day || utc_day != now.div_euclid(86_400) {
            return Err("The UTC allowance day changed. Review a new batch.".into());
        }
        Ok(view)
    }

    pub fn allowance_gap(&self) -> rusqlite::Result<bool> {
        let conn = guard(&self.conn);
        let Some(mut observation) = read_allowance(&conn)? else {
            return Ok(false);
        };
        if !observation.monitoring {
            return Ok(false);
        }
        observation.mark_uncertain(
            "Log monitoring was interrupted; scan again to confirm remaining trades.",
        );
        write_allowance(&conn, &observation)?;
        Ok(true)
    }

    pub fn clear_allowance(&self) -> rusqlite::Result<()> {
        guard(&self.conn).execute("DELETE FROM trade_allowance", [])?;
        Ok(())
    }

    pub fn traded_names_since_scan(&self) -> rusqlite::Result<std::collections::BTreeSet<String>> {
        let conn = guard(&self.conn);
        let Some(observation) = read_allowance(&conn)? else {
            return Ok(Default::default());
        };
        let session = observation
            .before
            .as_ref()
            .map(|p| p.session.as_str())
            .unwrap_or("");
        let end = observation.before.as_ref().map(|p| p.end).unwrap_or(0);
        let mut stmt = conn.prepare(
            "SELECT t.items FROM trade_log_event e JOIN trade t ON t.id = e.trade_id
            WHERE (e.session = ?1 AND e.end_offset > ?2) OR (e.session != ?1 AND t.at >= ?3)",
        )?;
        let rows = stmt.query_map((session, end, observation.observed_at), |r| {
            r.get::<_, String>(0)
        })?;
        let mut names = std::collections::BTreeSet::new();
        for row in rows {
            let items: Vec<crate::services::eelog::TradeItem> = serde_json::from_str(&row?)
                .map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
            names.extend(
                items
                    .into_iter()
                    .filter(|i| i.direction == "given" && i.qty > 0)
                    .map(|i| i.name.to_lowercase()),
            );
        }
        Ok(names)
    }

    pub fn mark_trade_wfm_closed(&self, id: i64) -> rusqlite::Result<()> {
        let conn = guard(&self.conn);
        conn.execute("UPDATE trade SET wfm_closed = 1 WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Newest first.
    pub fn list_trades(&self, limit: i64) -> rusqlite::Result<Vec<TradeRow>> {
        let conn = guard(&self.conn);
        let mut stmt = conn.prepare(
            "SELECT id, at, partner, kind, plat, items, log_stamp, wfm_closed FROM trade ORDER BY at DESC, id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |r| {
            let items_json: String = r.get(5)?;
            Ok(TradeRow {
                id: r.get(0)?,
                at: r.get(1)?,
                partner: r.get(2)?,
                kind: r.get(3)?,
                plat: r.get(4)?,
                items: serde_json::from_str(&items_json).unwrap_or_default(),
                log_stamp: r.get(6)?,
                wfm_closed: r.get::<_, i64>(7)? != 0,
            })
        })?;
        rows.collect()
    }
}
