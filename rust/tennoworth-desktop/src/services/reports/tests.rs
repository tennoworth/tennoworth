use super::*;
use crate::services::eelog::{LogPosition, TradeEvent};
use crate::trading_contract::TradeItem;
use serde_json::{json, Value};
use std::future::IntoFuture;

fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/reports/contract.json"
    ))
    .unwrap()
}

fn key() -> [u8; 32] {
    let mut key = [0u8; 32];
    for (i, b) in key.iter_mut().enumerate() {
        *b = i as u8;
    }
    key
}

fn item(name: &str, qty: i64, direction: &str) -> TradeItem {
    TradeItem {
        name: name.into(),
        qty,
        direction: direction.into(),
    }
}

/// 2026-10-08 12:00 UTC, a Thursday in 2026-W41.
const THURSDAY: i64 = 1_791_460_800;
const DAY: i64 = 86_400;

fn trade(id: i64, at: i64, kind: &str, plat: i64, items: Vec<TradeItem>) -> TradeRow {
    TradeRow {
        id,
        at,
        partner: "SomeTenno".into(),
        kind: kind.into(),
        plat,
        items,
        log_stamp: Some("1234.5".into()),
        wfm_closed: false,
    }
}

fn sale(id: i64, at: i64, name: &str, plat: i64) -> TradeRow {
    trade(id, at, "sale", plat, vec![item(name, 1, "given")])
}

fn resolve(name: &str) -> Option<(String, bool)> {
    match name {
        "primed flow" => Some(("primed_flow".into(), true)),
        "ash prime set" => Some(("ash_prime_set".into(), false)),
        _ => None,
    }
}

fn today() -> NaiveDate {
    "2026-10-08".parse().unwrap()
}

#[test]
fn ids_match_the_shared_derivation_vectors() {
    let fx = fixture();
    let pseudonyms = &fx["pseudonyms"];
    assert_eq!(hex(&key()), pseudonyms["key"].as_str().unwrap());
    for v in pseudonyms["vectors"].as_array().unwrap() {
        let ids = week_ids(&key(), v["week"].as_str().unwrap());
        assert_eq!(ids.contributor, v["contributor"].as_str().unwrap());
        assert_eq!(ids.erase_secret, v["erase_secret"].as_str().unwrap());
        assert_eq!(ids.erase, v["erase"].as_str().unwrap());
        assert_eq!(
            sale_ref(&key(), v["trade_id"].as_i64().unwrap()),
            v["ref"].as_str().unwrap()
        );
    }
}

#[test]
fn iso_weeks_match_the_shared_vectors() {
    for v in fixture()["iso_weeks"].as_array().unwrap() {
        let day: NaiveDate = v["day"].as_str().unwrap().parse().unwrap();
        assert_eq!(iso_week(day), v["week"].as_str().unwrap());
    }
}

#[test]
fn only_plat_for_one_kind_of_item_is_a_sale() {
    let at = THURSDAY;
    let stack = trade(
        1,
        at,
        "sale",
        90,
        vec![
            item("Primed Flow", 2, "given"),
            item("primed flow", 1, "given"),
        ],
    );
    let reported = sale_of(&stack, &resolve).unwrap();
    assert_eq!(
        (reported.slug.as_str(), reported.qty, reported.plat),
        ("primed_flow", 3, 90)
    );
    assert!(reported.tiered);

    let bought = trade(
        2,
        at,
        "purchase",
        150,
        vec![item("Ash Prime Set", 1, "received")],
    );
    assert_eq!(sale_of(&bought, &resolve).unwrap().side, "purchase");

    for skipped in [
        trade(
            3,
            at,
            "trade",
            0,
            vec![
                item("Primed Flow", 1, "given"),
                item("Ash Prime Set", 1, "received"),
            ],
        ),
        trade(
            4,
            at,
            "sale",
            90,
            vec![
                item("Primed Flow", 1, "given"),
                item("Ash Prime Set", 1, "given"),
            ],
        ),
        trade(5, at, "sale", 90, vec![item("Primed Flow", 1, "received")]),
        trade(6, at, "sale", 90, vec![item("Unknown Thing", 1, "given")]),
        trade(7, at, "sale", 0, vec![item("Primed Flow", 1, "given")]),
        trade(
            8,
            at,
            "sale",
            100_001,
            vec![item("Primed Flow", 1, "given")],
        ),
        trade(9, at, "sale", 90, vec![]),
    ] {
        assert_eq!(sale_of(&skipped, &resolve), None, "trade {}", skipped.id);
    }
}

