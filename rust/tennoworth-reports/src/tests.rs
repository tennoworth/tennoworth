use super::*;
use crate::contract::{validate_erase, validate_sales, validate_shape};
use crate::store::{aggregate, RawSale, MAX_SALES_PER_CONTRIBUTOR_WEEK, MIN_CONTRIBUTORS};
use chrono::{DateTime, NaiveDate};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::future::IntoFuture;

fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/reports/contract.json"
    ))
    .unwrap()
}

fn fixture_today() -> NaiveDate {
    fixture()["today"].as_str().unwrap().parse().unwrap()
}

fn at(stamp: &str) -> DateTime<Utc> {
    stamp.parse().unwrap()
}

fn store_at(now: DateTime<Utc>) -> Store {
    let mut store = Store::new(Connection::open_in_memory().unwrap()).unwrap();
    store.tick(now).unwrap();
    store
}

fn hex(n: u64) -> String {
    format!("{n:064x}")
}

/// One contributor's batch of single-unit sales of `slug`, all on `day`.
fn batch(contributor: u64, slug: &str, day: &str, prices: &[u32]) -> Value {
    json!({
        "v": 1,
        "contributor": hex(contributor),
        "erase": hex(contributor + 1_000_000),
        "sales": prices.iter().enumerate().map(|(i, plat)| json!({
            "ref": hex(i as u64), "day": day, "side": "sale", "slug": slug,
            "tiered": false, "qty": 1, "plat": plat,
        })).collect::<Vec<_>>(),
    })
}

fn add(store: &mut Store, body: Value, today: &str) -> Result<(), StatusCode> {
    let batch = validate_sales(
        serde_json::from_value(body).unwrap(),
        today.parse().unwrap(),
    )
    .map_err(|_| StatusCode::BAD_REQUEST)?;
    store.add_sales(batch)
}

fn raw(contributor: &str, plat: u32, qty: u32) -> RawSale {
    RawSale {
        side: "sale".into(),
        slug: "primed_flow".into(),
        tiered: false,
        contributor: contributor.into(),
        qty,
        plat,
    }
}

#[test]
fn sales_cases_match_the_shared_fixture() {
    let today = fixture_today();
    for case in fixture()["sales"].as_array().unwrap() {
        let parsed = serde_json::from_value(case["body"].clone())
            .ok()
            .and_then(|b| validate_sales(b, today).ok());
        assert_eq!(
            parsed.is_some(),
            case["valid"].as_bool().unwrap(),
            "{}",
            case["case"]
        );
    }
}

#[test]
fn shape_cases_match_the_shared_fixture() {
    for case in fixture()["shapes"].as_array().unwrap() {
        let parsed = serde_json::from_value(case["body"].clone())
            .ok()
            .and_then(|b| validate_shape(b).ok());
        assert_eq!(
            parsed.is_some(),
            case["valid"].as_bool().unwrap(),
            "{}",
            case["case"]
        );
    }
}

#[test]
fn erase_cases_and_vectors_match_the_shared_fixture() {
    let fx = fixture();
    for case in fx["erase"].as_array().unwrap() {
        let parsed = serde_json::from_value(case["body"].clone())
            .ok()
            .and_then(|b| validate_erase(b).ok());
        assert_eq!(
            parsed.is_some(),
            case["valid"].as_bool().unwrap(),
            "{}",
            case["case"]
        );
    }
    for vector in fx["erase_vectors"].as_array().unwrap() {
        assert_eq!(
            contract::erase_hash(vector["secret"].as_str().unwrap()),
            vector["erase"].as_str().unwrap()
        );
    }
}

#[test]
fn equal_shapes_reported_in_any_order_store_the_same_text() {
    let fx = fixture();
    let shape =
        validate_shape(serde_json::from_value(fx["canonical_shape"]["body"].clone()).unwrap())
            .unwrap();
    assert_eq!(
        shape.shape,
        fx["canonical_shape"]["shape"].as_str().unwrap()
    );
}

#[test]
fn one_contributor_gets_one_vote_however_much_they_send() {
    let mut rows: Vec<RawSale> = (0..100).map(|_| raw("loud", 1000, 1)).collect();
    for c in ["a", "b", "c", "d"] {
        rows.push(raw(c, 10, 1));
    }
    let published = aggregate(&rows, MIN_CONTRIBUTORS);
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].contributors, 5);
    assert_eq!(published[0].median, 10.0);
    assert_eq!(published[0].units, 104);
}

#[test]
fn an_item_below_the_contributor_floor_is_not_published() {
    let rows: Vec<RawSale> = ["a", "b", "c", "d"].iter().map(|c| raw(c, 10, 1)).collect();
    assert!(aggregate(&rows, MIN_CONTRIBUTORS).is_empty());
}

