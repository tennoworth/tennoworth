//! Opt-in price reports: sale prices from the trade ledger, sent anonymously to
//! tennoworth.app. docs/price-reports.md owns the contract and its privacy
//! terms; tests/fixtures/reports/contract.json pins the wire format and the
//! id derivation on both sides.
//!
//! The ledger is the queue. Opting in records the newest trade id, and only
//! trades recorded after it are ever read; the mark advances as batches are
//! accepted, so nothing is copied into a second store.
#![allow(
    clippy::unreachable,
    reason = "tauri::command injects unreachable code into async wrappers"
)]

use crate::persistence::{Db, TradeRow};
use crate::services::sellables::MarketData;
use crate::services::usage;
use chrono::{DateTime, Datelike, Days, NaiveDate, Utc};
use hmac::{Hmac, Mac};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tauri::{Manager, State};
use tokio::sync::watch;

const CONSENT: &str = "reports.sales-consent-v1";
const KEY: &str = "reports.key-v1";
/// The id of the last trade already handled (sent, refused or not reportable).
const MARK: &str = "reports.sales-mark-v1";
const SENT: &str = "reports.sent-v1";
const BASE: &str = "https://tennoworth.app/api/reports";

const CONTRACT_VERSION: u32 = 1;
const MAX_SALES_PER_BATCH: usize = 50;
const MAX_QTY: i64 = 999;
const MAX_PLAT: i64 = 100_000;
/// The service refuses sales older than this; reading further back is waste.
const MAX_SALE_AGE_DAYS: u64 = 7;
/// Trades read per pass. A long offline stretch drains over several passes.
const TRADES_PER_PASS: i64 = 500;
const PASS_INTERVAL: Duration = Duration::from_secs(60);
const MAX_BACKOFF: Duration = Duration::from_secs(30 * 60);

#[derive(Serialize, Debug, PartialEq)]
pub struct PriceReportStatus {
    pub enabled: bool,
    pub available: bool,
    /// Sales accepted by the service during the current ISO week.
    pub sent_this_week: u32,
}

pub struct PriceReports {
    consent: watch::Sender<bool>,
    mutation: std::sync::Mutex<()>,
    available: bool,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct SentCount {
    week: String,
    count: u32,
}

pub(crate) fn iso_week(day: NaiveDate) -> String {
    let w = day.iso_week();
    format!("{}-W{:02}", w.year(), w.week())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The install's identity for one purpose. Each label yields an unrelated value,
/// and none of them reveals the key.
fn derive(key: &[u8; 32], label: &str) -> String {
    #[allow(
        clippy::expect_used,
        reason = "HMAC-SHA256 accepts a key of any length"
    )]
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("any key length");
    mac.update(label.as_bytes());
    hex(&mac.finalize().into_bytes())
}

struct WeekIds {
    contributor: String,
    erase_secret: String,
    erase: String,
}

fn week_ids(key: &[u8; 32], week: &str) -> WeekIds {
    let erase_secret = derive(key, &format!("del|{week}"));
    WeekIds {
        contributor: derive(key, &format!("id|{week}")),
        erase: hex(&Sha256::digest(erase_secret.as_bytes())),
        erase_secret,
    }
}

