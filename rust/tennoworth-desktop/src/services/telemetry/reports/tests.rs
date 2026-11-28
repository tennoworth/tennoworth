use super::*;
use crate::contracts::trading::TradeItem;
use crate::services::game_log::eelog::{LogPosition, TradeEvent};
use serde_json::{json, Value};
use std::future::IntoFuture;

fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../../../../tests/fixtures/reports.json"
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

fn resolve(name: &str) -> Option<Resolved> {
    match name {
        "primed flow" => Some(Resolved::Item("primed_flow".into(), true)),
        "ash prime set" => Some(Resolved::Item("ash_prime_set".into(), false)),
        "acceltra critacan" => Some(Resolved::Riven("acceltra".into(), SaleVerdict::Sellable)),
        _ => None,
    }
}

fn today() -> Date {
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
        let day: Date = v["day"].as_str().unwrap().parse().unwrap();
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
    let parsed: tennoworth_collectors::reports::contract::SalesBatch =
        serde_json::from_value(body.clone()).unwrap();
    tennoworth_collectors::reports::contract::validate_sales(parsed, today()).unwrap();
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
        delivery: tokio::sync::Mutex::new(()),
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
    let before = record(&db, 1, Timestamp::now().as_second(), "Primed Flow", 40);
    let state = reports(true);
    update_preferences(&db, &state, true).unwrap();
    assert!(opted_in(&db));
    assert_eq!(mark(&db).unwrap(), before);
    assert!(load_key(&db, false).unwrap().is_some());
    let after = record(&db, 2, Timestamp::now().as_second(), "Primed Flow", 41);
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

fn market() -> std::sync::Arc<MarketData> {
    std::sync::Arc::new(
        serde_json::from_value(json!({
            "catalog": {"primed flow": "primed_flow", "acceltra critacan": "acceltra_riven"},
            "items": {
                "primed_flow": {"tags": ["mod", "legendary"]},
                "acceltra_riven": {"tags": ["mod", "riven_mod"]},
            },
        }))
        .unwrap(),
    )
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
    let now = Timestamp::now().as_second();
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
        &state,
    )
    .await;
    let newest = db.last_trade_id().unwrap();
    assert!(newest > opted);
    (db, opted, newest)
}

#[tokio::test]
async fn an_accepted_batch_advances_the_mark_and_the_weekly_count() {
    let store =
        tennoworth_collectors::reports::Store::new(rusqlite::Connection::open_in_memory().unwrap())
            .unwrap();
    let service = tennoworth_collectors::reports::Collector::new(store);
    service.tick().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/api/reports", listener.local_addr().unwrap());
    let task = tokio::spawn(
        axum::serve(listener, tennoworth_collectors::reports::router(service)).into_future(),
    );

    let (db, _, newest) = run_pass(&endpoint, true).await;
    assert_eq!(mark(&db).unwrap(), newest);
    assert_eq!(
        sent_this_week(&db, &iso_week(utc_date(Timestamp::now()))),
        3
    );
    task.abort();
}

/// A server that answers every request with `status`.
async fn answering(status: u16) -> (String, tokio::task::JoinHandle<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/api/reports", listener.local_addr().unwrap());
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
    assert_eq!(
        sent_this_week(&db, &iso_week(utc_date(Timestamp::now()))),
        0
    );
    task.abort();
}

#[tokio::test]
async fn nothing_is_sent_without_consent() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}/api/reports", listener.local_addr().unwrap());
    let (db, opted, _) = run_pass(&endpoint, false).await;
    assert_eq!(mark(&db).unwrap(), opted);
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[tokio::test]
async fn deleting_reports_skips_queued_trades_even_if_the_service_is_unavailable() {
    for status in [204, 503] {
        let db = Db::open_in_memory().unwrap();
        let state = reports(true);
        update_preferences(&db, &state, true).unwrap();
        let newest = record(&db, 10, Timestamp::now().as_second(), "Primed Flow", 40);
        let (endpoint, task) = answering(status).await;
        let client = usage::client().unwrap();
        let result = erase_recent(&db, &state, &client, &endpoint).await;
        assert_eq!(result.is_ok(), status == 204);
        assert_eq!(mark(&db).unwrap(), newest);
        assert!(
            opted_in(&db),
            "deletion preserves consent for future trades"
        );
        let mut receiver = state.consent.subscribe();
        assert!(pass(&db, &market, &client, &endpoint, &mut receiver, &state).await);
        assert_eq!(
            sent_this_week(&db, &iso_week(utc_date(Timestamp::now()))),
            0
        );
        if status == 204 {
            let future = record(&db, 11, Timestamp::now().as_second(), "Primed Flow", 41);
            assert!(pass(&db, &market, &client, &endpoint, &mut receiver, &state).await);
            assert_eq!(mark(&db).unwrap(), future);
            assert_eq!(
                sent_this_week(&db, &iso_week(utc_date(Timestamp::now()))),
                1
            );
        }
        task.abort();
    }
}

