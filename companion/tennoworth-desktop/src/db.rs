//! Canonical desktop state store (SQLite via rusqlite `bundled`). This is the
//! single owner of the schema, the migration runner, and every SQL statement —
//! call sites use the typed methods below and never write raw SQL. The schema is
//! the one agreed in docs/product-plan-2026-07.md (C3), applied verbatim as the
//! v1 migration.
//!
//! Two distinct concerns share this file:
//!   - inventory HISTORY (`snapshot` / `snapshot_item`, plus `listing_log`) —
//!     the profit-tracking substrate, appended from day one.
//!   - app STATE (`setting` kv, `reserve` per-slug) — the desktop backing for
//!     the persistence the browser keeps in localStorage.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension};

/// Schema migrations, applied in order. The index (1-based) is the schema
/// version each one brings the DB to; `user_version` records the current level.
/// v1 is the plan's C3 schema, verbatim.
const MIGRATIONS: &[&str] = &[
    // v1 — initial schema.
    r#"
CREATE TABLE snapshot (
  id INTEGER PRIMARY KEY,
  taken_at TEXT NOT NULL,            -- ISO8601 UTC
  source TEXT NOT NULL CHECK(source IN ('memory','import')),
  game_version TEXT
);
CREATE TABLE snapshot_item (
  snapshot_id INTEGER NOT NULL REFERENCES snapshot(id),
  slug TEXT NOT NULL,                -- resolved item slug
  count INTEGER NOT NULL,
  leveled INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (snapshot_id, slug)
);
CREATE TABLE setting (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE reserve (slug TEXT PRIMARY KEY, keep INTEGER NOT NULL);
CREATE TABLE listing_log (            -- what we listed, when, at what price
  id INTEGER PRIMARY KEY,
  slug TEXT NOT NULL, listed_at TEXT NOT NULL,
  price INTEGER NOT NULL, qty INTEGER NOT NULL,
  outcome TEXT                        -- NULL until sold/cancelled observed
);
"#,
];

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

/// The open database. `Connection` is not `Sync`, so it lives behind a `Mutex`;
/// held as Tauri managed state (`State<'_, Db>`) and shared across commands.
/// Scans are already single-flighted upstream, so lock contention is a non-issue.
pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    /// Open (creating if absent) the store at `path` and bring it to the latest
    /// schema version. Fails only on a genuine I/O / corruption problem — the
    /// desktop treats that as unrecoverable (the store is canonical).
    pub fn open(path: &Path) -> rusqlite::Result<Db> {
        Self::init(Connection::open(path)?)
    }

    fn init(conn: Connection) -> rusqlite::Result<Db> {
        // FK enforcement is per-connection (not persisted); turn it on so a
        // snapshot_item can never dangle without its snapshot.
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrate(&conn)?;
        Ok(Db {
            conn: Mutex::new(conn),
        })
    }

    pub fn get_setting(&self, key: &str) -> rusqlite::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT value FROM setting WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO setting (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            (key, value),
        )?;
        Ok(())
    }

    pub fn get_reserves(&self) -> rusqlite::Result<Vec<Reserve>> {
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO reserve (slug, keep) VALUES (?1, ?2)
             ON CONFLICT(slug) DO UPDATE SET keep = excluded.keep",
            (slug, keep),
        )?;
        Ok(())
    }

    pub fn delete_reserve(&self, slug: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM reserve WHERE slug = ?1", [slug])?;
        Ok(())
    }

    /// Insert a whole snapshot (header + all item rows) in ONE transaction:
    /// either every row lands or none does. A mid-insert failure (e.g. the
    /// game returned two entries resolving to the same slug, tripping the
    /// `(snapshot_id, slug)` PK) rolls the whole thing back — no orphaned header.
    /// `taken_at = None` stamps the current UTC time in SQL. Returns the new id.
    pub fn insert_snapshot(
        &self,
        source: &str,
        taken_at: Option<&str>,
        game_version: Option<&str>,
        items: &[SnapshotItem],
    ) -> rusqlite::Result<i64> {
        let mut conn = self.conn.lock().unwrap();
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

    pub fn list_snapshots(&self, limit: i64) -> rusqlite::Result<Vec<SnapshotSummary>> {
        let conn = self.conn.lock().unwrap();
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

    #[cfg(test)]
    fn open_in_memory() -> rusqlite::Result<Db> {
        Self::init(Connection::open_in_memory()?)
    }

    #[cfg(test)]
    fn snapshot_count(&self) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM snapshot", [], |r| r.get(0))
    }

    #[cfg(test)]
    fn snapshot_item_count(&self) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM snapshot_item", [], |r| r.get(0))
    }

    #[cfg(test)]
    fn user_version(&self) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("PRAGMA user_version", [], |r| r.get(0))
    }
}

