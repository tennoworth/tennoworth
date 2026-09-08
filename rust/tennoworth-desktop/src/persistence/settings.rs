use super::{guard, Db, Reserve};
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

    pub fn get_reserves(&self) -> rusqlite::Result<Vec<Reserve>> {
        let conn = guard(&self.conn);
        let mut stmt = conn.prepare("SELECT slug, keep FROM reserve ORDER BY slug")?;
        let rows = stmt.query_map([], |r| {
            Ok(Reserve {
                slug: r.get(0)?,
                keep: r.get(1)?,
            })
        })?;
        rows.collect()
    }

    pub fn set_reserve(&self, slug: &str, keep: i64) -> rusqlite::Result<()> {
        let conn = guard(&self.conn);
        conn.execute(
            "INSERT INTO reserve (slug, keep) VALUES (?1, ?2)
             ON CONFLICT(slug) DO UPDATE SET keep = excluded.keep",
            (slug, keep),
        )?;
        Ok(())
    }

    pub fn delete_reserve(&self, slug: &str) -> rusqlite::Result<()> {
        let conn = guard(&self.conn);
        conn.execute("DELETE FROM reserve WHERE slug = ?1", [slug])?;
        Ok(())
    }
}