#[tokio::test]
async fn deletion_waits_for_an_active_upload_then_erases_its_results() {
    use axum::{routing::post, Router};
    use std::sync::Arc;
    let db = Db::open_in_memory().unwrap();
    let state = reports(true);
    update_preferences(&db, &state, true).unwrap();
    record(&db, 10, Timestamp::now().as_second(), "Primed Flow", 40);
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = Router::new()
        .route(
            "/sales",
            post({
                let (entered, release, events) = (entered.clone(), release.clone(), events.clone());
                move || {
                    let (entered, release, events) =
                        (entered.clone(), release.clone(), events.clone());
                    async move {
                        entered.notify_one();
                        release.notified().await;
                        events.lock().unwrap().push("sale");
                        axum::http::StatusCode::NO_CONTENT
                    }
                }
            }),
        )
        .route(
            "/erase",
            post({
                let events = events.clone();
                move || {
                    let events = events.clone();
                    async move {
                        events.lock().unwrap().push("erase");
                        axum::http::StatusCode::NO_CONTENT
                    }
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let endpoint = base.clone();
    let server = tokio::spawn(axum::serve(listener, app).into_future());
    let client = usage::client().unwrap();
    let mut receiver = state.consent.subscribe();
    let upload = pass(&db, &market, &client, &endpoint, &mut receiver, &state);
    let delete = async {
        entered.notified().await;
        let deletion = erase_recent(&db, &state, &client, &base);
        tokio::pin!(deletion);
        assert!(
            tokio::time::timeout(Duration::from_secs(1), &mut deletion)
                .await
                .is_err(),
            "deletion must wait while the upload is blocked"
        );
        assert!(events.lock().unwrap().is_empty());
        release.notify_one();
        deletion.await.unwrap();
    };
    let (sent, ()) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(upload, delete)
    })
    .await
    .unwrap();
    assert!(sent);
    assert_eq!(*events.lock().unwrap(), ["sale", "erase", "erase"]);
    assert_eq!(
        sent_this_week(&db, &iso_week(utc_date(Timestamp::now()))),
        0
    );
    server.abort();
}

#[test]
fn a_sold_riven_is_reported_by_weapon_and_verdict_in_its_own_batch() {
    let trades = vec![
        sale(1, THURSDAY, "Primed Flow", 40),
        sale(2, THURSDAY, "Acceltra Critacan", 150),
        sale(3, THURSDAY, "Primed Flow", 41),
    ];
    let (batches, _) = plan(&trades, today(), &resolve);
    let shape: Vec<(bool, Vec<i64>, i64)> = batches
        .iter()
        .map(|b| {
            (
                b.rivens,
                b.sales.iter().map(|s| s.trade_id).collect(),
                b.through,
            )
        })
        .collect();
    assert_eq!(
        shape,
        [(false, vec![1], 1), (true, vec![2], 2), (false, vec![3], 3)],
        "kinds stay in trade order so the mark still covers each batch"
    );
    let body = body(&key(), &batches[1]);
    let parsed: tennoworth_collectors::reports::contract::RivenSalesBatch =
        serde_json::from_value(body.clone()).unwrap();
    tennoworth_collectors::reports::contract::validate_riven_sales(parsed, today()).unwrap();
    assert_eq!(body["sales"][0]["weapon"], "acceltra");
    assert_eq!(body["sales"][0]["verdict"], "sellable");
    let text = body.to_string();
    for leaked in ["SomeTenno", "1234.5", "Critacan", "critacan"] {
        assert!(
            !text.contains(leaked),
            "{leaked} must not leave the machine"
        );
    }
}

#[test]
fn a_bought_riven_or_one_traded_with_others_is_not_reported() {
    let bought = trade(
        1,
        THURSDAY,
        "purchase",
        150,
        vec![item("Acceltra Critacan", 1, "received")],
    );
    assert_eq!(
        sale_of(&bought, &resolve),
        None,
        "a bought riven was never scanned"
    );
    let two = trade(
        2,
        THURSDAY,
        "sale",
        300,
        vec![item("Acceltra Critacan", 2, "given")],
    );
    assert_eq!(sale_of(&two, &resolve), None);
    let mixed = trade(
        3,
        THURSDAY,
        "sale",
        300,
        vec![
            item("Acceltra Critacan", 1, "given"),
            item("Primed Flow", 1, "given"),
        ],
    );
    assert_eq!(sale_of(&mixed, &resolve), None);
}

fn riven_market() -> std::sync::Arc<MarketData> {
    std::sync::Arc::new(
        serde_json::from_value(json!({
            "catalog": {"primed flow": "primed_flow"},
            "items": {"primed_flow": {"tags": ["mod", "legendary"]}},
            "rivens": {
                "weapons": {"dex_nikana": {"name": "Dex Nikana", "game_ref": "/Lotus/Weapons/Tenno/Melee/Swords/DexNikana"}},
                "attributes": [{"game_ref": "WeaponCritDamageMod", "slug": "critical_damage", "name": "Critical Damage"}],
            },
        }))
        .unwrap(),
    )
}

fn inventory_with_a_dex_riven() -> Vec<u8> {
    let fingerprint = json!({
        "compat": "/Lotus/Weapons/Tenno/Melee/Swords/DexNikana",
        "buffs": [{"Tag": "WeaponCritDamageMod", "Value": 1}],
        "curses": [],
    });
    json!({"Upgrades": [{
        "ItemType": "/Lotus/Upgrades/Mods/Randomized/PlayerMeleeWeaponRandomModRare",
        "UpgradeFingerprint": fingerprint.to_string(),
    }]})
    .to_string()
    .into_bytes()
}

#[test]
fn scans_are_remembered_by_weapon_and_verdict_only_while_sharing_is_on() {
    let db = Db::open_in_memory().unwrap();
    let state = reports(true);
    let market = riven_market();
    remember_scan(&db, &market, &inventory_with_a_dex_riven());
    assert!(
        held_rivens(&db).is_empty(),
        "nothing is kept before consent"
    );

    update_preferences(&db, &state, true).unwrap();
    remember_scan(&db, &market, &inventory_with_a_dex_riven());
    let held = held_rivens(&db);
    assert_eq!(held.len(), 1);
    assert_eq!(
        (held[0].weapon.as_str(), held[0].verdict),
        ("dex_nikana", SaleVerdict::Unrated)
    );
    let stored = db.get_setting(HELD).unwrap().unwrap();
    assert!(
        !stored.contains("WeaponCritDamageMod") && !stored.contains("critical_damage"),
        "no stats are kept"
    );

    remember_scan(&db, &market, br#"{"Upgrades": []}"#);
    assert_eq!(held_rivens(&db).len(), 1, "the previous scan still counts");
    remember_scan(&db, &market, br#"{"Upgrades": []}"#);
    assert!(held_rivens(&db).is_empty(), "two scans later it is gone");

    remember_scan(&db, &market, &inventory_with_a_dex_riven());
    update_preferences(&db, &state, false).unwrap();
    assert!(held_rivens(&db).is_empty(), "opting out forgets the rivens");
}

/// A server that records each request's path and body and answers `status`.
async fn recording(
    status: u16,
) -> (
    String,
    std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>>,
    tokio::task::JoinHandle<std::io::Result<()>>,
) {
    use axum::{extract::Request, Router};
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = Router::new().fallback({
        let seen = seen.clone();
        move |request: Request| {
            let seen = seen.clone();
            async move {
                let path = request.uri().path().to_string();
                let bytes = axum::body::to_bytes(request.into_body(), 1 << 20)
                    .await
                    .unwrap();
                let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
                seen.lock().unwrap().push((path, body));
                axum::http::StatusCode::from_u16(status).unwrap()
            }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api/reports", listener.local_addr().unwrap());
    (
        base,
        seen,
        tokio::spawn(axum::serve(listener, app).into_future()),
    )
}

#[tokio::test]
async fn a_pass_sends_riven_sales_to_their_own_endpoint() {
    let db = Db::open_in_memory().unwrap();
    let state = reports(true);
    update_preferences(&db, &state, true).unwrap();
    let market = riven_market();
    remember_scan(&db, &market, &inventory_with_a_dex_riven());
    let now = Timestamp::now().as_second();
    record(&db, 10, now, "Dex Nikana Critatox", 300);
    record(&db, 11, now, "Primed Flow", 40);
    let newest = record(&db, 12, now, "Soma Critatox", 90);
    let (base, seen, task) = recording(204).await;
    let (_sender, mut receiver) = watch::channel(true);
    let catalog = || market.clone();
    assert!(
        pass(
            &db,
            &catalog,
            &usage::client().unwrap(),
            &base,
            &mut receiver,
            &state
        )
        .await
    );
    let seen = seen.lock().unwrap().clone();
    let paths: Vec<&str> = seen.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(paths, ["/api/reports/riven-sales", "/api/reports/sales"]);
    assert_eq!(seen[0].1["sales"][0]["weapon"], "dex_nikana");
    assert_eq!(seen[0].1["sales"][0]["verdict"], "unrated");
    assert_eq!(
        mark(&db).unwrap(),
        newest,
        "the unmatched Soma riven is settled, not retried"
    );
    task.abort();
}

fn vote(weapon: &str, agree: bool) -> RollVote {
    RollVote {
        weapon: weapon.into(),
        verdict: SaleVerdict::Weak,
        agree,
        suggest: (!agree).then(|| RollSuggestion {
            stat: "zoom".into(),
            positive: false,
            good: true,
        }),
    }
}

#[tokio::test]
async fn a_rating_needs_sharing_on_and_is_kept_one_per_weapon_this_week() {
    let db = Db::open_in_memory().unwrap();
    let state = reports(true);
    let client = usage::client().unwrap();
    let (base, seen, task) = recording(204).await;
    assert!(rate(&db, &state, &client, &base, vote("acceltra", true))
        .await
        .is_err());
    assert!(
        seen.lock().unwrap().is_empty(),
        "nothing is sent before consent"
    );

    update_preferences(&db, &state, true).unwrap();
    state.consent.send_replace(true);
    rate(&db, &state, &client, &base, vote("acceltra", true))
        .await
        .unwrap();
    rate(&db, &state, &client, &base, vote("soma", true))
        .await
        .unwrap();
    let votes = rate(&db, &state, &client, &base, vote("acceltra", false))
        .await
        .unwrap();
    assert_eq!(votes.len(), 2);
    assert!(!votes.iter().find(|v| v.weapon == "acceltra").unwrap().agree);
    assert_eq!(
        week_votes(&db, &iso_week(utc_date(Timestamp::now()))),
        votes
    );

    let (path, body) = seen.lock().unwrap().last().cloned().unwrap();
    assert_eq!(path, "/api/reports/roll-vote");
    let parsed: tennoworth_collectors::reports::contract::RollVote =
        serde_json::from_value(body).unwrap();
    tennoworth_collectors::reports::contract::validate_vote(parsed).unwrap();

    erase_recent(&db, &state, &client, &base).await.unwrap();
    assert!(week_votes(&db, &iso_week(utc_date(Timestamp::now()))).is_empty());
    task.abort();
}

#[tokio::test]
async fn a_refused_or_failed_rating_is_not_kept() {
    for status in [400, 503] {
        let db = Db::open_in_memory().unwrap();
        let state = reports(true);
        update_preferences(&db, &state, true).unwrap();
        state.consent.send_replace(true);
        let (base, _, task) = recording(status).await;
        let client = usage::client().unwrap();
        assert!(rate(&db, &state, &client, &base, vote("acceltra", true))
            .await
            .is_err());
        assert!(week_votes(&db, &iso_week(utc_date(Timestamp::now()))).is_empty());
        task.abort();
    }
}

#[tokio::test]
async fn an_install_that_opted_in_before_rivens_sends_none_until_it_includes_them() {
    let db = Db::open_in_memory().unwrap();
    let state = reports(true);
    update_preferences(&db, &state, true).unwrap();
    // Consent given under the first terms, which left riven trades out.
    db.set_setting(RIVEN_CONSENT, "").unwrap();
    assert!(!status(&db, &state).rivens);
    let market = riven_market();
    remember_scan(&db, &market, &inventory_with_a_dex_riven());
    assert!(
        held_rivens(&db).is_empty(),
        "no scan is kept for riven sales"
    );

    db.set_setting(
        HELD,
        &serde_json::to_string(&HeldScans {
            current: vec![HeldRiven {
                weapon: "dex_nikana".into(),
                name: "Dex Nikana".into(),
                verdict: SaleVerdict::Unrated,
            }],
            previous: Vec::new(),
        })
        .unwrap(),
    )
    .unwrap();
    record(
        &db,
        10,
        Timestamp::now().as_second(),
        "Dex Nikana Critatox",
        300,
    );
    let (base, seen, task) = recording(204).await;
    let (_sender, mut receiver) = watch::channel(true);
    let catalog = || market.clone();
    assert!(
        pass(
            &db,
            &catalog,
            &usage::client().unwrap(),
            &base,
            &mut receiver,
            &state
        )
        .await
    );
    assert!(
        seen.lock().unwrap().is_empty(),
        "the riven sale is not sent"
    );

    include_rivens(&db, &state).unwrap();
    state.consent.send_replace(true);
    assert!(status(&db, &state).rivens);
    remember_scan(&db, &market, &inventory_with_a_dex_riven());
    record(
        &db,
        11,
        Timestamp::now().as_second(),
        "Dex Nikana Critatox",
        310,
    );
    assert!(
        pass(
            &db,
            &catalog,
            &usage::client().unwrap(),
            &base,
            &mut receiver,
            &state
        )
        .await
    );
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    assert_eq!(
        seen[0].1["sales"].as_array().unwrap().len(),
        1,
        "only the sale after including rivens"
    );
    assert_eq!(seen[0].1["sales"][0]["plat"], 310);
    task.abort();

    update_preferences(&db, &state, false).unwrap();
    assert!(
        include_rivens(&db, &state).is_err(),
        "rivens need price sharing on"
    );
}