#[test]
fn the_band_is_the_spread_of_contributor_medians_in_unit_prices() {
    // A stack sale of 3 for 90 is 30 each.
    let rows = vec![
        raw("a", 10, 1),
        raw("b", 20, 1),
        raw("c", 90, 3),
        raw("d", 40, 1),
        raw("e", 50, 1),
        raw("e", 70, 1),
    ];
    let row = &aggregate(&rows, MIN_CONTRIBUTORS)[0];
    assert_eq!((row.p25, row.median, row.p75), (20.0, 30.0, 40.0));
}

#[test]
fn a_resent_batch_does_not_count_twice() {
    let mut store = store_at(at("2026-10-08T12:00:00Z"));
    let body = batch(1, "primed_flow", "2026-10-08", &[40, 41]);
    add(&mut store, body.clone(), "2026-10-08").unwrap();
    add(&mut store, body, "2026-10-08").unwrap();
    let rows: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM sale", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 2);
}

#[test]
fn a_contributor_past_the_weekly_cap_is_refused() {
    let mut store = store_at(at("2026-10-08T12:00:00Z"));
    let per_batch = 50;
    let full = MAX_SALES_PER_CONTRIBUTOR_WEEK / per_batch;
    for b in 0..full {
        let mut body = batch(1, "primed_flow", "2026-10-08", &[40; 50]);
        for (i, sale) in body["sales"].as_array_mut().unwrap().iter_mut().enumerate() {
            sale["ref"] = json!(hex((b * per_batch) as u64 + i as u64));
        }
        add(&mut store, body, "2026-10-08").unwrap();
    }
    let mut over = batch(1, "primed_flow", "2026-10-08", &[40]);
    over["sales"][0]["ref"] = json!(hex(999_999));
    assert_eq!(
        add(&mut store, over, "2026-10-08"),
        Err(StatusCode::TOO_MANY_REQUESTS)
    );
}

#[test]
fn closed_weeks_fold_into_aggregates_and_lose_their_raw_rows() {
    // Sales in 2026-W39. By 2026-10-08 (W41) only W40 and W41 are still open.
    let mut store = store_at(at("2026-09-24T12:00:00Z"));
    for c in 1..=5 {
        add(
            &mut store,
            batch(c, "primed_flow", "2026-09-24", &[40]),
            "2026-09-24",
        )
        .unwrap();
    }
    add(
        &mut store,
        batch(9, "rare_item", "2026-09-24", &[500]),
        "2026-09-24",
    )
    .unwrap();
    store.tick(at("2026-10-08T12:00:00Z")).unwrap();

    let raw_left: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM sale", [], |r| r.get(0))
        .unwrap();
    assert_eq!(raw_left, 0, "a contributor id must not outlive its week");
    let prices = store.prices(at("2026-10-08T12:00:00Z"), false).unwrap();
    let folded = &prices.weeks[0];
    assert_eq!((folded.week.as_str(), folded.complete), ("2026-W39", true));
    assert_eq!(
        folded.items.len(),
        1,
        "the one-contributor item stays unpublished"
    );
    assert_eq!(folded.items[0].slug, "primed_flow");
    assert_eq!(
        prices
            .weeks
            .iter()
            .map(|w| w.week.as_str())
            .collect::<Vec<_>>(),
        ["2026-W39", "2026-W40", "2026-W41"]
    );
}

#[test]
fn the_previous_week_stays_open_for_late_sales() {
    let mut store = store_at(at("2026-10-05T00:30:00Z"));
    for c in 1..=5 {
        add(
            &mut store,
            batch(c, "primed_flow", "2026-10-02", &[40]),
            "2026-10-05",
        )
        .unwrap();
    }
    store.tick(at("2026-10-08T12:00:00Z")).unwrap();
    let prices = store.prices(at("2026-10-08T12:00:00Z"), false).unwrap();
    let w40 = prices.weeks.iter().find(|w| w.week == "2026-W40").unwrap();
    assert!(!w40.complete);
    assert_eq!(w40.items[0].contributors, 5);
}

