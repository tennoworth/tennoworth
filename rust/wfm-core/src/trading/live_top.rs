//! Live top-of-book prices from warframe.market v2 - `/orders/item/{slug}/top`.
//!
//! The Sell view prices from `market.json`, a snapshot up to two hours old
//! that narrows each item to one tier (rank 0 / one relic refinement) with
//! our own filtering over the whole order book. WFM's `top` endpoint answers
//! the exact question the listing modal asks - "what are the ≤5 best asks
//! and bids for THIS tier, from players who are online right now" - and
//! accepts `rank` / `subtype` (and `charges` / `stars`) so the tier is chosen
//! server-side. Public: no JWT, so it works before login too.
//!
//! Every request goes through [`fetch_live_tops`], which paces itself to
//! WFM's 3 req/s ceiling; a batch of 50 items is ~17 s, which is why the
//! caller reports progress rather than blocking silently.

use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use crate::http::browser_client;

/// Start-to-start spacing between requests - 340 ms ≈ 2.9 req/s, under WFM's
/// documented 3 req/s. Same figure the scraper uses.
pub const LIVE_TOP_SPACING: Duration = Duration::from_millis(340);
static LAST_LIVE_START: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

/// One item's tier to look up.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct LiveTopQuery {
    pub slug: String,
    /// Mod/arcane rank; `None` for rankless items. WFM ignores it on those.
    #[serde(default)]
    pub rank: Option<u32>,
    /// Relic refinement (`intact` …), fish/gem sizes, etc.
    #[serde(default)]
    pub subtype: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct BuyerOrder {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub user_slug: String,
    pub status: String,
    pub platform: String,
    pub crossplay: bool,
    pub quantity: u32,
    pub per_trade: u32,
    pub platinum: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct BuyerBook {
    pub orders: Vec<BuyerOrder>,
    pub own_orders_excluded: bool,
    pub observed_at: Option<String>,
}

/// The answer for one query. `sells` / `buys` are the platinum values of the
/// ≤5 best asks / bids WFM returned (online players only), best first;
/// `low_sell` / `top_buy` are their heads for convenience.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct LiveTop {
    pub slug: String,
    #[serde(default)]
    pub rank: Option<u32>,
    #[serde(default)]
    pub subtype: Option<String>,
    pub sells: Vec<f64>,
    pub buys: Vec<f64>,
    pub low_sell: Option<f64>,
    pub top_buy: Option<f64>,
    /// The caller's OWN ask/bid on this tier, when [`fetch_live_tops`] was
    /// given a username and one of the ≤5 top orders is theirs. Those orders
    /// are excluded from `sells` / `buys`, so `low_sell` is "the best ask that
    /// is not mine" - the number a repricing decision actually needs (the
    /// snapshot can't tell whose order is whose; this can).
    #[serde(default)]
    pub own_ask: Option<f64>,
    #[serde(default)]
    pub own_bid: Option<f64>,
    /// Set when this one lookup failed (item unknown to WFM, network blip);
    /// the batch keeps going and the UI shows the row as "no live data".
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub buyer_book: Option<BuyerBook>,
}

impl LiveTop {
    fn failed(q: &LiveTopQuery, e: String) -> Self {
        LiveTop {
            slug: q.slug.clone(),
            rank: q.rank,
            subtype: q.subtype.clone(),
            sells: vec![],
            buys: vec![],
            low_sell: None,
            top_buy: None,
            own_ask: None,
            own_bid: None,
            error: Some(e),
            buyer_book: None,
        }
    }
}