#[test]
fn batches_follow_weeks_and_the_mark_covers_skipped_trades() {
    let trades = vec![
        sale(10, THURSDAY - 9 * DAY, "Primed Flow", 40), // too old
        sale(11, THURSDAY - 4 * DAY, "Primed Flow", 40), // W40
        sale(12, THURSDAY - 4 * DAY, "Unknown Thing", 5),
        sale(13, THURSDAY, "Primed Flow", 41), // W41
        sale(14, THURSDAY, "Unknown Thing", 5),
    ];
    let (batches, last) = plan(&trades, today(), &resolve);
    assert_eq!(last, Some(14));
    let summary: Vec<(&str, Vec<i64>, i64)> = batches
        .iter()
        .map(|b| {
            (
                b.week.as_str(),
                b.sales.iter().map(|s| s.trade_id).collect(),
                b.through,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [("2026-W40", vec![11], 12), ("2026-W41", vec![13], 14)]
    );
}

#[test]
fn a_full_batch_starts_another() {
    let trades: Vec<TradeRow> = (1..=51)
        .map(|id| sale(id, THURSDAY, "Primed Flow", 40))
        .collect();
    let (batches, _) = plan(&trades, today(), &resolve);
    assert_eq!(
        batches.iter().map(|b| b.sales.len()).collect::<Vec<_>>(),
        [50, 1]
    );
    assert_eq!(batches[0].through, 50);
}

#[test]
fn the_service_accepts_what_the_app_builds_and_it_names_no_partner() {
    let trades = vec![
        sale(1, THURSDAY, "Primed Flow", 40),
        trade(
            2,
            THURSDAY,
            "purchase",
            150,
            vec![item("Ash Prime Set", 1, "received")],
        ),
    ];
    let (batches, _) = plan(&trades, today(), &resolve);
    let body = body(&key(), &batches[0]);
    let parsed: tennoworth_reports::contract::SalesBatch =
        serde_json::from_value(body.clone()).unwrap();
    tennoworth_reports::contract::validate_sales(parsed, today()).unwrap();
    let text = body.to_string();
    for leaked in ["SomeTenno", "1234.5", "Primed Flow"] {
        assert!(
            !text.contains(leaked),
            "{leaked} must not leave the machine"
        );
    }
}

fn reports(available: bool) -> PriceReports {
    let (consent, _) = watch::channel(false);
    PriceReports {
        consent,
        available,
        mutation: std::sync::Mutex::new(()),
    }
}

fn record(db: &Db, id_hint: u64, at: i64, name: &str, plat: i64) -> i64 {
    let event = TradeEvent {
        partner: "SomeTenno".into(),
        kind: "sale".into(),
        plat,
        items: vec![item(name, 1, "given")],
        log_stamp: Some(format!("{id_hint}.0")),
    };
    let position = LogPosition {
        session: "s".into(),
        start: id_hint * 10,
        end: id_hint * 10 + 5,
        observed_after: at,
    };
    db.insert_trade(&event, at, &position).unwrap().unwrap()
}

#[test]
fn opting_in_covers_trades_from_now_on_only() {
    let db = Db::open_in_memory().unwrap();
    let before = record(&db, 1, Utc::now().timestamp(), "Primed Flow", 40);
    let state = reports(true);
    update_preferences(&db, &state, true).unwrap();
    assert!(opted_in(&db));
    assert_eq!(mark(&db).unwrap(), before);
    assert!(load_key(&db, false).unwrap().is_some());
    let after = record(&db, 2, Utc::now().timestamp(), "Primed Flow", 41);
    let pending = db
        .trades_after(mark(&db).unwrap(), TRADES_PER_PASS)
        .unwrap();
    assert_eq!(pending.iter().map(|t| t.id).collect::<Vec<_>>(), [after]);
}

#[test]
fn consent_is_refused_where_sharing_is_unavailable() {
    let db = Db::open_in_memory().unwrap();
    assert!(update_preferences(&db, &reports(false), true).is_err());
    assert!(!opted_in(&db));
    assert!(!usage::allowed(), "tests never report to production");
}

#[test]
fn a_damaged_key_fails_closed() {
    let db = Db::open_in_memory().unwrap();
    for broken in ["", "zz", &"g".repeat(64)] {
        db.set_setting(KEY, broken).unwrap();
        assert!(load_key(&db, true).is_err(), "{broken:?}");
    }
}

fn market() -> MarketData {
    serde_json::from_value(json!({
        "catalog": {"primed flow": "primed_flow", "acceltra critacan": "acceltra_riven"},
        "items": {
            "primed_flow": {"tags": ["mod", "legendary"]},
            "acceltra_riven": {"tags": ["mod", "riven_mod"]},
        },
    }))
    .unwrap()
}

#[test]
fn rivens_and_unpriced_names_are_not_reported() {
    let market = market();
    assert_eq!(
        market.report_item("Primed Flow"),
        Some(("primed_flow".into(), true))
    );
    assert_eq!(market.report_item("Acceltra Critacan"), None);
    assert_eq!(market.report_item("Nothing Here"), None);
}

/// Opt in, record trades and run a pass against `endpoint`.
async fn run_pass(endpoint: &str, consent_on: bool) -> (Db, i64, i64) {
    let db = Db::open_in_memory().unwrap();
    let state = reports(true);
    update_preferences(&db, &state, true).unwrap();
    let opted = mark(&db).unwrap();
    let now = Utc::now().timestamp();
    for i in 0..3 {
        record(&db, 10 + i, now, "Primed Flow", 40 + i as i64);
    }
    let (_sender, mut receiver) = watch::channel(consent_on);
    let catalog = || market();
    pass(
        &db,
        &catalog,
        &usage::client().unwrap(),
        endpoint,
        &mut receiver,
    )
    .await;
    let newest = db.last_trade_id().unwrap();
    assert!(newest > opted);
    (db, opted, newest)
}

#[tokio::test]
async fn an_accepted_batch_advances_the_mark_and_the_weekly_count() {
    let store =
        tennoworth_reports::Store::new(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let service = tennoworth_reports::Collector::new(store);
    service.tick().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "http://{}/api/reports/sales",
        listener.local_addr().unwrap()
    );
    let task =
        tokio::spawn(axum::serve(listener, tennoworth_reports::router(service)).into_future());

    let (db, _, newest) = run_pass(&endpoint, true).await;
    assert_eq!(mark(&db).unwrap(), newest);
    assert_eq!(sent_this_week(&db, &iso_week(Utc::now().date_naive())), 3);
    task.abort();
}

/// A server that answers every request with `status`.
async fn answering(status: u16) -> (String, tokio::task::JoinHandle<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "http://{}/api/reports/sales",
        listener.local_addr().unwrap()
    );
    let task = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut buffer = vec![0u8; 65536];
            let _ = socket.read(&mut buffer).await;
            let reply =
                format!("HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            let _ = socket.write_all(reply.as_bytes()).await;
        }
    });
    (endpoint, task)
}

#[tokio::test]
async fn a_server_failure_keeps_the_trades_for_the_next_pass() {
    let (endpoint, task) = answering(503).await;
    let (db, opted, _) = run_pass(&endpoint, true).await;
    assert_eq!(mark(&db).unwrap(), opted);
    task.abort();
}

#[tokio::test]
async fn a_refused_batch_is_settled_and_not_counted() {
    let (endpoint, task) = answering(400).await;
    let (db, _, newest) = run_pass(&endpoint, true).await;
    assert_eq!(
        mark(&db).unwrap(),
        newest,
        "a refused batch must not be resent forever"
    );
    assert_eq!(sent_this_week(&db, &iso_week(Utc::now().date_naive())), 0);
    task.abort();
}

#[tokio::test]
async fn nothing_is_sent_without_consent() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!(
        "http://{}/api/reports/sales",
        listener.local_addr().unwrap()
    );
    let (db, opted, _) = run_pass(&endpoint, false).await;
    assert_eq!(mark(&db).unwrap(), opted);
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}
