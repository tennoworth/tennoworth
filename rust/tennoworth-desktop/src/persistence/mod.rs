mod inventory;
pub(crate) mod keyring_store;
mod notifications;
mod records;
mod schema;
mod settings;
pub(crate) mod snapshot;
mod trades;
mod watches;
pub(crate) use records::{
    ListingLogEntry, ListingLogRow, NewWatch, Reserve, SnapshotItem, SnapshotSummary, TradeRow,
    Watch,
};
use schema::MIGRATIONS;

// Canonical desktop state store (SQLite via rusqlite `bundled`). This is the
// single owner of the schema, the migration runner, and every SQL statement -
// call sites use the typed methods below and never write raw SQL. The schema is
// the one agreed in the product plan (C3), applied verbatim as the
// v1 migration.
//
// Two distinct concerns share this file:
//   - inventory HISTORY (`snapshot` / `snapshot_item`, plus `listing_log`) -
//     the profit-tracking substrate, appended from day one.
//   - app STATE (`setting` kv, `reserve` per-slug) - the desktop backing for
//     the persistence the browser keeps in localStorage.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension};
use wfm_core::poison::guard;

/// The open database. `Connection` is not `Sync`, so it lives behind a `Mutex`;
/// held as Tauri managed state (`State<'_, Db>`) and shared across commands.
/// Scans are already single-flighted upstream, so lock contention is a non-issue.
pub struct Db {
    conn: Mutex<Connection>,
    run_id: String,
}