fn buyer_orders(q: &LiveTopQuery, data: &serde_json::Value, me: Option<&str>) -> Option<BuyerBook> {
    let rows = data.get("buy")?.as_array()?;
    let mut orders = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for row in rows {
        let parse = || -> Option<BuyerOrder> {
            let user = row.get("user")?;
            let text = |value: &serde_json::Value, key: &str| -> Option<String> {
                value
                    .get(key)?
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 100)
                    .map(String::from)
            };
            let positive = |key: &str| -> Option<u32> {
                u32::try_from(row.get(key)?.as_u64()?)
                    .ok()
                    .filter(|n| *n > 0)
            };
            let rank = match row.get("rank").filter(|v| !v.is_null()) {
                Some(value) => value.as_u64()?,
                None => 0,
            };
            let subtype = row.get("subtype").filter(|v| !v.is_null());
            if is_own_order(row, me)
                || !row.get("visible")?.as_bool()?
                || row.get("type")?.as_str()? != "buy"
                || rank != u64::from(q.rank.unwrap_or(0))
                || subtype.and_then(|v| v.as_str())
                    != q.subtype.as_deref().filter(|s| !s.is_empty())
                || subtype.is_some_and(|v| !v.is_string())
                || ["charges", "amberStars", "cyanStars"]
                    .iter()
                    .any(|k| row.get(k).is_some_and(|v| !v.is_null()))
            {
                return None;
            }
            let status = text(user, "status")?;
            if !matches!(status.as_str(), "online" | "ingame") {
                return None;
            }
            let quantity = positive("quantity")?;
            let per_trade = match row.get("perTrade").filter(|v| !v.is_null()) {
                Some(_) => positive("perTrade")?,
                None => 1,
            };
            if per_trade > 6 || quantity % per_trade != 0 {
                return None;
            }
            Some(BuyerOrder {
                id: text(row, "id")?,
                user_id: text(user, "id")?,
                name: text(user, "ingameName")?,
                user_slug: text(user, "slug")?,
                status,
                platform: text(user, "platform")?,
                crossplay: user.get("crossplay")?.as_bool()?,
                quantity,
                per_trade,
                platinum: positive("platinum")?,
            })
        };
        if let Some(order) = parse() {
            if seen.insert(order.user_id.clone()) {
                orders.push(order);
            }
        }
    }
    orders.sort_by(|a, b| {
        (f64::from(b.platinum) / f64::from(b.per_trade))
            .total_cmp(&(f64::from(a.platinum) / f64::from(a.per_trade)))
            .then_with(|| a.id.cmp(&b.id))
    });
    orders.truncate(5);
    Some(BuyerBook {
        orders,
        own_orders_excluded: me.is_some_and(|s| !s.is_empty()),
        observed_at: None,
    })
}

fn top_url(q: &LiveTopQuery) -> String {
    let mut url = format!("https://api.warframe.market/v2/orders/item/{}/top", q.slug);
    let mut params: Vec<String> = Vec::new();
    if let Some(r) = q.rank {
        params.push(format!("rank={r}"));
    }
    if let Some(s) = q.subtype.as_deref().filter(|s| !s.is_empty()) {
        params.push(format!("subtype={s}"));
    }
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }
    url
}

/// Does this order belong to `me` (WFM in-game name, case-insensitive; the
/// user's `slug` is its lowercase form so both are checked)?
fn is_own_order(order: &serde_json::Value, me: Option<&str>) -> bool {
    let Some(me) = me.filter(|m| !m.is_empty()) else {
        return false;
    };
    let user = order.get("user");
    let by = |k: &str| user.and_then(|u| u.get(k)).and_then(|v| v.as_str());
    by("ingameName").is_some_and(|n| n.eq_ignore_ascii_case(me))
        || by("slug").is_some_and(|n| n.eq_ignore_ascii_case(me))
}

/// Parse WFM's `top` envelope: `{data:{sell:[{platinum,..}],buy:[...]}}`.
/// Sorted defensively - WFM already returns best-first, but the contract is
/// ours to keep, not theirs. Orders by `me` are split out into `own_ask` /
/// `own_bid` rather than counted as competition.
pub fn parse_top(q: &LiveTopQuery, body: &serde_json::Value, me: Option<&str>) -> Result<LiveTop> {
    let data = wfm_client::unwrap_envelope(body);
    if data.get("sell").is_none() && data.get("buy").is_none() {
        bail!("unexpected /top shape for {}: no sell/buy arrays", q.slug);
    }
    // (others' prices, my price if present)
    let side = |name: &str| -> (Vec<f64>, Option<f64>) {
        let mut others = Vec::new();
        let mut mine = None;
        for o in data
            .get(name)
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
        {
            let Some(p) = o
                .get("platinum")
                .and_then(|p| p.as_f64())
                .and_then(|p| wfm_client::unit_price(p, o.get("perTrade")))
            else {
                continue;
            };
            if is_own_order(o, me) {
                mine =
                    Some(mine.map_or(p, |m: f64| if name == "sell" { m.min(p) } else { m.max(p) }));
            } else {
                others.push(p);
            }
        }
        (others, mine)
    };
    let (mut sells, own_ask) = side("sell");
    let (mut buys, own_bid) = side("buy");
    sells.sort_unstable_by(f64::total_cmp);
    buys.sort_unstable_by(|a, b| b.total_cmp(a));
    Ok(LiveTop {
        slug: q.slug.clone(),
        rank: q.rank,
        subtype: q.subtype.clone(),
        low_sell: sells.first().copied(),
        top_buy: buys.first().copied(),
        sells,
        buys,
        own_ask,
        own_bid,
        error: None,
        buyer_book: buyer_orders(q, data, me),
    })
}

