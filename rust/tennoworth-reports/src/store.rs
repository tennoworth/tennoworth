//! Raw reports live only for the current and the previous ISO week, the two
//! weeks a client can still add to or erase. Older weeks are folded into
//! published aggregates and their raw rows deleted, so a contributor id never
//! outlives the week after it was used.

use crate::contract::{iso_week, ValidBatch, ValidErase, ValidShape};
use axum::http::StatusCode;
use chrono::{DateTime, Days, NaiveDate, Utc};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Distinct contributors an item needs in a week before its prices are
/// published. It is the privacy floor and the poisoning floor at once: one
/// contributor gets one vote, and a thin item stays unpublished.
pub const MIN_CONTRIBUTORS: u32 = 5;
/// No client needs more than this in a week; past it a contributor id is
/// someone filling the table, and its vote is capped at one regardless.
pub const MAX_SALES_PER_CONTRIBUTOR_WEEK: i64 = 500;
/// Folded weeks returned by the public endpoints, besides the two live ones.
const PUBLISHED_WEEKS: i64 = 8;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PriceRow {
    pub side: String,
    pub slug: String,
    pub tiered: bool,
    pub contributors: u32,
    pub units: u64,
    pub p25: f64,
    pub median: f64,
    pub p75: f64,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PriceWeek {
    pub week: String,
    /// False while the week can still receive late or erased reports.
    pub complete: bool,
    pub items: Vec<PriceRow>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Prices {
    pub updated_at: String,
    pub min_contributors: u32,
    pub weeks: Vec<PriceWeek>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ShapeCount {
    pub shape: serde_json::Value,
    pub contributors: u32,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ShapeWeek {
    pub week: String,
    pub complete: bool,
    pub shapes: Vec<ShapeCount>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Shapes {
    pub updated_at: String,
    pub weeks: Vec<ShapeWeek>,
}

pub struct RawSale {
    pub side: String,
    pub slug: String,
    pub tiered: bool,
    pub contributor: String,
    pub qty: u32,
    pub plat: u32,
}

/// Linear interpolation between closest ranks; `sorted` is non-empty.
fn quantile(sorted: &[f64], p: f64) -> f64 {
    let pos = sorted.len().saturating_sub(1) as f64 * p;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    let (Some(a), Some(b)) = (sorted.get(lo), sorted.get(hi)) else {
        return 0.0;
    };
    a + (b - a) * (pos - lo as f64)
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// Each contributor's median unit price is one vote; the published band is the
/// spread of those votes. Sending more sales buys no extra weight.
pub fn aggregate(rows: &[RawSale], min_contributors: u32) -> Vec<PriceRow> {
    type Key<'a> = (&'a str, &'a str, bool);
    let mut groups: BTreeMap<Key<'_>, (BTreeMap<&str, Vec<f64>>, u64)> = BTreeMap::new();
    for row in rows {
        let entry = groups
            .entry((row.side.as_str(), row.slug.as_str(), row.tiered))
            .or_default();
        entry
            .0
            .entry(row.contributor.as_str())
            .or_default()
            .push(f64::from(row.plat) / f64::from(row.qty));
        entry.1 += u64::from(row.qty);
    }
    let mut out = Vec::new();
    for ((side, slug, tiered), (by_contributor, units)) in groups {
        let Ok(contributors) = u32::try_from(by_contributor.len()) else {
            continue;
        };
        if contributors < min_contributors {
            continue;
        }
        let mut votes: Vec<f64> = by_contributor
            .into_values()
            .map(|mut prices| {
                prices.sort_by(f64::total_cmp);
                quantile(&prices, 0.5)
            })
            .collect();
        votes.sort_by(f64::total_cmp);
        out.push(PriceRow {
            side: side.to_string(),
            slug: slug.to_string(),
            tiered,
            contributors,
            units,
            p25: round1(quantile(&votes, 0.25)),
            median: round1(quantile(&votes, 0.5)),
            p75: round1(quantile(&votes, 0.75)),
        });
    }
    out
}

fn unavailable<E>(_: E) -> StatusCode {
    StatusCode::SERVICE_UNAVAILABLE
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn existing(path: &Path) -> anyhow::Result<Self> {
        Self::new(Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
        )?)
    }
    pub fn open(path: &Path, now: DateTime<Utc>) -> anyhow::Result<Self> {
        let mut store = Self::new(Connection::open(path)?)?;
        store.tick(now)?;
        Ok(store)
    }
    pub fn new(conn: Connection) -> anyhow::Result<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode=DELETE; PRAGMA secure_delete=ON;
            PRAGMA max_page_count=32768;
            CREATE TABLE IF NOT EXISTS sale (
              contributor TEXT NOT NULL, ref TEXT NOT NULL, week TEXT NOT NULL,
              erase TEXT NOT NULL, side TEXT NOT NULL, slug TEXT NOT NULL,
              tiered INTEGER NOT NULL, qty INTEGER NOT NULL, plat INTEGER NOT NULL,
              PRIMARY KEY (contributor, ref));
            CREATE INDEX IF NOT EXISTS sale_week ON sale(week);
            CREATE TABLE IF NOT EXISTS shape (
              week TEXT NOT NULL, contributor TEXT NOT NULL, erase TEXT NOT NULL,
              shape TEXT NOT NULL, PRIMARY KEY (week, contributor, shape));
            CREATE TABLE IF NOT EXISTS published_price (
              week TEXT NOT NULL, side TEXT NOT NULL, slug TEXT NOT NULL,
              tiered INTEGER NOT NULL, contributors INTEGER NOT NULL,
              units INTEGER NOT NULL, p25 REAL NOT NULL, median REAL NOT NULL,
              p75 REAL NOT NULL, PRIMARY KEY (week, side, slug, tiered));
            CREATE TABLE IF NOT EXISTS published_shape (
              week TEXT NOT NULL, shape TEXT NOT NULL, contributors INTEGER NOT NULL,
              PRIMARY KEY (week, shape));
            CREATE TABLE IF NOT EXISTS health (id INTEGER PRIMARY KEY CHECK(id=1), last_tick TEXT NOT NULL);",
        )?;
        Ok(Self { conn })
    }

    /// The oldest week still open: a sale up to seven days old lands in it.
    fn live_since(today: NaiveDate) -> anyhow::Result<String> {
        let back = today
            .checked_sub_days(Days::new(7))
            .ok_or_else(|| anyhow::anyhow!("date out of range"))?;
        Ok(iso_week(back))
    }

    pub fn tick(&mut self, now: DateTime<Utc>) -> anyhow::Result<()> {
        let live = Self::live_since(now.date_naive())?;
        let tx = self.conn.transaction()?;
        let last: Option<String> = tx
            .query_row("SELECT last_tick FROM health WHERE id=1", [], |r| r.get(0))
            .optional()?;
        // Folding is irreversible; a clock that jumped back must not reopen weeks.
        if let Some(last) = last.and_then(|s| DateTime::parse_from_rfc3339(&s).ok()) {
            anyhow::ensure!(now >= last.with_timezone(&Utc), "clock moved backwards");
        }
        let closed: Vec<String> = tx
            .prepare("SELECT DISTINCT week FROM sale WHERE week < ?1 UNION SELECT DISTINCT week FROM shape WHERE week < ?1")?
            .query_map([&live], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for week in closed {
            let rows = Self::raw_sales(&tx, &week)?;
            for row in aggregate(&rows, MIN_CONTRIBUTORS) {
                tx.execute(
                    "INSERT OR REPLACE INTO published_price VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                    (
                        &week,
                        &row.side,
                        &row.slug,
                        row.tiered,
                        row.contributors,
                        row.units,
                        row.p25,
                        row.median,
                        row.p75,
                    ),
                )?;
            }
            tx.execute(
                "INSERT OR REPLACE INTO published_shape
                 SELECT week, shape, COUNT(DISTINCT contributor) FROM shape WHERE week=?1 GROUP BY shape",
                [&week],
            )?;
            tx.execute("DELETE FROM sale WHERE week=?1", [&week])?;
            tx.execute("DELETE FROM shape WHERE week=?1", [&week])?;
        }
        tx.execute(
            "INSERT INTO health VALUES (1,?1) ON CONFLICT(id) DO UPDATE SET last_tick=excluded.last_tick",
            [now.to_rfc3339()],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn raw_sales(conn: &Connection, week: &str) -> rusqlite::Result<Vec<RawSale>> {
        conn.prepare("SELECT side, slug, tiered, contributor, qty, plat FROM sale WHERE week=?1")?
            .query_map([week], |r| {
                Ok(RawSale {
                    side: r.get(0)?,
                    slug: r.get(1)?,
                    tiered: r.get(2)?,
                    contributor: r.get(3)?,
                    qty: r.get(4)?,
                    plat: r.get(5)?,
                })
            })?
            .collect()
    }

    pub fn add_sales(&mut self, batch: ValidBatch) -> Result<(), StatusCode> {
        let tx = self.conn.transaction().map_err(unavailable)?;
        let held: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM sale WHERE contributor=?1",
                [&batch.contributor],
                |r| r.get(0),
            )
            .map_err(unavailable)?;
        let incoming = i64::try_from(batch.sales.len()).map_err(unavailable)?;
        if held + incoming > MAX_SALES_PER_CONTRIBUTOR_WEEK {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        for sale in &batch.sales {
            tx.execute(
                "INSERT OR IGNORE INTO sale VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                (
                    &batch.contributor,
                    &sale.reference,
                    &batch.week,
                    &batch.erase,
                    sale.side.as_str(),
                    &sale.slug,
                    sale.tiered,
                    sale.qty,
                    sale.plat,
                ),
            )
            .map_err(unavailable)?;
        }
        tx.commit().map_err(unavailable)
    }

    pub fn add_shape(&mut self, shape: ValidShape, now: DateTime<Utc>) -> Result<(), StatusCode> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO shape VALUES (?1,?2,?3,?4)",
                (
                    iso_week(now.date_naive()),
                    &shape.contributor,
                    &shape.erase,
                    &shape.shape,
                ),
            )
            .map(|_| ())
            .map_err(unavailable)
    }

    /// Only live weeks hold erasable rows; folded weeks keep no contributor.
    pub fn erase(&mut self, request: &ValidErase) -> Result<(), StatusCode> {
        let tx = self.conn.transaction().map_err(unavailable)?;
        tx.execute(
            "DELETE FROM sale WHERE week=?1 AND erase=?2",
            (&request.week, &request.erase),
        )
        .map_err(unavailable)?;
        tx.execute(
            "DELETE FROM shape WHERE week=?1 AND erase=?2",
            (&request.week, &request.erase),
        )
        .map_err(unavailable)?;
        tx.commit().map_err(unavailable)
    }

    fn updated_at(&self) -> anyhow::Result<String> {
        Ok(self
            .conn
            .query_row("SELECT last_tick FROM health WHERE id=1", [], |r| r.get(0))?)
    }

    /// Weeks to show: the recent folded ones (all of them when `all`), then the
    /// live weeks computed from raw rows under the same contributor floor.
    fn folded_weeks(&self, table: &str, all: bool) -> anyhow::Result<Vec<String>> {
        let limit = if all { -1 } else { PUBLISHED_WEEKS };
        let sql = format!(
            "SELECT week FROM (SELECT DISTINCT week FROM {table} ORDER BY week DESC LIMIT ?1) ORDER BY week"
        );
        Ok(self
            .conn
            .prepare(&sql)?
            .query_map([limit], |r| r.get(0))?
            .collect::<Result<_, _>>()?)
    }

    fn live_weeks(today: NaiveDate) -> anyhow::Result<Vec<String>> {
        let previous = Self::live_since(today)?;
        let current = iso_week(today);
        Ok(if previous == current {
            vec![current]
        } else {
            vec![previous, current]
        })
    }

    pub fn prices(&self, now: DateTime<Utc>, all: bool) -> anyhow::Result<Prices> {
        let mut weeks = Vec::new();
        for week in self.folded_weeks("published_price", all)? {
            let items = self
                .conn
                .prepare(
                    "SELECT side, slug, tiered, contributors, units, p25, median, p75
                     FROM published_price WHERE week=?1 ORDER BY side, slug, tiered",
                )?
                .query_map([&week], |r| {
                    Ok(PriceRow {
                        side: r.get(0)?,
                        slug: r.get(1)?,
                        tiered: r.get(2)?,
                        contributors: r.get(3)?,
                        units: r.get(4)?,
                        p25: r.get(5)?,
                        median: r.get(6)?,
                        p75: r.get(7)?,
                    })
                })?
                .collect::<Result<_, _>>()?;
            weeks.push(PriceWeek {
                week,
                complete: true,
                items,
            });
        }
        for week in Self::live_weeks(now.date_naive())? {
            let items = aggregate(&Self::raw_sales(&self.conn, &week)?, MIN_CONTRIBUTORS);
            weeks.push(PriceWeek {
                week,
                complete: false,
                items,
            });
        }
        Ok(Prices {
            updated_at: self.updated_at()?,
            min_contributors: MIN_CONTRIBUTORS,
            weeks,
        })
    }

    pub fn shapes(&self, now: DateTime<Utc>) -> anyhow::Result<Shapes> {
        let read = |r: &rusqlite::Row<'_>| -> rusqlite::Result<(String, u32)> {
            Ok((r.get(0)?, r.get(1)?))
        };
        let mut weeks = Vec::new();
        let mut push = |week: String, complete: bool, rows: Vec<(String, u32)>| {
            let shapes = rows
                .into_iter()
                .filter_map(|(shape, contributors)| {
                    Some(ShapeCount {
                        shape: serde_json::from_str(&shape).ok()?,
                        contributors,
                    })
                })
                .collect();
            weeks.push(ShapeWeek {
                week,
                complete,
                shapes,
            });
        };
        for week in self.folded_weeks("published_shape", false)? {
            let rows = self
                .conn
                .prepare(
                    "SELECT shape, contributors FROM published_shape WHERE week=?1 ORDER BY shape",
                )?
                .query_map([&week], read)?
                .collect::<Result<_, _>>()?;
            push(week, true, rows);
        }
        for week in Self::live_weeks(now.date_naive())? {
            let rows = self
                .conn
                .prepare(
                    "SELECT shape, COUNT(DISTINCT contributor) FROM shape WHERE week=?1
                     GROUP BY shape ORDER BY shape",
                )?
                .query_map([&week], read)?
                .collect::<Result<_, _>>()?;
            push(week, false, rows);
        }
        Ok(Shapes {
            updated_at: self.updated_at()?,
            weeks,
        })
    }

    pub fn healthy(&self, now: DateTime<Utc>) -> bool {
        self.updated_at()
            .ok()
            .and_then(|t| DateTime::parse_from_rfc3339(&t).ok())
            .is_some_and(|t| (0..=120).contains(&now.signed_duration_since(t).num_seconds()))
    }

    #[cfg(test)]
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }
}