fn sale_ref(key: &[u8; 32], trade_id: i64) -> String {
    derive(key, &format!("sale|{trade_id}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Sale {
    trade_id: i64,
    day: NaiveDate,
    side: &'static str,
    slug: String,
    tiered: bool,
    qty: i64,
    plat: i64,
}

/// A trade becomes a report only when it is plat for one kind of item: any
/// other shape has no unit price. The partner, the log stamp and the time of
/// day are not carried over.
fn sale_of(trade: &TradeRow, resolve: &dyn Fn(&str) -> Option<(String, bool)>) -> Option<Sale> {
    let (side, direction) = match trade.kind.as_str() {
        "sale" => ("sale", "given"),
        "purchase" => ("purchase", "received"),
        _ => return None,
    };
    let first = trade.items.first()?;
    let name = first.name.trim().to_lowercase();
    if trade
        .items
        .iter()
        .any(|i| i.direction != direction || i.name.trim().to_lowercase() != name)
    {
        return None;
    }
    let qty: i64 = trade.items.iter().map(|i| i.qty).sum();
    if !(1..=MAX_QTY).contains(&qty) || !(1..=MAX_PLAT).contains(&trade.plat) {
        return None;
    }
    let (slug, tiered) = resolve(&name)?;
    Some(Sale {
        trade_id: trade.id,
        day: DateTime::<Utc>::from_timestamp(trade.at, 0)?.date_naive(),
        side,
        slug,
        tiered,
        qty,
        plat: trade.plat,
    })
}

#[derive(Debug)]
struct Batch {
    week: String,
    sales: Vec<Sale>,
    /// The mark to store once this batch is settled: every trade up to here
    /// has been handled, including unreportable ones before the next batch.
    through: i64,
}

/// Group the trades after the mark into batches the service accepts: one ISO
/// week each, at most fifty sales. Returns the batches and the id of the last
/// trade read, which is the mark when there is nothing to send.
fn plan(
    trades: &[TradeRow],
    today: NaiveDate,
    resolve: &dyn Fn(&str) -> Option<(String, bool)>,
) -> (Vec<Batch>, Option<i64>) {
    let oldest = today.checked_sub_days(Days::new(MAX_SALE_AGE_DAYS));
    let mut batches: Vec<Batch> = Vec::new();
    for trade in trades {
        let Some(sale) = sale_of(trade, resolve) else {
            continue;
        };
        if oldest.is_none_or(|oldest| sale.day < oldest) {
            continue;
        }
        let week = iso_week(sale.day);
        match batches.last_mut() {
            Some(last) if last.week == week && last.sales.len() < MAX_SALES_PER_BATCH => {
                last.sales.push(sale);
            }
            _ => batches.push(Batch {
                week,
                sales: vec![sale],
                through: 0,
            }),
        }
    }
    let last_read = trades.last().map(|t| t.id);
    let starts: Vec<i64> = batches
        .iter()
        .skip(1)
        .filter_map(|b| b.sales.first().map(|s| s.trade_id - 1))
        .collect();
    for (batch, through) in batches.iter_mut().zip(
        starts
            .into_iter()
            .map(Some)
            .chain(std::iter::once(last_read)),
    ) {
        batch.through = through.unwrap_or_default();
    }
    (batches, last_read)
}

fn body(key: &[u8; 32], batch: &Batch) -> serde_json::Value {
    let ids = week_ids(key, &batch.week);
    serde_json::json!({
        "v": CONTRACT_VERSION,
        "contributor": ids.contributor,
        "erase": ids.erase,
        "sales": batch.sales.iter().map(|s| serde_json::json!({
            "ref": sale_ref(key, s.trade_id),
            "day": s.day.to_string(),
            "side": s.side,
            "slug": s.slug,
            "tiered": s.tiered,
            "qty": s.qty,
            "plat": s.plat,
        })).collect::<Vec<_>>(),
    })
}

fn opted_in(db: &Db) -> bool {
    db.get_setting(CONSENT).ok().flatten().as_deref() == Some("true")
}

/// The install key, created on first opt-in. A key that cannot be read back
/// whole fails closed rather than minting a new identity mid-week.
fn load_key(db: &Db, create: bool) -> Result<Option<[u8; 32]>, ()> {
    if let Some(raw) = db.get_setting(KEY).map_err(|_| ())? {
        let mut key = [0u8; 32];
        if raw.len() != 64 {
            return Err(());
        }
        for (i, byte) in key.iter_mut().enumerate() {
            *byte = raw
                .get(i * 2..i * 2 + 2)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .ok_or(())?;
        }
        return Ok(Some(key));
    }
    if !create {
        return Ok(None);
    }
    let mut key = [0u8; 32];
    OsRng.try_fill_bytes(&mut key).map_err(|_| ())?;
    db.set_setting(KEY, &hex(&key)).map_err(|_| ())?;
    Ok(Some(key))
}

fn mark(db: &Db) -> Result<i64, ()> {
    match db.get_setting(MARK).map_err(|_| ())? {
        Some(raw) => raw.parse().map_err(|_| ()),
        None => Err(()),
    }
}

fn sent_this_week(db: &Db, week: &str) -> u32 {
    db.get_setting(SENT)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str::<SentCount>(&raw).ok())
        .filter(|s| s.week == week)
        .map_or(0, |s| s.count)
}

fn add_sent(db: &Db, week: &str, count: usize) {
    let total = sent_this_week(db, week).saturating_add(u32::try_from(count).unwrap_or(u32::MAX));
    if let Ok(json) = serde_json::to_string(&SentCount {
        week: week.into(),
        count: total,
    }) {
        let _ = db.set_setting(SENT, &json);
    }
}

fn status(db: &Db, reports: &PriceReports) -> PriceReportStatus {
    PriceReportStatus {
        enabled: *reports.consent.borrow(),
        available: reports.available,
        sent_this_week: sent_this_week(db, &iso_week(Utc::now().date_naive())),
    }
}

#[tauri::command]
pub fn get_price_report_preferences(
    db: State<'_, Db>,
    reports: State<'_, PriceReports>,
) -> PriceReportStatus {
    status(&db, &reports)
}

#[tauri::command]
pub fn set_price_report_preferences(
    db: State<'_, Db>,
    reports: State<'_, PriceReports>,
    enabled: bool,
) -> Result<PriceReportStatus, String> {
    update_preferences(&db, &reports, enabled)?;
    Ok(status(&db, &reports))
}

fn update_preferences(db: &Db, reports: &PriceReports, enabled: bool) -> Result<(), String> {
    let _mutation = reports
        .mutation
        .lock()
        .map_err(|_| "Price sharing preferences are unavailable.".to_string())?;
    if enabled && !reports.available {
        return Err("Price sharing is disabled in this build or launch.".into());
    }
    if !enabled {
        reports.consent.send_replace(false);
        return db.set_setting(CONSENT, "false").map_err(|_| {
            "Could not save this preference. Sharing is stopped for this session; retry before restarting.".to_string()
        });
    }
    let failed = || "Could not save this preference. Nothing has been shared.".to_string();
    load_key(db, true).map_err(|_| failed())?;
    // Consent covers trades from now on, never the ledger already recorded.
    let newest = db.last_trade_id().map_err(|_| failed())?;
    db.set_setting(MARK, &newest.to_string())
        .map_err(|_| failed())?;
    db.set_setting(CONSENT, "true").map_err(|_| failed())?;
    reports.consent.send_replace(true);
    Ok(())
}

/// Ask the service to delete this install's reports for the two weeks it still
/// holds raw. Older weeks were already folded into anonymous aggregates.
#[tauri::command]
pub async fn erase_price_reports(
    db: State<'_, Db>,
    reports: State<'_, PriceReports>,
) -> Result<(), String> {
    if !reports.available {
        return Err("Price sharing is disabled in this build or launch.".into());
    }
    let unavailable = || "Could not reach the price-report service. Try again later.".to_string();
    let Some(key) = load_key(&db, false).map_err(|_| unavailable())? else {
        return Ok(());
    };
    let client = usage::client().map_err(|_| unavailable())?;
    let today = Utc::now().date_naive();
    for week in open_weeks(today) {
        let request = serde_json::json!({
            "v": CONTRACT_VERSION,
            "week": week,
            "secret": week_ids(&key, &week).erase_secret,
        });
        let accepted = post(&client, &format!("{BASE}/erase"), &request).await;
        if accepted != Ok(reqwest::StatusCode::NO_CONTENT) {
            return Err(unavailable());
        }
    }
    let _ = db.set_setting(SENT, "");
    Ok(())
}

/// The weeks the service still keeps raw: this one and the previous one.
fn open_weeks(today: NaiveDate) -> Vec<String> {
    let mut weeks = Vec::new();
    if let Some(previous) = today.checked_sub_days(Days::new(7)) {
        weeks.push(iso_week(previous));
    }
    weeks.push(iso_week(today));
    weeks
}

/// The one place this module sends: a JSON body with the usage client's
/// settings (no cookies, no redirects, no WFM headers).
async fn post(
    client: &reqwest::Client,
    url: &str,
    body: &serde_json::Value,
) -> Result<reqwest::StatusCode, ()> {
    client
        .post(url)
        .json(body)
        .send()
        .await
        .map(|r| r.status())
        .map_err(|_| ())
}

enum Delivery {
    /// Accepted, or refused for good (malformed, over the weekly cap): either
    /// way the batch is settled and must not be offered again.
    Settled { accepted: bool },
    /// Network failure, timeout, server trouble or withdrawn consent: retry later.
    Retry,
}

async fn deliver(
    client: &reqwest::Client,
    endpoint: &str,
    body: &serde_json::Value,
    consent: &mut watch::Receiver<bool>,
) -> Delivery {
    if !*consent.borrow_and_update() {
        return Delivery::Retry;
    }
    let response = tokio::select! {
        biased;
        _ = consent.changed() => return Delivery::Retry,
        response = post(client, endpoint, body) => response,
    };
    match response {
        Ok(reqwest::StatusCode::NO_CONTENT) => Delivery::Settled { accepted: true },
        Ok(status)
            if status.is_client_error()
                && status != reqwest::StatusCode::REQUEST_TIMEOUT
                && status != reqwest::StatusCode::TOO_MANY_REQUESTS =>
        {
            Delivery::Settled { accepted: false }
        }
        _ => Delivery::Retry,
    }
}

/// One pass over the ledger. Returns false when a batch must be retried.
async fn pass(
    db: &Db,
    market: &(dyn Fn() -> MarketData + Sync),
    client: &reqwest::Client,
    endpoint: &str,
    consent: &mut watch::Receiver<bool>,
) -> bool {
    let (Ok(Some(key)), Ok(from)) = (load_key(db, false), mark(db)) else {
        return true;
    };
    let Ok(trades) = db.trades_after(from, TRADES_PER_PASS) else {
        return false;
    };
    if trades.is_empty() {
        return true;
    }
    let today = Utc::now().date_naive();
    let (batches, last_read) = {
        let market = market();
        plan(&trades, today, &|name: &str| market.report_item(name))
    };
    for batch in &batches {
        match deliver(client, endpoint, &body(&key, batch), consent).await {
            Delivery::Settled { accepted } => {
                if accepted {
                    add_sent(db, &iso_week(today), batch.sales.len());
                }
                if db.set_setting(MARK, &batch.through.to_string()).is_err() {
                    return false;
                }
            }
            Delivery::Retry => return false,
        }
    }
    if batches.is_empty() {
        if let Some(last) = last_read {
            return db.set_setting(MARK, &last.to_string()).is_ok();
        }
    }
    true
}

pub fn start(app: tauri::AppHandle) {
    let available = usage::allowed();
    let enabled = available && opted_in(&app.state::<Db>());
    let (consent, mut receiver) = watch::channel(enabled);
    app.manage(PriceReports {
        consent,
        available,
        mutation: std::sync::Mutex::new(()),
    });
    if !available {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let Ok(client) = usage::client() else { return };
        let endpoint = format!("{BASE}/sales");
        let mut wait = PASS_INTERVAL;
        loop {
            // Setup registers the market cache after this task starts.
            let cache = app.try_state::<crate::services::market::MarketCache>();
            if let (Some(cache), true) = (
                cache,
                *receiver.borrow_and_update() && opted_in(&app.state::<Db>()),
            ) {
                let market = || MarketData::load(&cache);
                let delivered = pass(
                    &app.state::<Db>(),
                    &market,
                    &client,
                    &endpoint,
                    &mut receiver,
                )
                .await;
                wait = if delivered {
                    PASS_INTERVAL
                } else {
                    (wait * 2).min(MAX_BACKOFF)
                };
            }
            tokio::select! {
                _ = receiver.changed() => {},
                _ = tokio::time::sleep(wait) => {},
            }
        }
    });
}

#[cfg(test)]
mod tests;