impl Db {
    /// Open (creating if absent) the store at `path` and bring it to the latest
    /// schema version. Fails only on a genuine I/O / corruption problem - the
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
            run_id: wfm_core::identity::random_token(16),
        })
    }

    // ---- trades (ledger) ----

    // ---- watches ----

    #[cfg(test)]
    pub(crate) fn open_in_memory() -> rusqlite::Result<Db> {
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
) -> rusqlite::Result<Option<crate::services::allowance::Observation>> {
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
    observation: &crate::services::allowance::Observation,
) -> rusqlite::Result<()> {
    let raw = serde_json::to_string(observation)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    conn.execute("INSERT INTO trade_allowance (id, observation) VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET observation = excluded.observation", [raw])?;
    Ok(())
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
                "listing_log".to_string(),
                "notification".to_string(),
                "notification_checkpoint".to_string(),
                "reserve".to_string(),
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
        let path = temp_db_path();
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
        let position = crate::services::eelog::LogPosition {
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
        drop(db);
        std::fs::remove_file(path).unwrap();
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

        assert_eq!(
            db.insert_listing_log("plan-a", &[row("mag_prime_set", "ok")])
                .unwrap(),
            1
        );
        assert_eq!(
            db.insert_listing_log("plan-b", &[row("rhino_prime_set", "ok")])
                .unwrap(),
            1
        );

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
    fn resumed_plan_keeps_stable_history_without_duplicate_successes() {
        let db = Db::open_in_memory().unwrap();
        db.insert_listing_log("paused", &[row("first", "ok"), row("second", "pending")])
            .unwrap();
        let initial = db.list_listing_log(10).unwrap();
        assert_eq!(initial.len(), 1);
        let id = initial[0].id;
        db.insert_listing_log("paused", &[row("first", "ok"), row("second", "ok")])
            .unwrap();
        db.insert_listing_log("paused", &[row("first", "ok"), row("second", "ok")])
            .unwrap();
        let resumed = db.list_listing_log(10).unwrap();
        assert_eq!(resumed.len(), 2);
        assert_eq!(resumed.iter().find(|r| r.slug == "first").unwrap().id, id);
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
        // Shipped users are on v1. Build a v1 DB the way they have one - run
        // ONLY the first migration - put a row in it, then open it normally and
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
        db.insert_listing_log("plan-after", &[row("new_item", "ok")])
            .unwrap();
        assert_eq!(db.list_listing_log(10).unwrap().len(), 2);

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
        use crate::services::eelog::{TradeEvent, TradeItem};
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
        let mut position = crate::services::eelog::LogPosition {
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
        let market: crate::services::sellables::MarketData = serde_json::from_value(serde_json::json!({
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
        let boundary = crate::services::eelog::LogPosition {
            session: "digest-test".into(),
            start: 100,
            end: 100,
            observed_after: 100,
        };
        db.save_allowance(crate::services::allowance::Observation::scanned(
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
        db.set_reserve("test_part", 3).unwrap();
        let rows = crate::services::sellables::rank_sellables(&db, &market);
        assert_eq!(rows[0].sellable_qty, 3);
        assert_eq!(market.session_quantities(&db).unwrap()["test_part"], 3);
        let prices = serde_json::json!({"updated_at":stamp});
        assert!(
            crate::services::reminders::digest(&prices, &rows, stamp, 110, "1970-01-01", 18)
                .unwrap()
                .body
                .contains("Test Part ×3")
        );
        db.set_setting("reserve-copies", "4").unwrap();
        assert_eq!(
            crate::services::sellables::rank_sellables(&db, &market)[0].sellable_qty,
            2
        );
        db.set_reserve("test_part", 6).unwrap();
        assert!(crate::services::sellables::rank_sellables(&db, &market).is_empty());
        db.set_setting("reserve-copies", "1").unwrap();
        db.set_reserve("test_part", 3).unwrap();
        let trade = crate::services::eelog::TradeEvent {
            partner: "Buyer".into(),
            kind: "sale".into(),
            plat: 10,
            log_stamp: Some("110".into()),
            items: vec![crate::services::eelog::TradeItem {
                name: "Test Part".into(),
                qty: 1,
                direction: "given".into(),
            }],
        };
        let position = crate::services::eelog::LogPosition {
            start: 101,
            end: 120,
            observed_after: 110,
            ..boundary
        };
        db.insert_trade(&trade, 110, &position).unwrap();
        let rows = crate::services::sellables::rank_sellables(&db, &market);
        assert!(rows.is_empty());
        assert!(!market
            .session_quantities(&db)
            .unwrap()
            .contains_key("test_part"));
        assert!(
            crate::services::reminders::digest(&prices, &rows, stamp, 111, "1970-01-01", 18)
                .is_none()
        );
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
        let boundary = crate::services::eelog::LogPosition {
            start: 120,
            end: 120,
            observed_after: 130,
            ..position
        };
        db.save_allowance(crate::services::allowance::Observation::scanned(
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
            crate::services::sellables::rank_sellables(&db, &market)[0].sellable_qty,
            2
        );
        assert_eq!(market.session_quantities(&db).unwrap()["test_part"], 2);
    }

    fn allowance_scan(db: &Db, account: &str, remaining: u32) {
        let id = db.insert_snapshot("memory", None, None, &[]).unwrap();
        let before = crate::services::eelog::LogPosition {
            session: "log".into(),
            start: 100,
            end: 100,
            observed_after: 100,
        };
        let after = crate::services::eelog::LogPosition {
            start: 200,
            end: 200,
            observed_after: 102,
            ..before.clone()
        };
        db.save_allowance(crate::services::allowance::Observation::scanned(
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

    fn allowance_trade(kind: &str) -> crate::services::eelog::TradeEvent {
        crate::services::eelog::TradeEvent {
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
            let position = crate::services::eelog::LogPosition {
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
        let position = crate::services::eelog::LogPosition {
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
        let position = crate::services::eelog::LogPosition {
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
        db.save_allowance(crate::services::allowance::Observation::scanned(
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
        let position = crate::services::eelog::LogPosition {
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
            crate::services::allowance::Confidence::Estimated
        );
        assert!(!view.monitoring);
        assert_eq!(
            db.insert_trade(&allowance_trade("sale"), 4000, &position)
                .unwrap(),
            None
        );
        let next = crate::services::eelog::LogPosition {
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
    use crate::services::notifications::Candidate;
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
}
