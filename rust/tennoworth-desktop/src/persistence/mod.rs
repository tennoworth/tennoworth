mod inventory;
pub(crate) mod keyring_store;
mod notifications;
mod outcomes;
mod records;
mod schema;
mod settings;
pub(crate) mod snapshot;
mod trades;
mod watches;
pub(crate) use outcomes::RETENTION_SECS as LISTING_RETENTION_SECS;
pub(crate) use records::{
    ListedItem, ListingOrigin, NewWatch, SnapshotItem, SnapshotSummary, TradeRow, Watch,
};
use schema::MIGRATIONS;

// Canonical desktop state store (SQLite via rusqlite `bundled`). This is the
// single owner of the schema, the migration runner, and every SQL statement -
// call sites use the typed methods below and never write raw SQL. The schema is
// the one agreed in the product plan (C3), applied verbatim as the
// v1 migration.
//
// Two distinct concerns share this file:
//   - inventory HISTORY (`snapshot` / `snapshot_item`) -
//     the profit-tracking substrate, appended from day one.
//   - app STATE (`setting` kv) - the desktop backing for
//     the persistence the browser keeps in localStorage.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::poison::guard;
use rusqlite::{Connection, OptionalExtension};

/// Why the store could not be opened.
#[derive(Debug)]
pub enum OpenError {
    /// A later TennoWorth migrated the store past what this build knows. Its
    /// tables may mean something this build would misread or overwrite.
    NewerSchema {
        found: i64,
        supported: i64,
    },
    /// The copy taken before a migration could not be written, so the
    /// migration did not run.
    Backup {
        path: PathBuf,
        reason: String,
    },
    Sqlite(rusqlite::Error),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::NewerSchema { found, supported } => write!(
                f,
                "the data was written by a newer version of TennoWorth (schema {found}, this version reads up to {supported})"
            ),
            OpenError::Backup { path, reason } => write!(
                f,
                "could not write the pre-upgrade backup {}: {reason}",
                path.display()
            ),
            OpenError::Sqlite(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for OpenError {}

impl From<rusqlite::Error> for OpenError {
    fn from(e: rusqlite::Error) -> Self {
        OpenError::Sqlite(e)
    }
}

/// The open database. `Connection` is not `Sync`, so it lives behind a `Mutex`;
/// held as Tauri managed state (`State<'_, Db>`) and shared across commands.
/// Scans are already single-flighted upstream, so lock contention is a non-issue.
pub struct Db {
    conn: Mutex<Connection>,
    run_id: String,
}

impl Db {
    /// Open (creating if absent) the store at `path` and bring it to the latest
    /// schema version, backing up an on-disk store before migrating it. The
    /// desktop treats every failure as unrecoverable (the store is canonical).
    pub fn open(path: &Path) -> Result<Db, OpenError> {
        Self::init(Connection::open(path)?)
    }

    fn init(conn: Connection) -> Result<Db, OpenError> {
        // Checked before anything below writes: setting the journal mode
        // rewrites the file header, and a newer store must be left as found.
        let found: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let supported = MIGRATIONS.len() as i64;
        if found > supported {
            return Err(OpenError::NewerSchema { found, supported });
        }
        if 0 < found && found < supported {
            backup_before_migration(&conn, found)?;
        }
        // FK enforcement is per-connection (not persisted); turn it on so a
        // snapshot_item can never dangle without its snapshot.
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // Snapshots and trade-log events write in bursts. A write-ahead log with
        // NORMAL sync commits without an fsync each time while staying durable
        // across an app crash; only a power loss can drop the last commits.
        // Only this process opens the file, so the log's side files are private.
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get::<_, String>(0))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        migrate(&conn)?;
        Ok(Db {
            conn: Mutex::new(conn),
            run_id: wfm_trading::identity::random_token(16),
        })
    }

    // ---- trades (ledger) ----

    // ---- watches ----

    #[cfg(test)]
    pub(crate) fn open_in_memory() -> Result<Db, OpenError> {
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

fn read_allowance(
    conn: &Connection,
) -> rusqlite::Result<Option<crate::contracts::trading::Observation>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT observation FROM trade_allowance WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    raw.map(|json| {
        serde_json::from_str(&json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })
    })
    .transpose()
}

fn write_allowance(
    conn: &Connection,
    observation: &crate::contracts::trading::Observation,
) -> rusqlite::Result<()> {
    let raw = serde_json::to_string(observation)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    conn.execute("INSERT INTO trade_allowance (id, observation) VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET observation = excluded.observation", [raw])?;
    Ok(())
}

/// One copy is written per schema version a store is migrated away from, so
/// this covers the current upgrade and the two before it: a migration whose
/// logic error surfaces a release or two later still has its source, while a
/// store that grows with the trade ledger cannot multiply without bound.
const BACKUPS_KEPT: usize = 3;

fn backup_path(store: &Path, version: i64) -> PathBuf {
    let mut name = store.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".schema-v{version}.bak"));
    store.with_file_name(name)
}

/// Copy an on-disk store at schema `version` beside itself before it is
/// migrated. An existing copy for that version is kept, not replaced: it was
/// taken before the first attempt, and a later attempt may follow a migration
/// that already ran and failed further on.
fn backup_before_migration(conn: &Connection, version: i64) -> Result<(), OpenError> {
    let Some(store) = conn.path().filter(|p| !p.is_empty()).map(PathBuf::from) else {
        return Ok(());
    };
    let target = backup_path(&store, version);
    let exists = target.try_exists().map_err(|e| OpenError::Backup {
        path: target.clone(),
        reason: e.to_string(),
    })?;
    if !exists {
        write_backup(conn, &target).map_err(|reason| OpenError::Backup {
            path: target.clone(),
            reason,
        })?;
    }
    prune_backups(&store, &target);
    Ok(())
}

