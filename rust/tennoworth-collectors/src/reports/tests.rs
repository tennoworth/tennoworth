use super::contract::{
    validate_erase, validate_riven_sales, validate_sales, validate_shape, validate_vote,
    Suggestion, Verdict,
};
use super::store::{
    aggregate, aggregate_rivens, aggregate_votes, RawRivenSale, RawSale, RawVote,
    MAX_SALES_PER_CONTRIBUTOR_WEEK, MAX_VOTES_PER_CONTRIBUTOR_WEEK, MIN_CONTRIBUTORS,
    MIN_TRADED_VOTERS,
};
use super::*;
use jiff::civil::Date;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::future::IntoFuture;

fn fixture() -> Value {
    serde_json::from_str(include_str!("../../../../tests/fixtures/reports.json")).unwrap()
}

fn fixture_today() -> Date {
    fixture()["today"].as_str().unwrap().parse().unwrap()
}

fn at(stamp: &str) -> Timestamp {
    stamp.parse().unwrap()
}

fn store_at(now: Timestamp) -> Store {
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
    assert_eq!(
        store
            .conn()
            .query_row("SELECT COUNT(*) FROM sale", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        MAX_SALES_PER_CONTRIBUTOR_WEEK,
        "an over-cap batch must roll back all inserts"
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
    let today = crate::utc_date(Timestamp::now()).to_string();
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
async fn a_read_flood_does_not_block_submissions() {
    // The published bands are public and anonymous; desktops depend on the
    // write routes. Spending the read allowance must leave submissions open.
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
    assert_eq!(limited.status(), 429, "the read lane is saturated");
    let erase = client
        .post(format!("{url}/api/reports/erase"))
        .json(
            &json!({"v": contract::CONTRACT_VERSION, "week": "2026-W44", "secret": "a".repeat(64)}),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(erase.status(), 204);
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

#[test]
fn iso_weeks_match_the_shared_vectors() {
    for v in fixture()["iso_weeks"].as_array().unwrap() {
        let day: Date = v["day"].as_str().unwrap().parse().unwrap();
        assert_eq!(contract::iso_week(day), v["week"].as_str().unwrap());
    }
}

#[test]
fn retries_at_the_weekly_cap_do_not_consume_quota() {
    let mut store = store_at(at("2026-10-08T12:00:00Z"));
    for offset in (0..500).step_by(50) {
        let mut body = batch(1, "primed_flow", "2026-10-08", &[40; 50]);
        for (i, sale) in body["sales"].as_array_mut().unwrap().iter_mut().enumerate() {
            sale["ref"] = json!(hex(offset + i as u64));
        }
        add(&mut store, body, "2026-10-08").unwrap();
    }
    add(
        &mut store,
        batch(1, "primed_flow", "2026-10-08", &[40; 50]),
        "2026-10-08",
    )
    .unwrap();
    assert_eq!(
        store
            .conn()
            .query_row("SELECT COUNT(*) FROM sale", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        500
    );
}

#[test]
fn duplicate_refs_in_a_batch_only_consume_one_slot() {
    let mut store = store_at(at("2026-10-08T12:00:00Z"));
    for offset in (0..450).step_by(50) {
        let mut body = batch(1, "primed_flow", "2026-10-08", &[40; 50]);
        for (i, sale) in body["sales"].as_array_mut().unwrap().iter_mut().enumerate() {
            sale["ref"] = json!(hex(offset + i as u64));
        }
        add(&mut store, body, "2026-10-08").unwrap();
    }
    let mut repeated = batch(1, "primed_flow", "2026-10-08", &[40; 50]);
    for sale in repeated["sales"].as_array_mut().unwrap() {
        sale["ref"] = json!(hex(999));
    }
    add(&mut store, repeated.clone(), "2026-10-08").unwrap();
    add(&mut store, repeated, "2026-10-08").unwrap();
    assert_eq!(
        store
            .conn()
            .query_row("SELECT COUNT(*) FROM sale", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        451
    );
}

#[test]
fn riven_sale_and_vote_cases_match_the_shared_fixture() {
    let today = fixture_today();
    for case in fixture()["riven_sales"].as_array().unwrap() {
        let parsed = serde_json::from_value(case["body"].clone())
            .ok()
            .and_then(|b| validate_riven_sales(b, today).ok());
        assert_eq!(
            parsed.is_some(),
            case["valid"].as_bool().unwrap(),
            "{}",
            case["case"]
        );
    }
    for case in fixture()["votes"].as_array().unwrap() {
        let parsed = serde_json::from_value(case["body"].clone())
            .ok()
            .and_then(|b| validate_vote(b).ok());
        assert_eq!(
            parsed.is_some(),
            case["valid"].as_bool().unwrap(),
            "{}",
            case["case"]
        );
    }
}

/// One contributor's riven sales of `weapon` with `verdict`, all on `day`.
fn riven_batch(contributor: u64, weapon: &str, verdict: &str, day: &str, prices: &[u32]) -> Value {
    json!({
        "v": 1,
        "contributor": hex(contributor),
        "erase": hex(contributor + 1_000_000),
        "sales": prices.iter().enumerate().map(|(i, plat)| json!({
            "ref": hex(i as u64), "day": day, "weapon": weapon,
            "verdict": verdict, "plat": plat,
        })).collect::<Vec<_>>(),
    })
}

fn add_rivens(store: &mut Store, body: Value, today: &str) -> Result<(), StatusCode> {
    let batch = validate_riven_sales(
        serde_json::from_value(body).unwrap(),
        today.parse().unwrap(),
    )
    .map_err(|_| StatusCode::BAD_REQUEST)?;
    store.add_riven_sales(batch)
}

fn vote_body(contributor: u64, weapon: &str, agree: bool, suggest: Option<Value>) -> Value {
    let mut body = json!({
        "v": 1, "contributor": hex(contributor), "erase": hex(contributor + 1_000_000),
        "weapon": weapon, "verdict": "weak", "agree": agree,
    });
    if let Some(suggest) = suggest {
        body["suggest"] = suggest;
    }
    body
}

fn add_vote(store: &mut Store, body: Value, now: &str) -> Result<(), StatusCode> {
    let vote = validate_vote(serde_json::from_value(body).unwrap())
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    store.add_vote(vote, at(now))
}

fn raw_riven(contributor: &str, verdict: Verdict, plat: u32) -> RawRivenSale {
    RawRivenSale {
        weapon: "acceltra".into(),
        verdict,
        contributor: contributor.into(),
        plat,
    }
}

#[test]
fn riven_prices_are_per_weapon_and_verdict_with_one_vote_each() {
    let mut rows: Vec<RawRivenSale> = (0..50)
        .map(|_| raw_riven("loud", Verdict::Sellable, 9000))
        .collect();
    for (c, plat) in [("a", 100), ("b", 120), ("c", 140), ("d", 160)] {
        rows.push(raw_riven(c, Verdict::Sellable, plat));
    }
    for c in ["a", "b", "c", "d"] {
        rows.push(raw_riven(c, Verdict::Weak, 20));
    }
    let published = aggregate_rivens(&rows, MIN_CONTRIBUTORS);
    assert_eq!(published.len(), 1, "weak has four sellers, under the floor");
    let row = &published[0];
    assert_eq!(
        (row.weapon.as_str(), row.verdict),
        ("acceltra", Verdict::Sellable)
    );
    assert_eq!((row.contributors, row.sales), (5, 54));
    assert_eq!(row.median, 140.0, "the loud seller is one vote of five");
}

fn raw_vote(contributor: &str, agree: bool, suggest: Option<(&str, bool, bool)>) -> RawVote {
    RawVote {
        contributor: contributor.into(),
        weapon: "acceltra".into(),
        verdict: Verdict::Weak,
        agree,
        suggest: suggest.map(|(stat, positive, good)| Suggestion {
            stat: stat.into(),
            positive,
            good,
        }),
    }
}

fn sold(contributors: &[&str]) -> BTreeSet<(String, String)> {
    contributors
        .iter()
        .map(|c| (c.to_string(), "acceltra".to_string()))
        .collect()
}

#[test]
fn a_suggestion_is_proposed_only_with_enough_trading_voters_and_a_majority() {
    let zoom = Some(("zoom", false, true));
    let five: Vec<RawVote> = ["a", "b", "c", "d", "e"]
        .iter()
        .map(|c| raw_vote(c, false, zoom))
        .collect();

    let fresh_ids = aggregate_votes(&five, &sold(&[]), MIN_CONTRIBUTORS);
    assert!(
        !fresh_ids[0].suggestions[0].proposed,
        "five ids that never sold a riven are free to make"
    );
    let one_trader = aggregate_votes(&five, &sold(&["a"]), MIN_CONTRIBUTORS);
    assert!(!one_trader[0].suggestions[0].proposed);
    let traders = aggregate_votes(&five, &sold(&["a", "b"]), MIN_CONTRIBUTORS);
    let s = &traders[0].suggestions[0];
    assert_eq!(
        (s.voters, s.traded, s.proposed),
        (5, MIN_TRADED_VOTERS, true)
    );
    assert_eq!(traders[0].traded, 2);

    let mut outvoted = five;
    for c in ["f", "g", "h", "i", "j"] {
        outvoted.push(raw_vote(c, true, None));
    }
    let split = aggregate_votes(&outvoted, &sold(&["a", "b"]), MIN_CONTRIBUTORS);
    assert!(
        !split[0].suggestions[0].proposed,
        "half the voters agree with the guide as it is"
    );
    assert_eq!(split[0].verdicts[0].agree, 5);

    let four: Vec<RawVote> = ["a", "b", "c", "d"]
        .iter()
        .map(|c| raw_vote(c, false, zoom))
        .collect();
    assert!(aggregate_votes(&four, &sold(&["a", "b"]), MIN_CONTRIBUTORS).is_empty());
}

#[test]
fn a_contributor_rates_a_weapon_once_a_week_and_may_change_their_mind() {
    let mut store = store_at(at("2026-10-08T12:00:00Z"));
    let now = "2026-10-08T12:00:00Z";
    add_vote(&mut store, vote_body(1, "acceltra", true, None), now).unwrap();
    add_vote(
        &mut store,
        vote_body(
            1,
            "acceltra",
            false,
            Some(json!({"stat": "zoom", "positive": false, "good": true})),
        ),
        now,
    )
    .unwrap();
    for c in 2..=5 {
        add_vote(&mut store, vote_body(c, "acceltra", true, None), now).unwrap();
    }
    let votes = store.roll_votes(at(now), false).unwrap();
    let w41 = votes.weeks.iter().find(|w| w.week == "2026-W41").unwrap();
    let acceltra = &w41.weapons[0];
    assert_eq!(acceltra.voters, 5);
    assert_eq!(
        acceltra.verdicts[0].agree, 4,
        "the first rating was replaced"
    );
    assert_eq!(acceltra.suggestions.len(), 1);
}

#[test]
fn a_contributor_past_the_weekly_vote_cap_is_refused() {
    let mut store = store_at(at("2026-10-08T12:00:00Z"));
    let now = "2026-10-08T12:00:00Z";
    for w in 0..MAX_VOTES_PER_CONTRIBUTOR_WEEK {
        add_vote(
            &mut store,
            vote_body(1, &format!("weapon_{w}"), true, None),
            now,
        )
        .unwrap();
    }
    assert_eq!(
        add_vote(&mut store, vote_body(1, "one_more", true, None), now),
        Err(StatusCode::TOO_MANY_REQUESTS)
    );
    add_vote(&mut store, vote_body(1, "weapon_0", false, None), now)
        .expect("changing an existing rating is not a new one");
}

#[test]
fn riven_sales_and_votes_fold_with_their_week_and_erase_with_it() {
    let mut store = store_at(at("2026-09-24T12:00:00Z"));
    let then = "2026-09-24T12:00:00Z";
    for c in 1..=5 {
        add_rivens(
            &mut store,
            riven_batch(
                c,
                "acceltra",
                "sellable",
                "2026-09-24",
                &[100 + c as u32 * 10],
            ),
            "2026-09-24",
        )
        .unwrap();
        add_vote(
            &mut store,
            vote_body(
                c,
                "acceltra",
                false,
                Some(json!({"stat": "zoom", "positive": false, "good": true})),
            ),
            then,
        )
        .unwrap();
    }
    store.tick(at("2026-10-08T12:00:00Z")).unwrap();
    for table in ["riven_sale", "roll_vote"] {
        let left: i64 = store
            .conn()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            left, 0,
            "{table}: a contributor id must not outlive its week"
        );
    }
    let now = at("2026-10-08T12:00:00Z");
    let prices = store.riven_prices(now, false).unwrap();
    let w39 = &prices.weeks[0];
    assert_eq!((w39.week.as_str(), w39.complete), ("2026-W39", true));
    assert_eq!(w39.rivens[0].median, 130.0);
    let votes = store.roll_votes(now, false).unwrap();
    let folded = &votes.weeks[0].weapons[0];
    assert_eq!((folded.voters, folded.traded), (5, 5));
    assert!(folded.suggestions[0].proposed);

    let mut fresh = store_at(now);
    let today = "2026-10-08T12:00:00Z";
    let secret = "ab".repeat(32);
    let mut mine = riven_batch(1, "acceltra", "weak", "2026-10-08", &[30]);
    mine["erase"] = json!(contract::erase_hash(&secret));
    add_rivens(&mut fresh, mine, "2026-10-08").unwrap();
    let mut my_vote = vote_body(1, "acceltra", true, None);
    my_vote["erase"] = json!(contract::erase_hash(&secret));
    add_vote(&mut fresh, my_vote, today).unwrap();
    add_vote(&mut fresh, vote_body(2, "acceltra", true, None), today).unwrap();
    let request = validate_erase(
        serde_json::from_value(json!({"v":1,"week":"2026-W41","secret":secret})).unwrap(),
    )
    .unwrap();
    fresh.erase(&request).unwrap();
    let count = |table: &str| -> i64 {
        fresh
            .conn()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!((count("riven_sale"), count("roll_vote")), (0, 1));
}

#[tokio::test]
async fn http_takes_riven_sales_and_votes_and_publishes_their_summaries() {
    let state = Collector::new(Store::new(Connection::open_in_memory().unwrap()).unwrap());
    state.tick().unwrap();
    let (url, task) = serve(state).await;
    let client = reqwest::Client::new();
    let today = crate::utc_date(Timestamp::now()).to_string();
    let post = |path: &str, body: Value| client.post(format!("{url}{path}")).json(&body).send();

    let sale = riven_batch(1, "acceltra", "sellable", &today, &[120]);
    assert_eq!(
        post("/api/reports/riven-sales", sale.clone())
            .await
            .unwrap()
            .status(),
        204
    );
    let mut with_stats = sale;
    with_stats["sales"][0]["stats"] = json!(["critical_damage"]);
    assert_eq!(
        post("/api/reports/riven-sales", with_stats)
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        post(
            "/api/reports/roll-vote",
            vote_body(1, "acceltra", true, None)
        )
        .await
        .unwrap()
        .status(),
        204
    );
    assert_eq!(
        post(
            "/api/reports/roll-vote",
            vote_body(
                1,
                "acceltra",
                true,
                Some(json!({"stat": "zoom", "positive": false, "good": true}))
            )
        )
        .await
        .unwrap()
        .status(),
        400
    );

    let prices = client
        .get(format!("{url}/api/reports/riven-prices"))
        .send()
        .await
        .unwrap();
    assert_eq!(prices.status(), 200);
    let prices: store::RivenPrices = prices.json().await.unwrap();
    assert!(
        prices.weeks.iter().all(|w| w.rivens.is_empty()),
        "one seller publishes nothing"
    );
    let votes = client
        .get(format!("{url}/api/reports/roll-votes"))
        .send()
        .await
        .unwrap();
    assert_eq!(votes.status(), 200);
    let votes: store::RollVotes = votes.json().await.unwrap();
    assert_eq!(
        (votes.min_contributors, votes.min_traded),
        (MIN_CONTRIBUTORS, MIN_TRADED_VOTERS)
    );
    assert!(
        votes.weeks.iter().all(|w| w.weapons.is_empty()),
        "one voter publishes nothing"
    );
    task.abort();
}
