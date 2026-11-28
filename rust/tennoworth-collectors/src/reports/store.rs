//! Raw reports live only for the current and the previous ISO week, the two
//! weeks a client can still add to or erase. Older weeks are folded into
//! published aggregates and their raw rows deleted, so a contributor id never
//! outlives the week after it was used.

use super::contract::{
    iso_week, Suggestion, ValidBatch, ValidErase, ValidRivenBatch, ValidShape, ValidVote, Verdict,
};
use crate::{add_days, rfc3339, utc_date};
use axum::http::StatusCode;
use jiff::civil::Date;
use jiff::Timestamp;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Distinct contributors an item needs in a week before its prices are
/// published. It is the privacy floor and the poisoning floor at once: one
/// contributor gets one vote, and a thin item stays unpublished.
pub const MIN_CONTRIBUTORS: u32 = 5;
/// No client needs more than this in a week; past it a contributor id is
/// someone filling the table, and its vote is capped at one regardless.
pub const MAX_SALES_PER_CONTRIBUTOR_WEEK: i64 = 500;
/// Riven sales one contributor id may hold in a week; a player selling more
/// is trading rivens for a living, and still gets one vote per weapon.
pub const MAX_RIVEN_SALES_PER_CONTRIBUTOR_WEEK: i64 = 200;
/// Weapons one contributor id may rate in a week.
pub const MAX_VOTES_PER_CONTRIBUTOR_WEEK: i64 = 200;
/// Voters on a suggestion who also reported selling a riven for that weapon
/// the same week. New ids are free; a reported sale costs a real trade.
pub const MIN_TRADED_VOTERS: u32 = 2;
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

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RivenPriceRow {
    pub weapon: String,
    pub verdict: Verdict,
    pub contributors: u32,
    pub sales: u64,
    pub p25: f64,
    pub median: f64,
    pub p75: f64,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RivenPriceWeek {
    pub week: String,
    pub complete: bool,
    pub rivens: Vec<RivenPriceRow>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RivenPrices {
    pub updated_at: String,
    pub min_contributors: u32,
    pub weeks: Vec<RivenPriceWeek>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VerdictVotes {
    pub verdict: Verdict,
    pub voters: u32,
    pub agree: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SuggestionVotes {
    pub stat: String,
    pub positive: bool,
    pub good: bool,
    pub voters: u32,
    /// Voters who also reported selling a riven for this weapon that week.
    pub traded: u32,
    /// Enough independent and trading voters, and a majority of the weapon's
    /// voters, back it: a maintainer should look at it.
    pub proposed: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WeaponVotes {
    pub weapon: String,
    pub voters: u32,
    pub traded: u32,
    pub verdicts: Vec<VerdictVotes>,
    pub suggestions: Vec<SuggestionVotes>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VoteWeek {
    pub week: String,
    pub complete: bool,
    pub weapons: Vec<WeaponVotes>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RollVotes {
    pub updated_at: String,
    pub min_contributors: u32,
    pub min_traded: u32,
    pub weeks: Vec<VoteWeek>,
}

pub struct RawRivenSale {
    pub weapon: String,
    pub verdict: Verdict,
    pub contributor: String,
    pub plat: u32,
}

pub struct RawVote {
    pub contributor: String,
    pub weapon: String,
    pub verdict: Verdict,
    pub agree: bool,
    pub suggest: Option<Suggestion>,
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

/// The spread of the contributors' own median prices: each contributor is one
/// vote, however many sales they sent. `by_contributor` is non-empty.
fn vote_band(by_contributor: BTreeMap<&str, Vec<f64>>) -> (f64, f64, f64) {
    let mut votes: Vec<f64> = by_contributor
        .into_values()
        .map(|mut prices| {
            prices.sort_by(f64::total_cmp);
            quantile(&prices, 0.5)
        })
        .collect();
    votes.sort_by(f64::total_cmp);
    (
        round1(quantile(&votes, 0.25)),
        round1(quantile(&votes, 0.5)),
        round1(quantile(&votes, 0.75)),
    )
}

/// Riven prices per weapon and verdict, under the same floor and one vote per
/// contributor as item prices.
pub fn aggregate_rivens(rows: &[RawRivenSale], min_contributors: u32) -> Vec<RivenPriceRow> {
    // Per weapon and verdict: each contributor's prices, and the sale count.
    type Group<'a> = (BTreeMap<&'a str, Vec<f64>>, u64);
    let mut groups: BTreeMap<(&str, Verdict), Group> = BTreeMap::new();
    for row in rows {
        let entry = groups
            .entry((row.weapon.as_str(), row.verdict))
            .or_default();
        entry
            .0
            .entry(row.contributor.as_str())
            .or_default()
            .push(f64::from(row.plat));
        entry.1 += 1;
    }
    let mut out = Vec::new();
    for ((weapon, verdict), (by_contributor, sales)) in groups {
        let Ok(contributors) = u32::try_from(by_contributor.len()) else {
            continue;
        };
        if contributors < min_contributors {
            continue;
        }
        let (p25, median, p75) = vote_band(by_contributor);
        out.push(RivenPriceRow {
            weapon: weapon.to_string(),
            verdict,
            contributors,
            sales,
            p25,
            median,
            p75,
        });
    }
    out
}

/// Ratings per weapon. A contributor holds one vote per weapon per week, so
/// every count is of distinct contributor ids. A weapon is published only
/// with `min_contributors` voters, and a suggestion is proposed only when that
/// many back it, `MIN_TRADED_VOTERS` of them sold a riven for the weapon
/// (`sellers`: contributor and weapon), and they are most of its voters.
pub fn aggregate_votes(
    votes: &[RawVote],
    sellers: &BTreeSet<(String, String)>,
    min_contributors: u32,
) -> Vec<WeaponVotes> {
    let mut by_weapon: BTreeMap<&str, Vec<&RawVote>> = BTreeMap::new();
    for vote in votes {
        by_weapon
            .entry(vote.weapon.as_str())
            .or_default()
            .push(vote);
    }
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    let mut out = Vec::new();
    for (weapon, votes) in by_weapon {
        let voters = count(votes.len());
        if voters < min_contributors {
            continue;
        }
        let traded = |v: &RawVote| sellers.contains(&(v.contributor.clone(), weapon.to_string()));
        let mut verdicts: BTreeMap<Verdict, (u32, u32)> = BTreeMap::new();
        let mut suggestions: BTreeMap<&Suggestion, (u32, u32)> = BTreeMap::new();
        for vote in &votes {
            let entry = verdicts.entry(vote.verdict).or_default();
            entry.0 += 1;
            entry.1 += u32::from(vote.agree);
            if let Some(suggest) = &vote.suggest {
                let entry = suggestions.entry(suggest).or_default();
                entry.0 += 1;
                entry.1 += u32::from(traded(vote));
            }
        }
        out.push(WeaponVotes {
            weapon: weapon.to_string(),
            voters,
            traded: count(votes.iter().filter(|v| traded(v)).count()),
            verdicts: verdicts
                .into_iter()
                .map(|(verdict, (voters, agree))| VerdictVotes {
                    verdict,
                    voters,
                    agree,
                })
                .collect(),
            suggestions: suggestions
                .into_iter()
                .map(|(s, (backers, traded))| SuggestionVotes {
                    stat: s.stat.clone(),
                    positive: s.positive,
                    good: s.good,
                    voters: backers,
                    traded,
                    proposed: backers >= min_contributors
                        && traded >= MIN_TRADED_VOTERS
                        && backers * 2 > voters,
                })
                .collect(),
        });
    }
    out
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
        let (p25, median, p75) = vote_band(by_contributor);
        out.push(PriceRow {
            side: side.to_string(),
            slug: slug.to_string(),
            tiered,
            contributors,
            units,
            p25,
            median,
            p75,
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
    pub fn open(path: &Path, now: Timestamp) -> anyhow::Result<Self> {
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
            CREATE TABLE IF NOT EXISTS riven_sale (
              contributor TEXT NOT NULL, ref TEXT NOT NULL, week TEXT NOT NULL,
              erase TEXT NOT NULL, weapon TEXT NOT NULL, verdict TEXT NOT NULL,
              plat INTEGER NOT NULL, PRIMARY KEY (contributor, ref));
            CREATE INDEX IF NOT EXISTS riven_sale_week ON riven_sale(week);
            CREATE TABLE IF NOT EXISTS roll_vote (
              week TEXT NOT NULL, contributor TEXT NOT NULL, erase TEXT NOT NULL,
              weapon TEXT NOT NULL, verdict TEXT NOT NULL, agree INTEGER NOT NULL,
              suggest TEXT, PRIMARY KEY (week, contributor, weapon));
            CREATE TABLE IF NOT EXISTS published_riven_price (
              week TEXT NOT NULL, weapon TEXT NOT NULL, row TEXT NOT NULL,
              PRIMARY KEY (week, weapon, row));
            CREATE TABLE IF NOT EXISTS published_roll_vote (
              week TEXT NOT NULL, weapon TEXT NOT NULL, row TEXT NOT NULL,
              PRIMARY KEY (week, weapon));
            CREATE TABLE IF NOT EXISTS health (id INTEGER PRIMARY KEY CHECK(id=1), last_tick TEXT NOT NULL);",
        )?;
        Ok(Self { conn })
    }

    /// The oldest week still open: a sale up to seven days old lands in it.
    fn live_since(today: Date) -> anyhow::Result<String> {
        let back = add_days(today, -7).ok_or_else(|| anyhow::anyhow!("date out of range"))?;
        Ok(iso_week(back))
    }

    pub fn tick(&mut self, now: Timestamp) -> anyhow::Result<()> {
        let live = Self::live_since(utc_date(now))?;
        let tx = self.conn.transaction()?;
        let last: Option<String> = tx
            .query_row("SELECT last_tick FROM health WHERE id=1", [], |r| r.get(0))
            .optional()?;
        // Folding is irreversible; a clock that jumped back must not reopen weeks.
        if let Some(last) = last.and_then(|s| s.parse::<Timestamp>().ok()) {
            anyhow::ensure!(now >= last, "clock moved backwards");
        }
        let closed: Vec<String> = tx
            .prepare(
                "SELECT DISTINCT week FROM sale WHERE week < ?1
                 UNION SELECT DISTINCT week FROM shape WHERE week < ?1
                 UNION SELECT DISTINCT week FROM riven_sale WHERE week < ?1
                 UNION SELECT DISTINCT week FROM roll_vote WHERE week < ?1",
            )?
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
            for row in aggregate_rivens(&Self::raw_riven_sales(&tx, &week)?, MIN_CONTRIBUTORS) {
                tx.execute(
                    "INSERT OR REPLACE INTO published_riven_price VALUES (?1,?2,?3)",
                    (&week, &row.weapon, serde_json::to_string(&row)?),
                )?;
            }
            for row in Self::week_votes(&tx, &week)? {
                tx.execute(
                    "INSERT OR REPLACE INTO published_roll_vote VALUES (?1,?2,?3)",
                    (&week, &row.weapon, serde_json::to_string(&row)?),
                )?;
            }
            for table in ["sale", "shape", "riven_sale", "roll_vote"] {
                tx.execute(&format!("DELETE FROM {table} WHERE week=?1"), [&week])?;
            }
        }
        tx.execute(
            "INSERT INTO health VALUES (1,?1) ON CONFLICT(id) DO UPDATE SET last_tick=excluded.last_tick",
            [rfc3339(now)],
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

    fn raw_riven_sales(conn: &Connection, week: &str) -> rusqlite::Result<Vec<RawRivenSale>> {
        conn.prepare("SELECT weapon, verdict, contributor, plat FROM riven_sale WHERE week=?1")?
            .query_map([week], |r| {
                let verdict: String = r.get(1)?;
                Ok(RawRivenSale {
                    weapon: r.get(0)?,
                    verdict: Verdict::parse(&verdict).unwrap_or(Verdict::Unrated),
                    contributor: r.get(2)?,
                    plat: r.get(3)?,
                })
            })?
            .collect()
    }

    /// The week's ratings, with who also sold a riven for the weapon.
    fn week_votes(conn: &Connection, week: &str) -> anyhow::Result<Vec<WeaponVotes>> {
        let votes: Vec<RawVote> = conn
            .prepare(
                "SELECT contributor, weapon, verdict, agree, suggest FROM roll_vote WHERE week=?1",
            )?
            .query_map([week], |r| {
                let verdict: String = r.get(2)?;
                let suggest: Option<String> = r.get(4)?;
                Ok(RawVote {
                    contributor: r.get(0)?,
                    weapon: r.get(1)?,
                    verdict: Verdict::parse(&verdict).unwrap_or(Verdict::Unrated),
                    agree: r.get(3)?,
                    suggest: suggest.and_then(|s| serde_json::from_str(&s).ok()),
                })
            })?
            .collect::<Result<_, _>>()?;
        let sellers: BTreeSet<(String, String)> = conn
            .prepare("SELECT DISTINCT contributor, weapon FROM riven_sale WHERE week=?1")?
            .query_map([week], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        Ok(aggregate_votes(&votes, &sellers, MIN_CONTRIBUTORS))
    }

    pub fn add_riven_sales(&mut self, batch: ValidRivenBatch) -> Result<(), StatusCode> {
        let tx = self.conn.transaction().map_err(unavailable)?;
        for sale in &batch.sales {
            tx.execute(
                "INSERT OR IGNORE INTO riven_sale VALUES (?1,?2,?3,?4,?5,?6,?7)",
                (
                    &batch.contributor,
                    &sale.reference,
                    &batch.week,
                    &batch.erase,
                    &sale.weapon,
                    sale.verdict.as_str(),
                    sale.plat,
                ),
            )
            .map_err(unavailable)?;
        }
        let held: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM riven_sale WHERE contributor=?1",
                [&batch.contributor],
                |r| r.get(0),
            )
            .map_err(unavailable)?;
        if held > MAX_RIVEN_SALES_PER_CONTRIBUTOR_WEEK {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        tx.commit().map_err(unavailable)
    }

    /// A later rating of the same weapon in the same week replaces the earlier
    /// one, so a player can change their mind without counting twice.
    pub fn add_vote(&mut self, vote: ValidVote, now: Timestamp) -> Result<(), StatusCode> {
        let week = iso_week(utc_date(now));
        let suggest = vote
            .suggest
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(unavailable)?;
        let tx = self.conn.transaction().map_err(unavailable)?;
        tx.execute(
            "INSERT OR REPLACE INTO roll_vote VALUES (?1,?2,?3,?4,?5,?6,?7)",
            (
                &week,
                &vote.contributor,
                &vote.erase,
                &vote.weapon,
                vote.verdict.as_str(),
                vote.agree,
                suggest,
            ),
        )
        .map_err(unavailable)?;
        let held: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM roll_vote WHERE week=?1 AND contributor=?2",
                (&week, &vote.contributor),
                |r| r.get(0),
            )
            .map_err(unavailable)?;
        if held > MAX_VOTES_PER_CONTRIBUTOR_WEEK {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        tx.commit().map_err(unavailable)
    }

    pub fn add_sales(&mut self, batch: ValidBatch) -> Result<(), StatusCode> {
        let tx = self.conn.transaction().map_err(unavailable)?;
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
        // Enforce the cap on actual inserts so lost acknowledgements and repeated
        // references remain idempotent even when the contributor is at the cap.
        let held: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM sale WHERE contributor=?1",
                [&batch.contributor],
                |r| r.get(0),
            )
            .map_err(unavailable)?;
        if held > MAX_SALES_PER_CONTRIBUTOR_WEEK {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        tx.commit().map_err(unavailable)
    }

    pub fn add_shape(&mut self, shape: ValidShape, now: Timestamp) -> Result<(), StatusCode> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO shape VALUES (?1,?2,?3,?4)",
                (
                    iso_week(utc_date(now)),
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
        for table in ["shape", "riven_sale", "roll_vote"] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE week=?1 AND erase=?2"),
                (&request.week, &request.erase),
            )
            .map_err(unavailable)?;
        }
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

    fn live_weeks(today: Date) -> anyhow::Result<Vec<String>> {
        let previous = Self::live_since(today)?;
        let current = iso_week(today);
        Ok(if previous == current {
            vec![current]
        } else {
            vec![previous, current]
        })
    }

    pub fn prices(&self, now: Timestamp, all: bool) -> anyhow::Result<Prices> {
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
        for week in Self::live_weeks(utc_date(now))? {
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

    pub fn shapes(&self, now: Timestamp) -> anyhow::Result<Shapes> {
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
        for week in Self::live_weeks(utc_date(now))? {
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

    pub fn riven_prices(&self, now: Timestamp, all: bool) -> anyhow::Result<RivenPrices> {
        let mut weeks = Vec::new();
        for week in self.folded_weeks("published_riven_price", all)? {
            let rivens = self.folded_rows("published_riven_price", &week)?;
            weeks.push(RivenPriceWeek {
                week,
                complete: true,
                rivens,
            });
        }
        for week in Self::live_weeks(utc_date(now))? {
            let rivens =
                aggregate_rivens(&Self::raw_riven_sales(&self.conn, &week)?, MIN_CONTRIBUTORS);
            weeks.push(RivenPriceWeek {
                week,
                complete: false,
                rivens,
            });
        }
        Ok(RivenPrices {
            updated_at: self.updated_at()?,
            min_contributors: MIN_CONTRIBUTORS,
            weeks,
        })
    }

    pub fn roll_votes(&self, now: Timestamp, all: bool) -> anyhow::Result<RollVotes> {
        let mut weeks = Vec::new();
        for week in self.folded_weeks("published_roll_vote", all)? {
            let weapons = self.folded_rows("published_roll_vote", &week)?;
            weeks.push(VoteWeek {
                week,
                complete: true,
                weapons,
            });
        }
        for week in Self::live_weeks(utc_date(now))? {
            let weapons = Self::week_votes(&self.conn, &week)?;
            weeks.push(VoteWeek {
                week,
                complete: false,
                weapons,
            });
        }
        Ok(RollVotes {
            updated_at: self.updated_at()?,
            min_contributors: MIN_CONTRIBUTORS,
            min_traded: MIN_TRADED_VOTERS,
            weeks,
        })
    }

    /// A folded week's rows, stored as the JSON they are published as.
    fn folded_rows<T: serde::de::DeserializeOwned>(
        &self,
        table: &str,
        week: &str,
    ) -> anyhow::Result<Vec<T>> {
        let rows: Vec<String> = self
            .conn
            .prepare(&format!(
                "SELECT row FROM {table} WHERE week=?1 ORDER BY weapon, row"
            ))?
            .query_map([week], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        rows.iter()
            .map(|row| Ok(serde_json::from_str(row)?))
            .collect()
    }

    pub fn healthy(&self, now: Timestamp) -> bool {
        self.updated_at()
            .ok()
            .and_then(|t| t.parse::<Timestamp>().ok())
            .is_some_and(|t| (0..=120).contains(&now.duration_since(t).as_secs()))
    }

    #[cfg(test)]
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }
}