fn fetch_one(
    client: &Client,
    platform: &str,
    q: &LiveTopQuery,
    me: Option<&str>,
) -> Result<LiveTop> {
    let url = top_url(q);
    let resp = wfm_client::wfm_headers(client.get(&url), platform)
        .send()
        .with_context(|| format!("GET {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        bail!("{url}: HTTP {status}");
    }
    let body: serde_json::Value = resp.json().with_context(|| format!("{url}: JSON"))?;
    let mut top = parse_top(q, &body, me)?;
    if let Some(book) = &mut top.buyer_book {
        book.orders
            .retain(|order| order.platform == platform || order.crossplay);
        book.observed_at = Some(crate::time::chrono_now_iso());
    }
    Ok(top)
}

/// Look up every query, paced at [`LIVE_TOP_SPACING`] start-to-start. Per-item
/// failures are returned inline (`error` set), never propagated - a 50-item
/// review must not lose 49 answers to one unknown slug. `me` (the WFM
/// in-game name, when logged in) keeps the user's own orders out of the
/// competition figures. `on_progress` is called after each item with
/// (done, total).
pub fn fetch_live_tops(
    platform: &str,
    me: Option<&str>,
    queries: &[LiveTopQuery],
    mut on_progress: impl FnMut(usize, usize),
) -> Result<Vec<LiveTop>> {
    let client = browser_client(20)?;
    let total = queries.len();
    let mut out = Vec::with_capacity(total);
    for (i, q) in queries.iter().enumerate() {
        // Buyer comparisons and batch repricing can be requested together.
        // Pace their request starts across invocations, without holding the
        // lock during network I/O.
        {
            let mut last_start = crate::poison::guard(&LAST_LIVE_START);
            if let Some(t) = *last_start {
                let elapsed = t.elapsed();
                if elapsed < LIVE_TOP_SPACING {
                    thread::sleep(LIVE_TOP_SPACING.saturating_sub(elapsed));
                }
            }
            *last_start = Some(Instant::now());
        }
        let row = match fetch_one(&client, platform, q, me) {
            Ok(t) => t,
            Err(e) => LiveTop::failed(q, e.to_string()),
        };
        out.push(row);
        on_progress(i + 1, total);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn buyer_book_matches_frontend_contract_fixture() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/buyer-alternatives/book.json"
        ))
        .unwrap();
        let query = serde_json::from_value(fixture["query"].clone()).unwrap();
        let book = parse_top(&query, &fixture["body"], fixture["me"].as_str())
            .unwrap()
            .buyer_book
            .unwrap();
        assert_eq!(
            serde_json::to_value(book.orders).unwrap(),
            fixture["orders"]
        );
    }

    fn q(slug: &str, rank: Option<u32>, subtype: Option<&str>) -> LiveTopQuery {
        LiveTopQuery {
            slug: slug.into(),
            rank,
            subtype: subtype.map(String::from),
        }
    }

    #[test]
    fn url_carries_rank_and_subtype_only_when_set() {
        assert_eq!(
            top_url(&q("primed_flow", Some(0), None)),
            "https://api.warframe.market/v2/orders/item/primed_flow/top?rank=0"
        );
        assert_eq!(
            top_url(&q("lith_c5_relic", None, Some("intact"))),
            "https://api.warframe.market/v2/orders/item/lith_c5_relic/top?subtype=intact"
        );
        assert_eq!(
            top_url(&q("volt_prime_set", None, Some(""))),
            "https://api.warframe.market/v2/orders/item/volt_prime_set/top"
        );
        assert_eq!(
            top_url(&q("x", Some(3), Some("radiant"))),
            "https://api.warframe.market/v2/orders/item/x/top?rank=3&subtype=radiant"
        );
    }

    #[test]
    fn parses_the_v2_envelope_best_first() {
        let body = json!({"apiVersion":"0.25.0","data":{
            "sell":[{"platinum":20},{"platinum":15},{"platinum":18}],
            "buy":[{"platinum":9},{"platinum":12}]}});
        let t = parse_top(&q("primed_flow", Some(0), None), &body, None).unwrap();
        assert_eq!(t.sells, vec![15.0, 18.0, 20.0]);
        assert_eq!(t.buys, vec![12.0, 9.0]);
        assert_eq!(t.low_sell, Some(15.0));
        assert_eq!(t.top_buy, Some(12.0));
        assert!(t.error.is_none());
    }

    #[test]
    fn empty_sides_are_none_not_zero() {
        let body = json!({"data":{"sell":[],"buy":[]}});
        let t = parse_top(&q("thin", None, None), &body, None).unwrap();
        assert_eq!(t.low_sell, None);
        assert_eq!(t.top_buy, None);
    }

    #[test]
    fn buyer_depth_keeps_identity_and_lots_and_rejects_incompatible_rows() {
        let order = json!({"id":"order-a", "type":"buy", "visible":true,
            "rank":0, "platinum":24, "quantity":6, "perTrade":2,
            "user":{"id":"buyer-a", "slug":"buyer_a", "ingameName":"BuyerA",
                "status":"ingame", "platform":"pc", "crossplay":true}});
        let mut invalid = Vec::new();
        for (field, value) in [
            ("rank", json!(5)),
            ("quantity", json!(5)),
            ("quantity", json!(0)),
            ("perTrade", json!(7)),
            ("platinum", json!(-1)),
            ("visible", json!(false)),
            ("type", json!("sell")),
            ("charges", json!(1)),
            ("subtype", json!("radiant")),
            ("subtype", json!(123)),
        ] {
            let mut row = order.clone();
            row[field] = value;
            invalid.push(row);
        }
        let mut own = order.clone();
        own["user"]["ingameName"] = json!("Me");
        invalid.push(own);
        let mut offline = order.clone();
        offline["user"]["status"] = json!("offline");
        invalid.push(offline);
        let mut no_identity = order.clone();
        no_identity["user"]["id"] = json!(null);
        invalid.push(no_identity);
        invalid.push(order.clone());
        invalid.push(order);
        let top = parse_top(
            &q("arcane", Some(0), None),
            &json!({"data":{"buy":invalid,"sell":[]}}),
            Some("me"),
        )
        .unwrap();
        let book = top.buyer_book.unwrap();
        assert!(book.own_orders_excluded);
        assert_eq!(book.orders.len(), 1);
        assert_eq!(book.orders[0].name, "BuyerA");
        assert_eq!(book.orders[0].platinum, 24);
        assert_eq!(book.orders[0].quantity, 6);
        assert_eq!(book.orders[0].per_trade, 2);
    }

    #[test]
    fn missing_buyer_array_remains_unknown_and_logged_out_books_are_labeled() {
        let missing =
            parse_top(&q("item", None, None), &json!({"data":{"sell":[]}}), None).unwrap();
        assert!(missing.buyer_book.is_none());
        let empty = parse_top(
            &q("item", None, None),
            &json!({"data":{"sell":[],"buy":[]}}),
            None,
        )
        .unwrap();
        assert!(!empty.buyer_book.unwrap().own_orders_excluded);
    }

    #[test]
    fn compares_bulk_orders_per_unit_without_losing_fractional_prices() {
        let body = json!({"data": {
            "sell": [{"platinum":48,"perTrade":6}, {"platinum":36,"perTrade":5},
                {"platinum":1,"perTrade":0},
                {"platinum":24,"perTrade":3,"user":{"slug":"me"}}],
            "buy": [{"platinum":35,"perTrade":5}]
        }});
        let t = parse_top(&q("arcane_energize", Some(0), None), &body, Some("me")).unwrap();
        assert_eq!(t.sells, vec![7.2, 8.0]);
        assert_eq!(t.low_sell, Some(7.2));
        assert_eq!(t.top_buy, Some(7.0));
        assert_eq!(t.own_ask, Some(8.0));
    }

    #[test]
    fn a_body_without_order_arrays_is_an_error_not_a_zero_price() {
        let body = json!({"data":{"nope":true}});
        assert!(parse_top(&q("x", None, None), &body, None).is_err());
    }

    #[test]
    fn my_own_orders_are_split_out_not_counted_as_competition() {
        let body = json!({"data":{
            "sell":[
                {"platinum":12,"user":{"ingameName":"Prowly","slug":"prowly"}},
                {"platinum":14,"user":{"ingameName":"Someone","slug":"someone"}}],
            "buy":[
                {"platinum":9,"user":{"ingameName":"Other","slug":"other"}},
                {"platinum":8,"user":{"ingameName":"prowly","slug":"prowly"}}]}});
        let t = parse_top(&q("primed_flow", Some(0), None), &body, Some("PROWLY")).unwrap();
        assert_eq!(t.own_ask, Some(12.0));
        assert_eq!(t.own_bid, Some(8.0));
        assert_eq!(t.low_sell, Some(14.0), "the best ask that is NOT mine");
        assert_eq!(t.top_buy, Some(9.0));
        // no username → nothing is "mine"
        let t2 = parse_top(&q("primed_flow", Some(0), None), &body, None).unwrap();
        assert_eq!(t2.own_ask, None);
        assert_eq!(t2.low_sell, Some(12.0));
    }
}