#[test]
fn erase_removes_only_the_matching_contributors_week() {
    let mut store = store_at(at("2026-10-08T12:00:00Z"));
    let secret = "ab".repeat(32);
    let mut mine = batch(1, "primed_flow", "2026-10-08", &[40]);
    mine["erase"] = json!(contract::erase_hash(&secret));
    add(&mut store, mine, "2026-10-08").unwrap();
    add(
        &mut store,
        batch(2, "primed_flow", "2026-10-08", &[41]),
        "2026-10-08",
    )
    .unwrap();

    let wrong_week = validate_erase(
        serde_json::from_value(json!({"v":1,"week":"2026-W40","secret":secret})).unwrap(),
    )
    .unwrap();
    store.erase(&wrong_week).unwrap();
    let count = |s: &Store| -> i64 {
        s.conn()
            .query_row("SELECT COUNT(*) FROM sale", [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(count(&store), 2);

    let right = validate_erase(
        serde_json::from_value(json!({"v":1,"week":"2026-W41","secret":secret})).unwrap(),
    )
    .unwrap();
    store.erase(&right).unwrap();
    assert_eq!(count(&store), 1);
    let left: String = store
        .conn()
        .query_row("SELECT contributor FROM sale", [], |r| r.get(0))
        .unwrap();
    assert_eq!(left, hex(2));
}

#[test]
fn a_clock_that_moves_back_does_not_fold() {
    let mut store = store_at(at("2026-10-08T12:00:00Z"));
    assert!(store.tick(at("2026-10-08T11:00:00Z")).is_err());
}

#[test]
fn shapes_count_distinct_contributors() {
    let mut store = store_at(at("2026-10-08T12:00:00Z"));
    let report = |c: u64| {
        validate_shape(
            serde_json::from_value(json!({
                "v": 1, "contributor": hex(c), "erase": hex(c),
                "keys": [{"name": "advancedTrait", "type": "object", "fields": ["Tag", "Value"]}],
                "tags": [],
            }))
            .unwrap(),
        )
        .unwrap()
    };
    for c in [1, 1, 2] {
        store
            .add_shape(report(c), at("2026-10-08T12:00:00Z"))
            .unwrap();
    }
    let shapes = store.shapes(at("2026-10-08T12:00:00Z")).unwrap();
    let w41 = shapes.weeks.iter().find(|w| w.week == "2026-W41").unwrap();
    assert_eq!(w41.shapes.len(), 1);
    assert_eq!(w41.shapes[0].contributors, 2);
    assert_eq!(w41.shapes[0].shape["keys"][0]["name"], "advancedTrait");
}

async fn serve(state: Arc<Collector>) -> (String, tokio::task::JoinHandle<std::io::Result<()>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    (
        url,
        tokio::spawn(axum::serve(listener, router(state)).into_future()),
    )
}

#[tokio::test]
async fn http_accepts_valid_reports_and_refuses_anything_extra() {
    let state = Collector::new(Store::new(Connection::open_in_memory().unwrap()).unwrap());
    state.tick().unwrap();
    let (url, task) = serve(state).await;
    let client = reqwest::Client::new();
    let today = Utc::now().date_naive().to_string();
    let post = |path: &str, body: Value| client.post(format!("{url}{path}")).json(&body).send();

    let ok = batch(1, "primed_flow", &today, &[40]);
    assert_eq!(
        post("/api/reports/sales", ok.clone())
            .await
            .unwrap()
            .status(),
        204
    );
    let mut extra = ok.clone();
    extra["sales"][0]["partner"] = json!("SomeTenno");
    assert_eq!(
        post("/api/reports/sales", extra).await.unwrap().status(),
        400
    );
    let huge = json!({ "pad": "x".repeat(20_000) });
    assert!(post("/api/reports/sales", huge)
        .await
        .unwrap()
        .status()
        .is_client_error());
    let erase = json!({"v": 1, "week": "2026-W41", "secret": "ab".repeat(32)});
    assert_eq!(
        post("/api/reports/erase", erase).await.unwrap().status(),
        204
    );

    let prices = client
        .get(format!("{url}/api/reports/prices"))
        .send()
        .await
        .unwrap();
    assert_eq!(prices.status(), 200);
    assert_eq!(prices.headers()["cache-control"], "public, max-age=300");
    let prices: store::Prices = prices.json().await.unwrap();
    assert_eq!(prices.min_contributors, MIN_CONTRIBUTORS);
    assert!(
        prices.weeks.iter().all(|w| w.items.is_empty()),
        "one contributor publishes nothing"
    );
    task.abort();
}

#[tokio::test]
async fn a_saturated_quota_does_not_take_health_down() {
    let state = Collector::with_limit(
        Store::new(Connection::open_in_memory().unwrap()).unwrap(),
        1,
    );
    state.tick().unwrap();
    let (url, task) = serve(state).await;
    let client = reqwest::Client::new();
    for _ in 0..3 {
        let _ = client
            .get(format!("{url}/api/reports/prices"))
            .send()
            .await
            .unwrap();
    }
    let limited = client
        .get(format!("{url}/api/reports/prices"))
        .send()
        .await
        .unwrap();
    assert_eq!(limited.status(), 429);
    let health = client.get(format!("{url}/health")).send().await.unwrap();
    assert_eq!(health.status(), 204);
    task.abort();
}
