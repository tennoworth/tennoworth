//! On-demand riven auction comparables from warframe.market's v1 auctions
//! endpoint.
//!
//! The Rivens view shows the user's own rivens; for each one a "Show comps"
//! button asks WFM for the cheapest matching auctions. WFM's API rules cap
//! auction searches at 10 requests/minute, so every call passes through a
//! process-wide sliding-window gate — two comps clicked back to back share
//! the budget instead of each assuming a fresh one.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::poison::guard;
use crate::util::browser_client;

/// WFM's documented cap: 10 auction searches per minute.
pub const AUCTIONS_PER_MIN: usize = 10;
/// The window the cap applies over.
pub const AUCTIONS_WINDOW: Duration = Duration::from_secs(60);
/// How many comps rows we keep per weapon.
pub const COMPS_LIMIT: usize = 20;

pub const AUCTIONS_SEARCH_URL: &str = "https://api.warframe.market/v1/auctions/search";

/// One attribute line of a riven auction. WFM sends percent attributes in
/// display units (`83.1` means `+83.1%`) and scalar attributes as their scalar.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RivenAuctionAttribute {
    pub url_name: String,
    pub value: f64,
    pub positive: bool,
}

/// One auction, reduced to what a comps panel shows. `price` is the effective
/// ask: the buyout for direct sells, else the starting bid.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RivenAuction {
    pub id: String,
    pub price: u32,
    pub buyout_price: Option<u32>,
    pub starting_price: u32,
    pub top_bid: Option<u32>,
    pub is_direct_sell: bool,
    pub owner: Option<String>,
    pub owner_status: Option<String>,
    pub mod_rank: u32,
    pub mastery_level: u32,
    pub re_rolls: u32,
    pub polarity: Option<String>,
    pub name: Option<String>,
    pub platform: Option<String>,
    pub attributes: Vec<RivenAuctionAttribute>,
}

/// The process-wide sliding-window gate: at most [`AUCTIONS_PER_MIN`] requests
/// in any rolling [`AUCTIONS_WINDOW`].
fn auction_gate() -> &'static Mutex<VecDeque<Instant>> {
    static GATE: OnceLock<Mutex<VecDeque<Instant>>> = OnceLock::new();
    GATE.get_or_init(|| Mutex::new(VecDeque::new()))
}

/// Block until an auctions-request slot is free, then consume one. The first
/// requests of a burst pass immediately (≤10), then callers sleep until the
/// oldest request ages out of the window.
pub fn pace_auction_request() {
    let gate = auction_gate();
    loop {
        // Recover a poisoned guard (see poison.rs) — a panic elsewhere must
        // not wedge every future comps click behind a second panic.
        let wait = {
            let mut stamps = guard(gate);
            let now = Instant::now();
            while stamps.front().is_some_and(|t| now.duration_since(*t) >= AUCTIONS_WINDOW) {
                stamps.pop_front();
            }
            if stamps.len() < AUCTIONS_PER_MIN {
                stamps.push_back(now);
                return;
            }
            // The window is full (len >= AUCTIONS_PER_MIN > 0), so a stamp
            // exists by construction; sleep until the oldest one ages out.
            match stamps.front().copied() {
                Some(oldest) => AUCTIONS_WINDOW.saturating_sub(now.duration_since(oldest)),
                None => continue,
            }
        };
        thread::sleep(wait);
    }
}

fn auctions_url(weapon_slug: &str) -> String {
    format!("{AUCTIONS_SEARCH_URL}?type=riven&weapon_url_name={weapon_slug}&sort_by=price_asc")
}

