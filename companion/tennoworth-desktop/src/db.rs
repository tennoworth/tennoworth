//! Canonical desktop state store (SQLite via rusqlite `bundled`). This is the
//! single owner of the schema, the migration runner, and every SQL statement —
//! call sites use the typed methods below and never write raw SQL. The schema is
//! the one agreed in the product plan (C3), applied verbatim as the
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
use wfm_core::poison::guard;

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
    // v2 — make listing_log actually recordable.
    //
    // v1 shipped the table and nothing ever wrote a row, so every plan's
    // evidence died with the modal that displayed it. Writing it needs four
    // things v1 has no column for:
    //   plan_id  — groups the items of one batch, so a partial run reads as
    //              one event instead of N unrelated rows.
    //   status   — the point of the log. An 'error' row IS the record of what
    //              went wrong; without it only successes are representable.
    //   action   — created vs updated (the duplicate-listing reconcile path).
    //   order_id — the join key for observing `outcome` later: diff these
    //              against a later GET /orders and a vanished id means sold or
    //              cancelled. Nothing does that yet; this is what makes it
    //              possible without a second migration.
    //
    // status defaults to 'ok' only to satisfy NOT NULL on the zero pre-existing
    // rows; every insert passes it explicitly.
    r#"
ALTER TABLE listing_log ADD COLUMN plan_id TEXT;
ALTER TABLE listing_log ADD COLUMN status TEXT NOT NULL DEFAULT 'ok';
ALTER TABLE listing_log ADD COLUMN action TEXT;
ALTER TABLE listing_log ADD COLUMN order_id TEXT;
ALTER TABLE listing_log ADD COLUMN message TEXT;
CREATE INDEX listing_log_slug_at ON listing_log(slug, listed_at);
CREATE INDEX listing_log_order ON listing_log(order_id);
"#,
    // v3 — price watches. One row per "tell me when": `side` names which side
    // of the book is watched — 'sell' fires when the lowest online ASK drops
    // to `threshold` or below (a buying opportunity), 'buy' fires when the
    // highest online BID reaches `threshold` or above (a selling opportunity).
    // `last_*` are the checker's evidence trail (what it saw, when, when it
    // last notified) so the UI can show "12p as of 3 min ago" without a
    // network call, and re-arm rather than nag.
    r#"
CREATE TABLE watch (
  id INTEGER PRIMARY KEY,
  slug TEXT NOT NULL,
  name TEXT NOT NULL,
  subtype TEXT,
  rank INTEGER,
  side TEXT NOT NULL CHECK(side IN ('sell','buy')),
  threshold INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  last_price INTEGER,
  last_checked_at INTEGER,            -- unix seconds
  last_fired_at INTEGER               -- unix seconds
);
CREATE INDEX watch_slug ON watch(slug);
"#,
];

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

/// What to record for one item of a plan run. Built by the listing command
/// layer from the plan's own items joined to their results — wfm-core stays
/// storage-free, so the DB write happens here rather than inside the executor.
pub struct ListingLogRow {
    pub slug: String,
    pub price: i64,
    pub qty: i64,
    /// "ok" | "skipped" | "error", verbatim from the plan result.
    pub status: String,
    /// "created" | "updated" on success, None otherwise.
    pub action: Option<String>,
    pub order_id: Option<String>,
    /// WFM's own error text on failure — the evidence that used to be lost.
    pub message: Option<String>,
}

/// A stored `listing_log` row, as handed to the SPA.
#[derive(serde::Serialize)]
pub struct ListingLogEntry {
    pub id: i64,
    pub plan_id: Option<String>,
    pub slug: String,
    pub listed_at: String,
    pub price: i64,
    pub qty: i64,
    pub status: String,
    pub action: Option<String>,
    pub order_id: Option<String>,
    pub message: Option<String>,
    /// NULL until a later orders-diff observes the listing sold or cancelled.
    pub outcome: Option<String>,
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