/// `VACUUM INTO` takes a consistent copy, including what is still in the
/// write-ahead log. It writes to a side file renamed into place, so a crash
/// mid-copy never leaves a partial file under the name a later attempt trusts.
fn write_backup(conn: &Connection, target: &Path) -> Result<(), String> {
    let mut partial = target.as_os_str().to_os_string();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    let partial_name = partial
        .to_str()
        .ok_or_else(|| "backup path is not valid UTF-8".to_string())?;
    // VACUUM INTO refuses a target that is not empty, which a crashed copy is.
    let _ = std::fs::remove_file(&partial);
    // Created empty at owner-only permissions first, because VACUUM INTO keeps
    // an existing file's mode but would create a new one world-readable.
    wfm_trading::platform::write_restricted(&partial, b"").map_err(|e| format!("{e:#}"))?;
    let copied = conn
        .execute("VACUUM INTO ?1", [partial_name])
        .map_err(|e| e.to_string())
        .and_then(|_| std::fs::rename(&partial, target).map_err(|e| e.to_string()));
    if copied.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    copied
}

/// Best effort: a backup that cannot be removed costs disk space, not data.
fn prune_backups(store: &Path, keep: &Path) {
    let (Some(dir), Some(name)) = (store.parent(), store.file_name().and_then(|n| n.to_str()))
    else {
        return;
    };
    let prefix = format!("{name}.schema-v");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut older: Vec<(i64, PathBuf)> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let file = entry.file_name();
            let version = file
                .to_str()?
                .strip_prefix(&prefix)?
                .strip_suffix(".bak")?
                .parse()
                .ok()?;
            let path = entry.path();
            (path != keep).then_some((version, path))
        })
        .collect();
    older.sort_by_key(|(version, _)| std::cmp::Reverse(*version));
    for (_, path) in older.into_iter().skip(BACKUPS_KEPT.saturating_sub(1)) {
        if let Err(e) = std::fs::remove_file(&path) {
            eprintln!(
                "tennoworth: could not remove old backup {}: {e}",
                path.display()
            );
        }
    }
}