/// Bring `conn` up to the latest schema version, applying only the migrations
/// past the current `user_version`. Idempotent: re-running on an up-to-date DB
/// applies nothing.
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        let version = (i + 1) as i64;
        if current < version {
            conn.execute_batch(sql)?;
            // pragma_update won't bind `user_version` as a parameter — it's part
            // of the statement text — so format it in (it's our own integer).
            conn.pragma_update(None, "user_version", version)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db_path() -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("tennoworth-db-test-{}-{}.db", std::process::id(), nanos))
    }

    #[test]
    fn migration_creates_the_full_schema_at_v1() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.user_version().unwrap(), 1);
        let conn = db.conn.lock().unwrap();
        let mut names: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        names.sort();
        assert_eq!(
            names,
            vec![
                "listing_log".to_string(),
                "reserve".to_string(),
                "setting".to_string(),
                "snapshot".to_string(),
                "snapshot_item".to_string(),
            ]
        );
    }

    #[test]
    fn migration_runner_is_idempotent_across_reopen() {
        let path = temp_db_path();
        {
            let db = Db::open(&path).unwrap();
            db.set_setting("k", "v").unwrap();
            assert_eq!(db.user_version().unwrap(), 1);
        }
        // Reopen: migrate() runs again but must apply nothing and preserve data.
        {
            let db = Db::open(&path).unwrap();
            assert_eq!(db.user_version().unwrap(), 1);
            assert_eq!(db.get_setting("k").unwrap().as_deref(), Some("v"));
        }
        // Running the runner directly a second time on a live conn is a no-op.
        {
            let db = Db::open(&path).unwrap();
            let conn = db.conn.lock().unwrap();
            migrate(&conn).unwrap();
            drop(conn);
            assert_eq!(db.user_version().unwrap(), 1);
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn settings_upsert_and_read() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.get_setting("view").unwrap(), None);
        db.set_setting("view", "sell").unwrap();
        assert_eq!(db.get_setting("view").unwrap().as_deref(), Some("sell"));
        db.set_setting("view", "relics").unwrap();
        assert_eq!(db.get_setting("view").unwrap().as_deref(), Some("relics"));
    }

    #[test]
    fn reserve_crud() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.get_reserves().unwrap().is_empty());
        db.set_reserve("vitality", 2).unwrap();
        db.set_reserve("serration", 1).unwrap();
        let got = db.get_reserves().unwrap();
        assert_eq!(got.len(), 2);
        // ORDER BY slug → serration before vitality.
        assert_eq!(got[0].slug, "serration");
        assert_eq!(got[0].keep, 1);
        // Upsert overwrites keep, doesn't duplicate.
        db.set_reserve("vitality", 5).unwrap();
        let got = db.get_reserves().unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got.iter().find(|r| r.slug == "vitality").unwrap().keep, 5);
        db.delete_reserve("serration").unwrap();
        let got = db.get_reserves().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].slug, "vitality");
        // Deleting a missing slug is a no-op, not an error.
        db.delete_reserve("nonexistent").unwrap();
        assert_eq!(db.get_reserves().unwrap().len(), 1);
    }

    #[test]
    fn snapshot_insert_and_list() {
        let db = Db::open_in_memory().unwrap();
        let items = vec![
            SnapshotItem { slug: "/Lotus/A".into(), count: 3, leveled: 0 },
            SnapshotItem { slug: "/Lotus/B".into(), count: 1, leveled: 1 },
        ];
        let id = db
            .insert_snapshot("memory", None, Some("40.1.2"), &items)
            .unwrap();
        assert!(id > 0);
        let list = db.list_snapshots(10).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, id);
        assert_eq!(list[0].source, "memory");
        assert_eq!(list[0].item_count, 2);
        // taken_at is a real ISO8601 UTC stamp (…T…Z), not empty.
        assert!(list[0].taken_at.contains('T') && list[0].taken_at.ends_with('Z'));

        // A second, explicit-time import snapshot; newest first.
        db.insert_snapshot(
            "import",
            Some("2020-01-01T00:00:00Z"),
            None,
            &[SnapshotItem { slug: "/Lotus/C".into(), count: 9, leveled: 0 }],
        )
        .unwrap();
        let list = db.list_snapshots(10).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].source, "import"); // higher id, listed first
        assert_eq!(list[0].item_count, 1);
    }

    #[test]
    fn snapshot_is_transactional_on_mid_insert_failure() {
        let db = Db::open_in_memory().unwrap();
        // Two items with the SAME slug → the second snapshot_item insert trips
        // the (snapshot_id, slug) primary key mid-transaction.
        let dup = vec![
            SnapshotItem { slug: "/Lotus/Dup".into(), count: 1, leveled: 0 },
            SnapshotItem { slug: "/Lotus/Dup".into(), count: 2, leveled: 0 },
        ];
        assert!(db.insert_snapshot("memory", None, None, &dup).is_err());
        // Whole snapshot rolled back: no header row, no item rows.
        assert_eq!(db.snapshot_count().unwrap(), 0);
        assert_eq!(db.snapshot_item_count().unwrap(), 0);

        // A bad `source` trips the CHECK on the header insert itself.
        assert!(db
            .insert_snapshot("bogus", None, None, &[])
            .is_err());
        assert_eq!(db.snapshot_count().unwrap(), 0);

        // The store is still usable afterwards — a good insert lands.
        db.insert_snapshot(
            "memory",
            None,
            None,
            &[SnapshotItem { slug: "/Lotus/Ok".into(), count: 1, leveled: 0 }],
        )
        .unwrap();
        assert_eq!(db.snapshot_count().unwrap(), 1);
    }
}
