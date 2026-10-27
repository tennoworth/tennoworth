use super::{guard, Db};
use rusqlite::OptionalExtension;

impl Db {
    pub fn get_setting(&self, key: &str) -> rusqlite::Result<Option<String>> {
        let conn = guard(&self.conn);
        conn.query_row("SELECT value FROM setting WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        let conn = guard(&self.conn);
        conn.execute(
            "INSERT INTO setting (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            (key, value),
        )?;
        Ok(())
    }
}