/// Apply only migrations newer than the persisted schema version.
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        let version = (i + 1) as i64;
        if current < version {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(sql)?;
            // pragma_update won't bind `user_version` as a parameter - it's part
            // of the statement text - so format it in (it's our own integer).
            tx.pragma_update(None, "user_version", version)?;
            tx.commit()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn temp_db_path() -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "tennoworth-db-test-{}-{}.db",
            std::process::id(),
            nanos
        ))
    }

    #[test]
    fn migration_creates_the_full_schema() {
        let db = Db::open_in_memory().unwrap();
        // Against MIGRATIONS.len(), not a literal: a hardcoded version silently
        // stops testing the newest migration the moment one is appended.
        assert_eq!(db.user_version().unwrap(), MIGRATIONS.len() as i64);
        let conn = db.conn.lock().unwrap();
        let mut names: Vec<String> = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            )
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        names.sort();
        assert_eq!(
            names,
            vec![
                "listing_outcome".to_string(),
                "notification".to_string(),
                "notification_checkpoint".to_string(),
                "setting".to_string(),
                "snapshot".to_string(),
                "snapshot_item".to_string(),
                "trade".to_string(),
                "trade_allowance".to_string(),
                "trade_log_event".to_string(),
                "watch".to_string(),
            ]
        );
    }

    #[test]
    fn notification_v5_upgrade_keeps_history_and_adds_trade_allowance() {
        // A directory of its own: the upgrade leaves a backup beside the store.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tennoworth.db");
        {
            let conn = Connection::open(&path).unwrap();
            for sql in MIGRATIONS.iter().take(5) {
                conn.execute_batch(sql).unwrap();
            }
            conn.pragma_update(None, "user_version", 5).unwrap();
            conn.execute("INSERT INTO notification(category,title,body,target,created_at,delivery) VALUES ('trades','Saved sale','Saved context','ledger',1000,'inbox_only')", []).unwrap();
            conn.execute(
                "INSERT INTO setting(key,value) VALUES ('reserve-copies','2')",
                [],
            )
            .unwrap();
        }
        let db = Db::open(&path).unwrap();
        assert_eq!(db.user_version().unwrap(), MIGRATIONS.len() as i64);
        assert_eq!(db.list_notifications().unwrap()[0].title, "Saved sale");
        assert_eq!(
            db.get_setting("reserve-copies").unwrap().as_deref(),
            Some("2")
        );
        assert_eq!(db.trade_allowance(1000).unwrap().remaining, None);
        let position = crate::services::game_log::eelog::LogPosition {
            session: "upgrade".into(),
            start: 1,
            end: 2,
            observed_after: 1000,
        };
        assert!(db
            .insert_trade(&allowance_trade("sale"), 1001, &position)
            .unwrap()
            .is_some());
        assert!(db
            .insert_trade(&allowance_trade("sale"), 1002, &position)
            .unwrap()
            .is_none());
        assert_eq!(db.list_notifications().unwrap().len(), 1);
    }

    /// A store at `version` holding one setting, closed before returning.
    fn store_at(path: &Path, version: usize) {
        let conn = Connection::open(path).unwrap();
        for sql in MIGRATIONS.iter().take(version) {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", version as i64)
            .unwrap();
        conn.execute(
            "INSERT INTO setting(key,value) VALUES ('before-upgrade','kept')",
            [],
        )
        .unwrap();
    }

    fn backups(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.contains(".schema-v"))
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_store_from_a_newer_version_is_refused_and_left_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tennoworth.db");
        let future = MIGRATIONS.len() as i64 + 1;
        {
            let db = Db::open(&path).unwrap();
            db.set_setting("written-by", "newer").unwrap();
            guard(&db.conn)
                .pragma_update(None, "user_version", future)
                .unwrap();
        }
        let before = std::fs::read(&path).unwrap();
        let listing = |dir: &Path| {
            let mut names: Vec<_> = std::fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            names.sort();
            names
        };
        let files_before = listing(dir.path());

        match Db::open(&path) {
            Err(OpenError::NewerSchema { found, supported }) => {
                assert_eq!((found, supported), (future, MIGRATIONS.len() as i64));
            }
            Err(other) => panic!("expected NewerSchema, got {other}"),
            Ok(_) => panic!("a newer store must not open"),
        }

        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(listing(dir.path()), files_before);
    }

    #[test]
    fn an_older_store_is_backed_up_before_it_is_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tennoworth.db");
        let old = MIGRATIONS.len() - 1;
        store_at(&path, old);

        let db = Db::open(&path).unwrap();
        assert_eq!(db.user_version().unwrap(), MIGRATIONS.len() as i64);
        drop(db);

        let name = format!("tennoworth.db.schema-v{old}.bak");
        assert_eq!(backups(dir.path()), vec![name.clone()]);
        let backup = Connection::open(dir.path().join(&name)).unwrap();
        let version: i64 = backup
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, old as i64);
        let kept: String = backup
            .query_row(
                "SELECT value FROM setting WHERE key='before-upgrade'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kept, "kept");
        // The fold the last migration performs has not run in the copy.
        let reserve: i64 = backup
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='reserve'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(reserve, 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join(&name))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn an_existing_backup_for_the_version_is_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tennoworth.db");
        let old = MIGRATIONS.len() - 1;
        store_at(&path, old);
        let earlier = dir.path().join(format!("tennoworth.db.schema-v{old}.bak"));
        std::fs::write(&earlier, b"first attempt").unwrap();

        Db::open(&path).unwrap();

        assert_eq!(std::fs::read(&earlier).unwrap(), b"first attempt");
    }

    #[test]
    fn a_new_store_gets_no_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tennoworth.db");
        drop(Db::open(&path).unwrap());
        drop(Db::open(&path).unwrap());
        assert_eq!(backups(dir.path()), Vec::<String>::new());
    }

    #[test]
    fn only_the_newest_backups_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tennoworth.db");
        for version in 1..=4 {
            std::fs::write(
                dir.path()
                    .join(format!("tennoworth.db.schema-v{version}.bak")),
                b"older",
            )
            .unwrap();
        }
        let unrelated = dir.path().join("other.db.schema-v1.bak");
        std::fs::write(&unrelated, b"not ours").unwrap();
        let old = MIGRATIONS.len() - 1;
        store_at(&path, old);

        Db::open(&path).unwrap();

        let mut expected = vec![
            "tennoworth.db.schema-v3.bak".to_string(),
            "tennoworth.db.schema-v4.bak".to_string(),
            format!("tennoworth.db.schema-v{old}.bak"),
        ];
        expected.sort();
        assert_eq!(BACKUPS_KEPT, expected.len());
        assert_eq!(
            backups(dir.path())
                .into_iter()
                .filter(|name| name.starts_with("tennoworth.db"))
                .collect::<Vec<_>>(),
            expected
        );
        assert!(unrelated.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_store_whose_backup_cannot_be_written_is_not_migrated() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tennoworth.db");
        let old = MIGRATIONS.len() - 1;
        store_at(&path, old);
        // Read-only directory: the store itself stays writable, but no new
        // file can be created beside it.
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
        let result = Db::open(&path);
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();

        assert!(matches!(result, Err(OpenError::Backup { .. })));
        let conn = Connection::open(&path).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, old as i64);
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
    fn a_file_store_uses_the_write_ahead_log() {
        let path = temp_db_path();
        let db = Db::open(&path).unwrap();
        let conn = guard(&db.conn);
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        let sync: i64 = conn
            .query_row("PRAGMA synchronous", [], |r| r.get(0))
            .unwrap();
        assert_eq!((mode.as_str(), sync), ("wal", 1));
        drop(conn);
        drop(db);
        std::fs::remove_file(&path).unwrap();
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

    /// Opens a database whose schema stops just before the reserve fold, seeded
    /// by `seed`, and returns it after the fold ran.
    fn folded(seed: &str) -> Db {
        let conn = Connection::open_in_memory().unwrap();
        let previous = MIGRATIONS.len() - 1;
        for sql in MIGRATIONS.iter().take(previous) {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", previous as i64)
            .unwrap();
        conn.execute_batch(seed).unwrap();
        Db::init(conn).unwrap()
    }

    fn table_exists(db: &Db, name: &str) -> bool {
        let conn = guard(&db.conn);
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [name],
            |r| r.get::<_, i64>(0),
        )
        .unwrap()
            > 0
    }

    fn set_plan_reserve(db: &Db, slug: &str, keep: u32) {
        let plan = serde_json::json!({"reserves": {slug: keep}, "goal": null});
        db.set_setting("protected-selling-plan-v1", &plan.to_string())
            .unwrap();
    }

    fn plan(db: &Db) -> serde_json::Value {
        serde_json::from_str(
            &db.get_setting("protected-selling-plan-v1")
                .unwrap()
                .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn old_keep_counts_fold_into_the_protection_plan() {
        let db = folded(
            r#"INSERT INTO reserve VALUES ('vitality', 2), ('serration', 5), ('blind "rage"', 9999999), ('zero', 0);
               INSERT INTO setting VALUES ('protected-selling-plan-v1', '{"reserves":{"serration":3,"other":1},"goal":"x_set"}');"#,
        );
        assert!(!table_exists(&db, "reserve"));
        assert_eq!(
            plan(&db),
            serde_json::json!({"reserves":{"other":1,"serration":5,"vitality":2,"blind \"rage\"":1000000},"goal":"x_set"})
        );
        // The larger of an existing plan reserve and the old keep wins.
        let db = folded(
            r#"INSERT INTO reserve VALUES ('serration', 2);
               INSERT INTO setting VALUES ('protected-selling-plan-v1', '{"reserves":{"serration":4},"goal":null}');"#,
        );
        assert_eq!(plan(&db)["reserves"]["serration"], 4);
    }

    #[test]
    fn the_fold_creates_a_plan_only_when_there_is_something_to_keep() {
        let db = folded("INSERT INTO reserve VALUES ('vitality', 2);");
        assert_eq!(
            plan(&db),
            serde_json::json!({"reserves":{"vitality":2},"goal":null})
        );
        let db = folded("INSERT INTO reserve VALUES ('zero', 0);");
        assert_eq!(db.get_setting("protected-selling-plan-v1").unwrap(), None);
        assert!(!table_exists(&db, "reserve"));
    }

    #[test]
    fn an_unreadable_plan_survives_the_fold_untouched() {
        let db = folded(
            "INSERT INTO reserve VALUES ('vitality', 2);
             INSERT INTO setting VALUES ('protected-selling-plan-v1', 'not json');",
        );
        assert_eq!(
            db.get_setting("protected-selling-plan-v1")
                .unwrap()
                .as_deref(),
            Some("not json")
        );
        assert!(!table_exists(&db, "reserve"));
    }

    #[test]
    fn snapshot_insert_and_list() {
        let db = Db::open_in_memory().unwrap();
        let items = vec![
            SnapshotItem {
                slug: "/Lotus/A".into(),
                count: 3,
                leveled: 0,
            },
            SnapshotItem {
                slug: "/Lotus/B".into(),
                count: 1,
                leveled: 1,
            },
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
            &[SnapshotItem {
                slug: "/Lotus/C".into(),
                count: 9,
                leveled: 0,
            }],
        )
        .unwrap();
        let list = db.list_snapshots(10).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].source, "import"); // higher id, listed first
        assert_eq!(list[0].item_count, 1);
    }

    #[test]
    fn a_repeated_inventory_reuses_its_snapshot_and_a_change_records_a_new_one() {
        let item = |slug: &str, count| SnapshotItem {
            slug: slug.into(),
            count,
            leveled: 0,
        };
        let db = Db::open_in_memory().unwrap();
        let first = db
            .record_snapshot("memory", None, &[item("/Lotus/A", 2), item("/Lotus/B", 1)])
            .unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute("UPDATE snapshot SET taken_at = '2020-01-01T00:00:00Z'", [])
            .unwrap();

        // Same items in another order: the same snapshot, confirmed now.
        let repeat = db
            .record_snapshot(
                "memory",
                Some("41.0"),
                &[item("/Lotus/B", 1), item("/Lotus/A", 2)],
            )
            .unwrap();
        assert_eq!(repeat, first);
        let list = db.list_snapshots(10).unwrap();
        assert_eq!(list.len(), 1);
        assert_ne!(
            list[0].taken_at, "2020-01-01T00:00:00Z",
            "a repeat is a fresh confirmation"
        );

        // One count differs: a new snapshot, which becomes the latest.
        let changed = db
            .record_snapshot("memory", None, &[item("/Lotus/A", 1), item("/Lotus/B", 1)])
            .unwrap();
        assert!(changed > first);
        // The same items from another source are not a repeat of it.
        let imported = db
            .record_snapshot("import", None, &[item("/Lotus/A", 1), item("/Lotus/B", 1)])
            .unwrap();
        assert!(imported > changed);
        assert_eq!(db.list_snapshots(10).unwrap().len(), 3);
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
            &[SnapshotItem {
                slug: "/Lotus/Old".into(),
                count: 1,
                leveled: 0,
            }],
        )
        .unwrap();
        db.insert_snapshot(
            "memory",
            None,
            None,
            &[
                SnapshotItem {
                    slug: "/Lotus/New/A".into(),
                    count: 3,
                    leveled: 1,
                },
                SnapshotItem {
                    slug: "/Lotus/New/B".into(),
                    count: 7,
                    leveled: 0,
                },
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
            SnapshotItem {
                slug: "/Lotus/Dup".into(),
                count: 1,
                leveled: 0,
            },
            SnapshotItem {
                slug: "/Lotus/Dup".into(),
                count: 2,
                leveled: 0,
            },
        ];
        assert!(db.insert_snapshot("memory", None, None, &dup).is_err());
        // Whole snapshot rolled back: no header row, no item rows.
        assert_eq!(db.snapshot_count().unwrap(), 0);
        assert_eq!(db.snapshot_item_count().unwrap(), 0);

        // A bad `source` trips the CHECK on the header insert itself.
        assert!(db.insert_snapshot("bogus", None, None, &[]).is_err());
        assert_eq!(db.snapshot_count().unwrap(), 0);

        // The store is still usable afterwards - a good insert lands.
        db.insert_snapshot(
            "memory",
            None,
            None,
            &[SnapshotItem {
                slug: "/Lotus/Ok".into(),
                count: 1,
                leveled: 0,
            }],
        )
        .unwrap();
        assert_eq!(db.snapshot_count().unwrap(), 1);
    }

    #[test]
    fn upgrade_drops_listing_log_and_keeps_the_rest() {
        // The version before the drop, built the way a user's database has it:
        // every earlier migration applied in turn, with history recorded.
        let previous = 7;
        let path = temp_db_path();
        {
            let conn = Connection::open(&path).unwrap();
            for sql in MIGRATIONS.iter().take(previous) {
                conn.execute_batch(sql).unwrap();
            }
            conn.pragma_update(None, "user_version", previous as i64)
                .unwrap();
            conn.execute(
                "INSERT INTO listing_log (plan_id, plan_index, slug, listed_at, price, qty, status)
                 VALUES ('plan-a', 0, 'mag_prime_set', '2026-09-01T00:00:00Z', 40, 1, 'ok')",
                [],
            )
            .unwrap();
            conn.execute("INSERT INTO setting(key,value) VALUES ('view','sell')", [])
                .unwrap();
        }

        let db = Db::open(&path).unwrap();
        assert_eq!(db.user_version().unwrap(), MIGRATIONS.len() as i64);
        assert_eq!(db.get_setting("view").unwrap().as_deref(), Some("sell"));
        let leftovers: i64 = db
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name LIKE 'listing_log%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(leftovers, 0, "the table and its indexes are gone");
        drop(db);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn watches_round_trip_and_record_checks() {
        let db = Db::open_in_memory().unwrap();
        let id = db
            .add_watch(
                &NewWatch {
                    slug: "primed_flow".into(),
                    name: "Primed Flow".into(),
                    subtype: None,
                    rank: Some(0),
                    side: "sell".into(),
                    threshold: 15,
                },
                Some("2026-08-16T00:00:00Z"),
            )
            .unwrap();
        db.add_watch(
            &NewWatch {
                slug: "lith_c5_relic".into(),
                name: "Lith C5 Relic".into(),
                subtype: Some("intact".into()),
                rank: None,
                side: "buy".into(),
                threshold: 8,
            },
            Some("2026-08-16T01:00:00Z"),
        )
        .unwrap();
        let all = db.list_watches().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].slug, "lith_c5_relic", "newest first");
        assert_eq!(all[0].subtype.as_deref(), Some("intact"));
        assert_eq!(all[1].id, id);
        assert_eq!(all[1].last_checked_at, None);

        db.record_watch_check(id, Some(12), 1_786_881_600, Some(1_786_881_600))
            .unwrap();
        db.record_watch_check(id, Some(14), 1_786_882_200, None)
            .unwrap();
        let w = db
            .list_watches()
            .unwrap()
            .into_iter()
            .find(|w| w.id == id)
            .unwrap();
        assert_eq!(w.last_price, Some(14));
        assert_eq!(w.last_checked_at, Some(1_786_882_200));
        assert_eq!(
            w.last_fired_at,
            Some(1_786_881_600),
            "fired_at is kept when a later check did not fire"
        );

        db.delete_watch(id).unwrap();
        assert_eq!(db.list_watches().unwrap().len(), 1);
    }

    #[test]
    fn trades_insert_dedupe_and_list_newest_first() {
        use crate::services::game_log::eelog::{TradeEvent, TradeItem};
        let db = Db::open_in_memory().unwrap();
        let t = TradeEvent {
            partner: "Buyer".into(),
            kind: "sale".into(),
            plat: 45,
            items: vec![TradeItem {
                name: "Primed Flow".into(),
                qty: 1,
                direction: "given".into(),
            }],
            log_stamp: Some("1234.567".into()),
        };
        let mut position = crate::services::game_log::eelog::LogPosition {
            session: "one".into(),
            start: 10,
            end: 20,
            observed_after: 1_786_881_600,
        };
        let a = db
            .insert_trade(&t, 1_786_881_600, &position)
            .unwrap()
            .unwrap();
        let b = db
            .insert_trade(&t, 1_786_881_600 + 3600, &position)
            .unwrap();
        assert_eq!(b, None, "replays remain duplicates beyond ten minutes");
        position.session = "two".into();
        let c = db
            .insert_trade(&t, 1_786_881_600 + 3600, &position)
            .unwrap()
            .unwrap();
        assert_ne!(a, c);
        db.mark_trade_wfm_closed(a).unwrap();
        let rows = db.list_trades(10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, c, "newest first");
        assert!(!rows[0].wfm_closed);
        assert!(rows[1].wfm_closed);
        assert_eq!(rows[1].items[0].name, "Primed Flow");
    }

    #[test]
    fn digests_and_sessions_share_reserves_and_post_trade_exclusions() {
        let db = Db::open_in_memory().unwrap();
        let market: crate::services::market::sellables::MarketData = serde_json::from_value(serde_json::json!({
            "items":{"test_part":{"vol":1000,"low_sell":10,"avg":10,"median_now":10,"median_90d":10,"low5_avg":10}},
            "path_to_info":{"/Lotus/TestPart":{"name":"Test Part","slug":"test_part"}}
        })).unwrap();
        let stamp = "1970-01-01T00:01:40Z";
        let id = db
            .insert_snapshot(
                "memory",
                Some(stamp),
                None,
                &[SnapshotItem {
                    slug: "/Lotus/TestPart".into(),
                    count: 6,
                    leveled: 1,
                }],
            )
            .unwrap();
        let boundary = crate::services::game_log::eelog::LogPosition {
            session: "digest-test".into(),
            start: 100,
            end: 100,
            observed_after: 100,
        };
        db.save_allowance(crate::services::game_log::allowance::Observation::scanned(
            "account".into(),
            id,
            &serde_json::json!({"TradesRemaining":8}),
            Some(boundary.clone()),
            Some(boundary.clone()),
            100,
            100,
        ))
        .unwrap();
        db.set_setting("reserve-copies", "1").unwrap();
        // One leveled copy plus two protected: three of six held back.
        set_plan_reserve(&db, "test_part", 2);
        let rows = crate::services::market::sellables::rank_sellables(&db, &market);
        assert_eq!(rows[0].sellable_qty, 3);
        assert_eq!(market.session_quantities(&db).unwrap()["test_part"], 3);
        let prices = serde_json::json!({"updated_at":stamp});
        assert!(crate::services::notifications::reminders::digest(
            &prices,
            &rows,
            stamp,
            110,
            "1970-01-01",
            18
        )
        .unwrap()
        .body
        .contains("Test Part ×3"));
        db.set_setting("reserve-copies", "4").unwrap();
        assert_eq!(
            crate::services::market::sellables::rank_sellables(&db, &market)[0].sellable_qty,
            2
        );
        set_plan_reserve(&db, "test_part", 6);
        assert!(crate::services::market::sellables::rank_sellables(&db, &market).is_empty());
        db.set_setting("reserve-copies", "1").unwrap();
        set_plan_reserve(&db, "test_part", 2);
        let trade = crate::services::game_log::eelog::TradeEvent {
            partner: "Buyer".into(),
            kind: "sale".into(),
            plat: 10,
            log_stamp: Some("110".into()),
            items: vec![crate::services::game_log::eelog::TradeItem {
                name: "Test Part".into(),
                qty: 1,
                direction: "given".into(),
            }],
        };
        let position = crate::services::game_log::eelog::LogPosition {
            start: 101,
            end: 120,
            observed_after: 110,
            ..boundary
        };
        db.insert_trade(&trade, 110, &position).unwrap();
        let rows = crate::services::market::sellables::rank_sellables(&db, &market);
        assert!(rows.is_empty());
        assert!(!market
            .session_quantities(&db)
            .unwrap()
            .contains_key("test_part"));
        assert!(crate::services::notifications::reminders::digest(
            &prices,
            &rows,
            stamp,
            111,
            "1970-01-01",
            18
        )
        .is_none());
        let id = db
            .insert_snapshot(
                "memory",
                Some(stamp),
                None,
                &[SnapshotItem {
                    slug: "/Lotus/TestPart".into(),
                    count: 5,
                    leveled: 1,
                }],
            )
            .unwrap();
        let boundary = crate::services::game_log::eelog::LogPosition {
            start: 120,
            end: 120,
            observed_after: 130,
            ..position
        };
        db.save_allowance(crate::services::game_log::allowance::Observation::scanned(
            "account".into(),
            id,
            &serde_json::json!({"TradesRemaining":7}),
            Some(boundary.clone()),
            Some(boundary),
            130,
            130,
        ))
        .unwrap();
        assert_eq!(
            crate::services::market::sellables::rank_sellables(&db, &market)[0].sellable_qty,
            2
        );
        assert_eq!(market.session_quantities(&db).unwrap()["test_part"], 2);
    }

    fn allowance_scan(db: &Db, account: &str, remaining: u32) {
        let id = db.insert_snapshot("memory", None, None, &[]).unwrap();
        let before = crate::services::game_log::eelog::LogPosition {
            session: "log".into(),
            start: 100,
            end: 100,
            observed_after: 100,
        };
        let after = crate::services::game_log::eelog::LogPosition {
            start: 200,
            end: 200,
            observed_after: 102,
            ..before.clone()
        };
        db.save_allowance(crate::services::game_log::allowance::Observation::scanned(
            account.into(),
            id,
            &serde_json::json!({"TradesRemaining":remaining,"PlayerLevel":24}),
            Some(before),
            Some(after),
            100,
            102,
        ))
        .unwrap();
    }

    fn allowance_trade(kind: &str) -> crate::services::game_log::eelog::TradeEvent {
        crate::services::game_log::eelog::TradeEvent {
            partner: "Partner".into(),
            kind: kind.into(),
            plat: 12,
            items: vec![],
            log_stamp: Some("100.0".into()),
        }
    }

    #[test]
    fn allowance_and_ledger_commit_once_for_every_transaction_kind() {
        let db = Db::open_in_memory().unwrap();
        allowance_scan(&db, "account", 8);
        for (index, kind) in ["sale", "purchase", "trade"].iter().enumerate() {
            let position = crate::services::game_log::eelog::LogPosition {
                session: "log".into(),
                start: 201 + index as u64 * 20,
                end: 220 + index as u64 * 20,
                observed_after: 103,
            };
            assert!(db
                .insert_trade(&allowance_trade(kind), 110, &position)
                .unwrap()
                .is_some());
            assert!(db
                .insert_trade(&allowance_trade(kind), 10_000, &position)
                .unwrap()
                .is_none());
        }
        assert_eq!(db.list_trades(100).unwrap().len(), 3);
        assert_eq!(db.trade_allowance(110).unwrap().remaining, Some(5));
    }

    #[test]
    fn allowance_reconciles_callback_committed_before_observation_is_saved() {
        let db = Db::open_in_memory().unwrap();
        let position = crate::services::game_log::eelog::LogPosition {
            session: "log".into(),
            start: 201,
            end: 220,
            observed_after: 103,
        };
        db.insert_trade(&allowance_trade("sale"), 110, &position)
            .unwrap();
        allowance_scan(&db, "account", 8);
        assert_eq!(db.trade_allowance(110).unwrap().remaining, Some(7));
    }

    #[test]
    fn failed_ledger_write_cannot_advance_allowance_or_replay_position() {
        let db = Db::open_in_memory().unwrap();
        allowance_scan(&db, "account", 8);
        let position = crate::services::game_log::eelog::LogPosition {
            session: "log".into(),
            start: 201,
            end: 220,
            observed_after: 103,
        };
        assert!(db
            .insert_trade(&allowance_trade("invalid-kind"), 110, &position)
            .is_err());
        assert_eq!(db.trade_allowance(110).unwrap().remaining, Some(8));
        assert!(db
            .insert_trade(&allowance_trade("sale"), 110, &position)
            .unwrap()
            .is_some());
        assert_eq!(db.trade_allowance(110).unwrap().remaining, Some(7));
    }

    #[test]
    fn fresh_account_and_missing_metadata_replace_previous_observation() {
        let db = Db::open_in_memory().unwrap();
        allowance_scan(&db, "first", 8);
        allowance_scan(&db, "second", 0);
        assert_eq!(db.trade_allowance(110).unwrap().remaining, Some(0));
        assert_eq!(
            read_allowance(&guard(&db.conn))
                .unwrap()
                .unwrap()
                .account_key,
            "second"
        );
        db.save_allowance(crate::services::game_log::allowance::Observation::scanned(
            "third".into(),
            3,
            &serde_json::json!({}),
            None,
            None,
            110,
            111,
        ))
        .unwrap();
        assert_eq!(db.trade_allowance(112).unwrap().remaining, None);
    }

    #[test]
    fn submission_guard_rejects_old_snapshots_and_days_without_spending_trades() {
        let db = Db::open_in_memory().unwrap();
        allowance_scan(&db, "first", 8);
        let observed = db.trade_allowance(110).unwrap();
        let id = observed.snapshot_id.unwrap();
        assert!(db.session_allowance(id, 0, 110).is_ok());
        assert_eq!(db.trade_allowance(110).unwrap().remaining, Some(8));
        assert!(db.session_allowance(id, 0, 86_401).is_err());
        allowance_scan(&db, "second", 24);
        assert!(db.session_allowance(id, 0, 111).is_err());
    }

    #[test]
    fn restart_retains_age_and_deduplication_but_cannot_assume_the_account() {
        let path = temp_db_path();
        let position = crate::services::game_log::eelog::LogPosition {
            session: "log".into(),
            start: 201,
            end: 220,
            observed_after: 103,
        };
        {
            let db = Db::open(&path).unwrap();
            allowance_scan(&db, "account", 8);
            db.insert_trade(&allowance_trade("sale"), 110, &position)
                .unwrap();
        }
        let db = Db::open(&path).unwrap();
        let view = db.trade_allowance(120).unwrap();
        assert_eq!(view.remaining, Some(7));
        assert_eq!(view.observed_at, Some(102));
        assert_eq!(
            view.confidence,
            crate::contracts::trading::Confidence::Estimated
        );
        assert!(!view.monitoring);
        assert_eq!(
            db.insert_trade(&allowance_trade("sale"), 4000, &position)
                .unwrap(),
            None
        );
        let next = crate::services::game_log::eelog::LogPosition {
            start: 230,
            end: 250,
            observed_after: 120,
            ..position
        };
        db.insert_trade(&allowance_trade("sale"), 121, &next)
            .unwrap();
        assert_eq!(db.trade_allowance(122).unwrap().remaining, Some(7));
        drop(db);
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(test)]
mod notification_tests {
    use super::*;
    use crate::contracts::notification::Candidate;
    fn candidate() -> Candidate {
        Candidate::once(
            "baro:visit".into(),
            "baro",
            "Baro is here".into(),
            "Relay".into(),
            "baro",
            1000,
        )
    }
    #[test]
    fn structured_notification_survives_restart_and_bad_details_keep_the_body() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/notifications/baro.json"
        ))
        .unwrap();
        let path = tests::temp_db_path();
        let id;
        {
            let db = Db::open(&path).unwrap();
            let mut notice = candidate();
            notice.content =
                Some(serde_json::from_value(fixture["expected_content"].clone()).unwrap());
            id = db
                .insert_notification(&notice, 1000, false, true)
                .unwrap()
                .unwrap();
        }
        let db = Db::open(&path).unwrap();
        let rows = db.list_notifications().unwrap();
        assert_eq!(
            serde_json::to_value(&rows[0].content).unwrap(),
            fixture["expected_content"]
        );
        for bad in ["{", r#"{"kind":"unknown"}"#] {
            guard(&db.conn)
                .execute(
                    "UPDATE notification SET content=?1 WHERE id=?2",
                    rusqlite::params![bad, id],
                )
                .unwrap();
            let rows = db.list_notifications().unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].body, "Relay");
            assert!(rows[0].content.is_none());
        }
        drop(db);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn structured_content_upgrade_preserves_history_checkpoints_and_preferences() {
        let path = tests::temp_db_path();
        let previous = 8;
        {
            let conn = Connection::open(&path).unwrap();
            for sql in MIGRATIONS.iter().take(previous) {
                conn.execute_batch(sql).unwrap();
            }
            conn.pragma_update(None, "user_version", previous as i64)
                .unwrap();
            conn.execute("INSERT INTO notification(category,title,body,target,created_at,read,delivery) VALUES ('baro','Saved arrival','Full saved text','baro',1000,1,'failed')", []).unwrap();
            conn.execute("INSERT INTO notification_checkpoint(key,stage,at,expires_at) VALUES ('baro:visit',2,1000,5000)", []).unwrap();
            conn.execute("INSERT INTO setting(key,value) VALUES ('notifications-v1','{\"popups\":false,\"categories\":{\"baro\":{\"enabled\":false,\"native\":false}}}')", []).unwrap();
        }
        let db = Db::open(&path).unwrap();
        let rows = db.list_notifications().unwrap();
        assert_eq!(rows[0].body, "Full saved text");
        assert!(rows[0].content.is_none());
        assert!(rows[0].read);
        assert_eq!(rows[0].delivery, "failed");
        assert!(!db.notification_preferences().unwrap().categories["baro"].enabled);
        assert!(!db.notification_preferences().unwrap().popups);
        assert!(db
            .insert_notification(&candidate(), 1001, true, true)
            .unwrap()
            .is_none());
        drop(db);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn inbox_survives_restart_and_clear_keeps_checkpoints() {
        let path = tests::temp_db_path();
        let c = candidate();
        {
            let db = Db::open(&path).unwrap();
            let id = db
                .insert_notification(&c, 1000, true, true)
                .unwrap()
                .unwrap();
            db.notification_delivery(id, "failed").unwrap();
            db.mark_notifications_read(Some(id)).unwrap();
            let mut prefs = db.notification_preferences().unwrap();
            prefs.popups = false;
            prefs.categories.get_mut("baro").unwrap().enabled = false;
            db.set_setting("notifications-v1", &serde_json::to_string(&prefs).unwrap())
                .unwrap();
        }
        let db = Db::open(&path).unwrap();
        let prefs = db.notification_preferences().unwrap();
        assert!(!prefs.popups && !prefs.categories["baro"].enabled);
        let rows = db.list_notifications().unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].read);
        assert_eq!(rows[0].delivery, "failed");
        assert!(db
            .insert_notification(&c, 1001, true, true)
            .unwrap()
            .is_none());
        db.clear_notifications().unwrap();
        assert!(db.list_notifications().unwrap().is_empty());
        assert!(db
            .insert_notification(&c, 1002, true, true)
            .unwrap()
            .is_none());
        let mut later = c;
        later.stage = 3;
        assert!(db
            .insert_notification(&later, 1003, false, true)
            .unwrap()
            .is_some());
        later.stage = 2;
        assert!(db
            .insert_notification(&later, 1004, false, true)
            .unwrap()
            .is_none());
        drop(db);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn concurrent_watch_producers_share_a_cooldown_even_after_restart() {
        let db = std::sync::Arc::new(Db::open_in_memory().unwrap());
        let mut c = candidate();
        c.key = "watch:1".into();
        c.cooldown = 21600;
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let db = db.clone();
                let c = c.clone();
                std::thread::spawn(move || {
                    db.insert_notification(&c, 1000, true, true)
                        .unwrap()
                        .is_some()
                })
            })
            .collect();
        assert_eq!(
            handles
                .into_iter()
                .map(|h| usize::from(h.join().unwrap()))
                .sum::<usize>(),
            1
        );
        assert_eq!(db.list_notifications().unwrap().len(), 1);
        assert!(db
            .insert_notification(&c, 1000 + 21600 - 1, true, true)
            .unwrap()
            .is_none());
        assert!(db
            .insert_notification(&c, 1000 + 21600, true, true)
            .unwrap()
            .is_some());
    }
    #[test]
    fn disabled_categories_checkpoint_without_history_and_pause_preserves_history() {
        let db = Db::open_in_memory().unwrap();
        let mut c = candidate();
        assert!(db
            .insert_notification(&c, 1000, true, false)
            .unwrap()
            .is_none());
        assert!(db.list_notifications().unwrap().is_empty());
        assert!(db
            .insert_notification(&c, 1001, true, true)
            .unwrap()
            .is_none());
        c.stage = 2;
        db.insert_notification(&c, 1002, false, true).unwrap();
        assert_eq!(db.list_notifications().unwrap()[0].delivery, "inbox_only");
        db.mark_notifications_read(None).unwrap();
        assert!(db.list_notifications().unwrap()[0].read);
    }
    #[test]
    fn retention_caps_history_without_rearming_live_events() {
        let db = Db::open_in_memory().unwrap();
        for i in 0..1005 {
            let mut c = candidate();
            c.key = format!("event:{i}");
            db.insert_notification(&c, 1000, false, true).unwrap();
        }
        db.prune_notifications(1001).unwrap();
        assert_eq!(db.list_notifications().unwrap().len(), 1000);
        let mut c = candidate();
        c.key = "event:0".into();
        assert!(db
            .insert_notification(&c, 1002, false, true)
            .unwrap()
            .is_none());
        db.prune_notifications(1000 + 30 * 86400 + 1).unwrap();
        assert!(db.list_notifications().unwrap().is_empty());
        assert!(db
            .insert_notification(&c, 1000 + 30 * 86400 + 2, false, true)
            .unwrap()
            .is_none());
    }
    #[test]
    fn stored_preferences_keep_only_categories_still_in_the_contract() {
        let db = Db::open_in_memory().unwrap();
        // The settings UI echoes the loaded map straight back on save, so a key
        // retired from the contract has to disappear at the read boundary or
        // every later save is rejected as an incomplete category set.
        db.set_setting(
            "notifications-v1",
            r#"{"popups":false,"categories":{"trades":{"enabled":true,"native":true},"watches":{"enabled":true,"native":true},"scans":{"enabled":true,"native":true},"baro":{"enabled":false,"native":true},"calendar":{"enabled":true,"native":true},"digest":{"enabled":true,"native":true}}}"#,
        )
        .unwrap();
        let prefs = db.notification_preferences().unwrap();
        let mut keys: Vec<&str> = prefs.categories.keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut expected = crate::contracts::notification::CATEGORIES.to_vec();
        expected.sort_unstable();
        assert_eq!(keys, expected);
        assert!(!prefs.categories.contains_key("scans"));
        assert!(!prefs.popups);
        assert!(!prefs.categories["baro"].enabled);
        // The same predicate `set_notification_preferences` uses to accept a save.
        assert!(crate::contracts::notification::categories_match_contract(
            &prefs.categories
        ));
    }
    #[test]
    fn prune_drops_rows_whose_category_left_the_contract() {
        let db = Db::open_in_memory().unwrap();
        let mut stale = candidate();
        stale.key = "scan:1".into();
        stale.category = "scans".into();
        assert!(db
            .insert_notification(&stale, 1000, false, true)
            .unwrap()
            .is_some());
        assert!(db
            .insert_notification(&candidate(), 1001, false, true)
            .unwrap()
            .is_some());
        assert_eq!(db.list_notifications().unwrap().len(), 2);
        db.prune_notifications(1001).unwrap();
        let rows = db.list_notifications().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].category, "baro");
    }
}