    // ---- watches ----

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
            (&w.slug, &w.name, &w.subtype, w.rank, &w.side, w.threshold, created_at),
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
        {
            let mut stmt = tx.prepare(
                "INSERT INTO listing_log
                   (plan_id, slug, listed_at, price, qty, status, action, order_id, message)
                 VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for r in rows {
                stmt.execute((
                    plan_id,
                    &r.slug,
                    r.price,
                    r.qty,
                    &r.status,
                    &r.action,
                    &r.order_id,
                    &r.message,
                ))?;
            }
        }
        tx.commit()?;
        Ok(rows.len())
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
    /// when no snapshot exists yet. `slug` is the DE item path — the caller
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

    #[cfg(test)]
    fn open_in_memory() -> rusqlite::Result<Db> {
        Self::init(Connection::open_in_memory()?)
    }

    #[cfg(test)]
    fn snapshot_count(&self) -> rusqlite::Result<i64> {
        let conn = guard(&self.conn);
        conn.query_row("SELECT COUNT(*) FROM snapshot", [], |r| r.get(0))
    }

    #[cfg(test)]
    fn snapshot_item_count(&self) -> rusqlite::Result<i64> {
        let conn = guard(&self.conn);
        conn.query_row("SELECT COUNT(*) FROM snapshot_item", [], |r| r.get(0))
    }

    #[cfg(test)]
    fn user_version(&self) -> rusqlite::Result<i64> {
        let conn = guard(&self.conn);
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
    fn migration_creates_the_full_schema() {
        let db = Db::open_in_memory().unwrap();
        // Against MIGRATIONS.len(), not a literal: a hardcoded version silently
        // stops testing the newest migration the moment one is appended.
        assert_eq!(db.user_version().unwrap(), MIGRATIONS.len() as i64);
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
                "watch".to_string(),
            ]
        );
    }