/// Parse the v1 response (`payload.auctions[]`). Closed / private / withdrawn
/// auctions are not comps; rows without a usable price are dropped. The API
/// returns up to 500 rows per weapon, so the result is sorted by price and
/// truncated to [`COMPS_LIMIT`] here — the server's `sort_by` is not relied on.
pub fn parse_auctions(body: &serde_json::Value) -> Result<Vec<RivenAuction>> {
    let data = wfm_client::unwrap_envelope(body);
    let Some(arr) = data.get("auctions").and_then(|a| a.as_array()) else {
        bail!("unexpected /auctions shape: no auctions array");
    };
    let mut out = Vec::new();
    for a in arr {
        if a.get("closed").and_then(|v| v.as_bool()).unwrap_or(false)
            || a.get("private").and_then(|v| v.as_bool()).unwrap_or(false)
            || !a.get("visible").and_then(|v| v.as_bool()).unwrap_or(true)
        {
            continue;
        }
        let Some(id) = a.get("id").and_then(|v| v.as_str()).map(String::from) else {
            continue;
        };
        let buyout = a
            .get("buyout_price")
            .and_then(|v| v.as_u64())
            .map(|p| p.min(u32::MAX as u64) as u32);
        let starting = a
            .get("starting_price")
            .and_then(|v| v.as_u64())
            .map(|p| p.min(u32::MAX as u64) as u32);
        let Some(price) = buyout.or(starting) else { continue };
        let item = a.get("item");
        let attributes = item
            .and_then(|i| i.get("attributes"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|attr| {
                        Some(RivenAuctionAttribute {
                            url_name: attr.get("url_name").and_then(|v| v.as_str())?.to_string(),
                            value: attr.get("value").and_then(|v| v.as_f64())?,
                            positive: attr.get("positive").and_then(|v| v.as_bool()).unwrap_or(true),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let owner = a
            .get("owner")
            .and_then(|o| o.get("ingame_name"))
            .and_then(|v| v.as_str())
            .map(String::from);
        let owner_status = a
            .get("owner")
            .and_then(|o| o.get("status"))
            .and_then(|v| v.as_str())
            .map(String::from);
        let u32v = |v: Option<&serde_json::Value>| {
            v.and_then(|x| x.as_u64()).map(|n| n.min(u32::MAX as u64) as u32).unwrap_or(0)
        };
        out.push(RivenAuction {
            id,
            price,
            buyout_price: buyout,
            starting_price: starting.unwrap_or(0),
            top_bid: a.get("top_bid").and_then(|v| v.as_u64()).map(|p| p.min(u32::MAX as u64) as u32),
            is_direct_sell: a.get("is_direct_sell").and_then(|v| v.as_bool()).unwrap_or(false),
            owner,
            owner_status,
            mod_rank: u32v(item.and_then(|i| i.get("mod_rank"))),
            mastery_level: u32v(item.and_then(|i| i.get("mastery_level"))),
            re_rolls: u32v(item.and_then(|i| i.get("re_rolls"))),
            polarity: item.and_then(|i| i.get("polarity")).and_then(|v| v.as_str()).map(String::from),
            name: item.and_then(|i| i.get("name")).and_then(|v| v.as_str()).map(String::from),
            platform: a.get("platform").and_then(|v| v.as_str()).map(String::from),
            attributes,
        });
    }
    out.sort_by_key(|x| x.price);
    out.truncate(COMPS_LIMIT);
    Ok(out)
}

/// The ≤[`COMPS_LIMIT`] cheapest matching auctions for one weapon, straight
/// from WFM's v1 auctions search. Paced through the shared 10/min gate, so
/// the caller can never exceed WFM's auction budget across the whole app.
pub fn fetch_riven_comps(platform: &str, weapon_slug: &str) -> Result<Vec<RivenAuction>> {
    pace_auction_request();
    let client = browser_client(20)?;
    let url = auctions_url(weapon_slug);
    let resp = wfm_client::wfm_headers(client.get(&url), platform)
        .send()
        .with_context(|| format!("GET {url}"))?;
    let status = resp.status();
    if !status.is_success() {
        bail!("{url}: HTTP {status}");
    }
    let body: serde_json::Value = resp.json().with_context(|| format!("{url}: JSON"))?;
    parse_auctions(&body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn url_carries_type_and_weapon() {
        assert_eq!(
            auctions_url("acceltra"),
            "https://api.warframe.market/v1/auctions/search?type=riven&weapon_url_name=acceltra&sort_by=price_asc"
        );
    }

    fn auction(id: &str, price: u32, closed: bool) -> serde_json::Value {
        json!({
            "id": id, "closed": closed, "private": false, "visible": true,
            "buyout_price": price, "starting_price": price, "top_bid": null,
            "is_direct_sell": true, "platform": "pc",
            "owner": {"ingame_name": "Someone", "status": "online"},
            "item": {
                "mod_rank": 0, "mastery_level": 15, "re_rolls": 3,
                "polarity": "madurai", "name": "arma-purado",
                "attributes": [
                    {"value": 28.0, "positive": true, "url_name": "magazine_capacity"},
                    {"value": 47.0, "positive": true, "url_name": "cold_damage"}
                ]
            }
        })
    }

    #[test]
    fn parses_and_sorts_cheapest_first_truncated_to_limit() {
        let mut rows: Vec<serde_json::Value> = (0..30)
            .map(|i| auction(&format!("a{i}"), 100 - i, false))
            .collect();
        // closed + private + withdrawn rows are not comps
        rows.push(auction("closed", 1, true));
        let mut private = auction("priv", 1, false);
        private["private"] = json!(true);
        rows.push(private);
        let mut hidden = auction("hidden", 1, false);
        hidden["visible"] = json!(false);
        rows.push(hidden);
        let body = json!({"payload": {"auctions": rows}});
        let got = parse_auctions(&body).unwrap();
        assert_eq!(got.len(), COMPS_LIMIT, "truncated to the comps limit");
        // cheapest first (prices 100..=71 truncated to the top 20 cheapest)
        assert_eq!(got.first().unwrap().price, 71);
        assert_eq!(got.last().unwrap().price, 90);
        assert!(got.iter().all(|a| a.id != "closed" && a.id != "priv" && a.id != "hidden"));
        assert_eq!(got[0].attributes.len(), 2);
        assert_eq!(got[0].attributes[0].url_name, "magazine_capacity");
        assert_eq!(got[0].re_rolls, 3);
        assert_eq!(got[0].owner.as_deref(), Some("Someone"));
    }

    #[test]
    fn a_row_without_any_price_is_dropped() {
        let body = json!({"payload": {"auctions": [
            auction("priced", 5, false),
            {"id": "noprice", "closed": false, "private": false, "visible": true,
             "item": {"attributes": []}}
        ]}});
        let got = parse_auctions(&body).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, "priced");
    }

    #[test]
    fn rejects_a_body_without_an_auctions_array() {
        let body = json!({"payload": {"nope": true}});
        assert!(parse_auctions(&body).is_err());
    }

    #[test]
    fn the_gate_consumes_burst_slots_up_to_the_cap() {
        // A burst of 10 passes instantly and fills the window; the 11th would
        // sleep, so we assert the shared state caps at the budget instead of
        // blocking the suite for a minute.
        let gate = auction_gate();
        gate.lock().unwrap().clear();
        for _ in 0..AUCTIONS_PER_MIN {
            pace_auction_request();
        }
        assert_eq!(gate.lock().unwrap().len(), AUCTIONS_PER_MIN);
        gate.lock().unwrap().clear();
    }
}
