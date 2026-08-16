//! Live top-of-book prices from warframe.market v2 — `/orders/item/{slug}/top`.
//!
//! The Sell view prices from `market.json`, a snapshot up to two hours old
//! that narrows each item to one tier (rank 0 / one relic refinement) with
//! our own filtering over the whole order book. WFM's `top` endpoint answers
//! the exact question the listing modal asks — "what are the ≤5 best asks
//! and bids for THIS tier, from players who are online right now" — and
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

use crate::util::browser_client;

/// Start-to-start spacing between requests — 340 ms ≈ 2.9 req/s, under WFM's
/// documented 3 req/s. Same figure the scraper uses.
pub const LIVE_TOP_SPACING: Duration = Duration::from_millis(340);

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

/// The answer for one query. `sells` / `buys` are the platinum values of the
/// ≤5 best asks / bids WFM returned (online players only), best first;
/// `low_sell` / `top_buy` are their heads for convenience.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct LiveTop {
    pub slug: String,
    #[serde(default)]
    pub rank: Option<u32>,
    #[serde(default)]
    pub subtype: Option<String>,
    pub sells: Vec<u32>,
    pub buys: Vec<u32>,
    pub low_sell: Option<u32>,
    pub top_buy: Option<u32>,
    /// The caller's OWN ask/bid on this tier, when [`fetch_live_tops`] was
    /// given a username and one of the ≤5 top orders is theirs. Those orders
    /// are excluded from `sells` / `buys`, so `low_sell` is "the best ask that
    /// is not mine" — the number a repricing decision actually needs (the
    /// snapshot can't tell whose order is whose; this can).
    #[serde(default)]
    pub own_ask: Option<u32>,
    #[serde(default)]
    pub own_bid: Option<u32>,
    /// Set when this one lookup failed (item unknown to WFM, network blip);
    /// the batch keeps going and the UI shows the row as "no live data".
    #[serde(default)]
    pub error: Option<String>,
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
        }
    }
}

fn top_url(q: &LiveTopQuery) -> String {
    let mut url = format!(
        "https://api.warframe.market/v2/orders/item/{}/top",
        q.slug
    );
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
    let Some(me) = me.filter(|m| !m.is_empty()) else { return false };
    let user = order.get("user");
    let by = |k: &str| user.and_then(|u| u.get(k)).and_then(|v| v.as_str());
    by("ingameName").is_some_and(|n| n.eq_ignore_ascii_case(me))
        || by("slug").is_some_and(|n| n.eq_ignore_ascii_case(me))
}

/// Parse WFM's `top` envelope: `{data:{sell:[{platinum,..}],buy:[...]}}`.
/// Sorted defensively — WFM already returns best-first, but the contract is
/// ours to keep, not theirs. Orders by `me` are split out into `own_ask` /
/// `own_bid` rather than counted as competition.
pub fn parse_top(q: &LiveTopQuery, body: &serde_json::Value, me: Option<&str>) -> Result<LiveTop> {
    let data = wfm_client::unwrap_envelope(body);
    if data.get("sell").is_none() && data.get("buy").is_none() {
        bail!("unexpected /top shape for {}: no sell/buy arrays", q.slug);
    }
    // (others' prices, my price if present)
    let side = |name: &str| -> (Vec<u32>, Option<u32>) {
        let mut others = Vec::new();
        let mut mine = None;
        for o in data.get(name).and_then(|v| v.as_array()).into_iter().flatten() {
            let Some(p) = o.get("platinum").and_then(|p| p.as_u64()) else { continue };
            let p = p.min(u32::MAX as u64) as u32;
            if is_own_order(o, me) {
                mine = Some(mine.map_or(p, |m: u32| m.min(p)));
            } else {
                others.push(p);
            }
        }
        (others, mine)
    };
    let (mut sells, own_ask) = side("sell");
    let (mut buys, own_bid) = side("buy");
    sells.sort_unstable();
    buys.sort_unstable_by(|a, b| b.cmp(a));
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
    })
}

fn fetch_one(client: &Client, platform: &str, q: &LiveTopQuery, me: Option<&str>) -> Result<LiveTop> {
    let url = top_url(q);
    let resp = wfm_client::wfm_headers(client.get(&url), platform)
        .send()
        .with_context(|| format!("GET {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        bail!("{url}: HTTP {status}");
    }
    let body: serde_json::Value = resp.json().with_context(|| format!("{url}: JSON"))?;
    parse_top(q, &body, me)
}

/// Look up every query, paced at [`LIVE_TOP_SPACING`] start-to-start. Per-item
/// failures are returned inline (`error` set), never propagated — a 50-item
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
    let mut last_start: Option<Instant> = None;
    for (i, q) in queries.iter().enumerate() {
        if let Some(t) = last_start {
            let elapsed = t.elapsed();
            if elapsed < LIVE_TOP_SPACING {
                thread::sleep(LIVE_TOP_SPACING - elapsed);
            }
        }
        last_start = Some(Instant::now());
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

    fn q(slug: &str, rank: Option<u32>, subtype: Option<&str>) -> LiveTopQuery {
        LiveTopQuery { slug: slug.into(), rank, subtype: subtype.map(String::from) }
    }

    #[test]
    fn url_carries_rank_and_subtype_only_when_set() {
        assert_eq!(top_url(&q("primed_flow", Some(0), None)),
            "https://api.warframe.market/v2/orders/item/primed_flow/top?rank=0");
        assert_eq!(top_url(&q("lith_c5_relic", None, Some("intact"))),
            "https://api.warframe.market/v2/orders/item/lith_c5_relic/top?subtype=intact");
        assert_eq!(top_url(&q("volt_prime_set", None, Some(""))),
            "https://api.warframe.market/v2/orders/item/volt_prime_set/top");
        assert_eq!(top_url(&q("x", Some(3), Some("radiant"))),
            "https://api.warframe.market/v2/orders/item/x/top?rank=3&subtype=radiant");
    }

    #[test]
    fn parses_the_v2_envelope_best_first() {
        let body = json!({"apiVersion":"0.25.0","data":{
            "sell":[{"platinum":20},{"platinum":15},{"platinum":18}],
            "buy":[{"platinum":9},{"platinum":12}]}});
        let t = parse_top(&q("primed_flow", Some(0), None), &body, None).unwrap();
        assert_eq!(t.sells, vec![15, 18, 20]);
        assert_eq!(t.buys, vec![12, 9]);
        assert_eq!(t.low_sell, Some(15));
        assert_eq!(t.top_buy, Some(12));
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
        assert_eq!(t.own_ask, Some(12));
        assert_eq!(t.own_bid, Some(8));
        assert_eq!(t.low_sell, Some(14), "the best ask that is NOT mine");
        assert_eq!(t.top_buy, Some(9));
        // no username → nothing is "mine"
        let t2 = parse_top(&q("primed_flow", Some(0), None), &body, None).unwrap();
        assert_eq!(t2.own_ask, None);
        assert_eq!(t2.low_sell, Some(12));
    }
}