    #[test]
    fn migration_runner_is_idempotent_across_reopen() {
        let path = temp_db_path();
        let latest = MIGRATIONS.len() as i64;
        {
            let db = Db::open(&path).unwrap();
            db.set_setting("k", "v").unwrap();
            assert_eq!(db.user_version().unwrap(), latest);
        }
        // Reopen: migrate() runs again but must apply nothing and preserve data.
        {
            let db = Db::open(&path).unwrap();
            assert_eq!(db.user_version().unwrap(), latest);
            assert_eq!(db.get_setting("k").unwrap().as_deref(), Some("v"));
        }
        // Running the runner directly a second time on a live conn is a no-op.
        {
            let db = Db::open(&path).unwrap();
            let conn = db.conn.lock().unwrap();
            migrate(&conn).unwrap();
            drop(conn);
            assert_eq!(db.user_version().unwrap(), latest);
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
    fn latest_snapshot_items_returns_only_the_newest_snapshots_rows() {
        let db = Db::open_in_memory().unwrap();
        // No snapshot yet → empty, not an error.
        assert!(db.latest_snapshot_items().unwrap().is_empty());

        db.insert_snapshot(
            "import",
            Some("2020-01-01T00:00:00Z"),
            None,
            &[SnapshotItem { slug: "/Lotus/Old".into(), count: 1, leveled: 0 }],
        )
        .unwrap();
        db.insert_snapshot(
            "memory",
            None,
            None,
            &[
                SnapshotItem { slug: "/Lotus/New/A".into(), count: 3, leveled: 1 },
                SnapshotItem { slug: "/Lotus/New/B".into(), count: 7, leveled: 0 },
            ],
        )
        .unwrap();

        let latest = db.latest_snapshot_items().unwrap();
        // Only the second (highest-id) snapshot's rows, ordered by slug.
        assert_eq!(latest.len(), 2);
        assert_eq!(latest[0].slug, "/Lotus/New/A");
        assert_eq!(latest[0].count, 3);
        assert_eq!(latest[0].leveled, 1);
        assert_eq!(latest[1].slug, "/Lotus/New/B");
        assert_eq!(latest[1].count, 7);
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

    fn row(slug: &str, status: &str) -> ListingLogRow {
        ListingLogRow {
            slug: slug.into(),
            price: 42,
            qty: 2,
            status: status.into(),
            action: Some("created".into()),
            order_id: Some(format!("{slug}-oid")),
            message: None,
        }
    }

    #[test]
    fn listing_log_records_a_plan_and_reads_it_back_newest_first() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.list_listing_log(10).unwrap().len(), 0);

        assert_eq!(db.insert_listing_log("plan-a", &[row("mag_prime_set", "ok")]).unwrap(), 1);
        assert_eq!(db.insert_listing_log("plan-b", &[row("rhino_prime_set", "ok")]).unwrap(), 1);

        let all = db.list_listing_log(10).unwrap();
        assert_eq!(all.len(), 2);
        // Newest first.
        assert_eq!(all[0].slug, "rhino_prime_set");
        assert_eq!(all[0].plan_id.as_deref(), Some("plan-b"));
        assert_eq!(all[1].slug, "mag_prime_set");
        assert_eq!(all[0].price, 42);
        assert_eq!(all[0].qty, 2);
        assert_eq!(all[0].order_id.as_deref(), Some("rhino_prime_set-oid"));
        // Not yet observed as sold/cancelled.
        assert!(all[0].outcome.is_none());
        // The DB stamps the time; nothing is allowed to leave it empty.
        assert!(!all[0].listed_at.is_empty());

        assert_eq!(db.list_listing_log(1).unwrap().len(), 1);
    }

    #[test]
    fn listing_log_preserves_failure_evidence() {
        // The whole point of the table: an error row must survive the modal
        // that displayed it, carrying WFM's own message.
        let db = Db::open_in_memory().unwrap();
        let failed = ListingLogRow {
            slug: "loki_prime_set".into(),
            price: 90,
            qty: 1,
            status: "error".into(),
            action: None,
            order_id: None,
            message: Some("app.field.orders.perTradeMustDivideQuantity".into()),
            ..row("loki_prime_set", "error")
        };
        db.insert_listing_log("plan-c", &[failed]).unwrap();

        let got = db.list_listing_log(10).unwrap();
        assert_eq!(got[0].status, "error");
        assert_eq!(
            got[0].message.as_deref(),
            Some("app.field.orders.perTradeMustDivideQuantity")
        );
        assert!(got[0].action.is_none());
        assert!(got[0].order_id.is_none());
    }

    #[test]
    fn empty_plan_writes_nothing() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.insert_listing_log("plan-empty", &[]).unwrap(), 0);
        assert_eq!(db.list_listing_log(10).unwrap().len(), 0);
    }

    #[test]
    fn v1_databases_upgrade_without_losing_rows() {
        // Shipped users are on v1. Build a v1 DB the way they have one — run
        // ONLY the first migration — put a row in it, then open it normally and
        // check v2's ALTERs landed on top of the existing data rather than
        // recreating the table.
        let path = temp_db_path();
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATIONS[0]).unwrap();
            conn.pragma_update(None, "user_version", 1i64).unwrap();
            conn.execute(
                "INSERT INTO listing_log (slug, listed_at, price, qty)
                 VALUES ('legacy_item', '2026-01-01T00:00:00Z', 7, 1)",
                [],
            )
            .unwrap();
        }

        let db = Db::open(&path).unwrap();
        assert_eq!(db.user_version().unwrap(), MIGRATIONS.len() as i64);

        let rows = db.list_listing_log(10).unwrap();
        assert_eq!(rows.len(), 1, "the pre-existing v1 row must survive");
        assert_eq!(rows[0].slug, "legacy_item");
        assert_eq!(rows[0].price, 7);
        // Columns v1 never had: NULL for the legacy row, except `status`, whose
        // DEFAULT backfills it.
        assert_eq!(rows[0].status, "ok");
        assert!(rows[0].plan_id.is_none());
        assert!(rows[0].order_id.is_none());

        // And the upgraded table still accepts new writes.
        db.insert_listing_log("plan-after", &[row("new_item", "ok")]).unwrap();
        assert_eq!(db.list_listing_log(10).unwrap().len(), 2);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn watches_round_trip_and_record_checks() {
        let db = Db::open_in_memory().unwrap();
        let id = db
            .add_watch(
                &NewWatch { slug: "primed_flow".into(), name: "Primed Flow".into(), subtype: None, rank: Some(0), side: "sell".into(), threshold: 15 },
                Some("2026-08-16T00:00:00Z"),
            )
            .unwrap();
        db.add_watch(
            &NewWatch { slug: "lith_c5_relic".into(), name: "Lith C5 Relic".into(), subtype: Some("intact".into()), rank: None, side: "buy".into(), threshold: 8 },
            Some("2026-08-16T01:00:00Z"),
        )
        .unwrap();
        let all = db.list_watches().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].slug, "lith_c5_relic", "newest first");
        assert_eq!(all[0].subtype.as_deref(), Some("intact"));
        assert_eq!(all[1].id, id);
        assert_eq!(all[1].last_checked_at, None);

        db.record_watch_check(id, Some(12), 1_786_881_600, Some(1_786_881_600)).unwrap();
        db.record_watch_check(id, Some(14), 1_786_882_200, None).unwrap();
        let w = db.list_watches().unwrap().into_iter().find(|w| w.id == id).unwrap();
        assert_eq!(w.last_price, Some(14));
        assert_eq!(w.last_checked_at, Some(1_786_882_200));
        assert_eq!(w.last_fired_at, Some(1_786_881_600), "fired_at is kept when a later check did not fire");

        db.delete_watch(id).unwrap();
        assert_eq!(db.list_watches().unwrap().len(), 1);
    }
}
