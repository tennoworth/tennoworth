//! On-demand riven auction comparables from warframe.market's v1 auctions
//! endpoint.
//!
//! The Rivens view shows the user's own rivens; for each one a "Show comps"
//! button asks WFM for the cheapest matching auctions. WFM's API rules cap
//! auction searches at 10 requests/minute. The shared governor enforces
//! conservative spacing between starts, without accumulated burst slots.

use wfm_client::governor::Kind;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::http::browser_client;

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

/// One auction, reduced to what a comps panel shows. `price` is the buyout:
/// a bid-only auction names no price a buyer can pay, so it is not a comp.
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
    /// WFM's own instants for the auction, kept verbatim. `None` when WFM omits
    /// or malforms them: an unknown age must never render as a fresh listing,
    /// and an update is not evidence that the seller is reachable.
    pub created: Option<String>,
    pub updated: Option<String>,
    pub name: Option<String>,
    pub platform: Option<String>,
    pub attributes: Vec<RivenAuctionAttribute>,
}

/// WFM's search form takes at most three positive stats and one negative.
pub const MAX_POSITIVE_STATS: usize = 3;
pub const MAX_NEGATIVE_STATS: usize = 1;

/// Optional stat filter for a comps search. WFM returns only auctions that
/// carry every listed stat with the listed sign (checked against live
/// results), so a filled filter narrows the sample to comparable rolls rather
/// than the whole weapon.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RivenStatFilter {
    #[serde(default)]
    pub positive: Vec<String>,
    #[serde(default)]
    pub negative: Vec<String>,
}

/// Attribute slugs are lowercase words joined by `_`, and one carries a `/`
/// (`base_damage_/_melee_damage`). Anything else is not a WFM attribute.
fn is_stat_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'/')
}

fn stat_list(stats: &[String]) -> String {
    stats.iter().map(|s| s.replace('/', "%2F")).collect::<Vec<_>>().join(",")
}

/// `buyout_policy=with` keeps only auctions that name a buyout. Without it the
/// cheapest rows on any popular weapon are bid auctions opened at 1p, which say
/// nothing about what a riven sells for.
fn auctions_url(weapon_slug: &str, filter: &RivenStatFilter) -> Result<String> {
    if filter.positive.len() > MAX_POSITIVE_STATS || filter.negative.len() > MAX_NEGATIVE_STATS {
        bail!("too many stats in a comps filter");
    }
    if !filter.positive.iter().chain(&filter.negative).all(|s| is_stat_slug(s)) {
        bail!("invalid stat in a comps filter");
    }
    let mut url = format!(
        "{AUCTIONS_SEARCH_URL}?type=riven&weapon_url_name={weapon_slug}&buyout_policy=with&sort_by=price_asc"
    );
    if !filter.positive.is_empty() {
        url.push_str("&positive_stats=");
        url.push_str(&stat_list(&filter.positive));
    }
    if !filter.negative.is_empty() {
        url.push_str("&negative_stats=");
        url.push_str(&stat_list(&filter.negative));
    }
    Ok(url)
}

/// Parse the v1 response (`payload.auctions[]`). Closed / private / withdrawn
/// auctions are not comps; rows without a usable price are dropped. The API
/// returns up to 500 rows per weapon, so the result is sorted by price and
/// truncated to [`COMPS_LIMIT`] here - the server's `sort_by` is not relied on.
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
        let Some(price) = buyout else {
            continue;
        };
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
                            positive: attr
                                .get("positive")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(true),
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
            v.and_then(|x| x.as_u64())
                .map(|n| n.min(u32::MAX as u64) as u32)
                .unwrap_or(0)
        };
        out.push(RivenAuction {
            id,
            price,
            buyout_price: buyout,
            starting_price: starting.unwrap_or(0),
            top_bid: a
                .get("top_bid")
                .and_then(|v| v.as_u64())
                .map(|p| p.min(u32::MAX as u64) as u32),
            is_direct_sell: a
                .get("is_direct_sell")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            owner,
            owner_status,
            mod_rank: u32v(item.and_then(|i| i.get("mod_rank"))),
            mastery_level: u32v(item.and_then(|i| i.get("mastery_level"))),
            re_rolls: u32v(item.and_then(|i| i.get("re_rolls"))),
            polarity: item
                .and_then(|i| i.get("polarity"))
                .and_then(|v| v.as_str())
                .map(String::from),
            created: a.get("created").and_then(|v| v.as_str()).map(String::from),
            updated: a.get("updated").and_then(|v| v.as_str()).map(String::from),
            name: item
                .and_then(|i| i.get("name"))
                .and_then(|v| v.as_str())
                .map(String::from),
            platform: a.get("platform").and_then(|v| v.as_str()).map(String::from),
            attributes,
        });
    }
    out.sort_by_key(|x| x.price);
    out.truncate(COMPS_LIMIT);
    Ok(out)
}

/// The ≤[`COMPS_LIMIT`] cheapest buyouts for one weapon, optionally narrowed
/// to rolls sharing `filter`'s stats, straight from WFM's v1 auctions search,
/// through the shared governor and read cache.
pub fn fetch_riven_comps(
    platform: &str,
    weapon_slug: &str,
    filter: &RivenStatFilter,
) -> Result<Vec<RivenAuction>> {
    let url = auctions_url(weapon_slug, filter)?;
    let client = browser_client(20)?;
    let body = wfm_client::transport::read_json(
        wfm_client::wfm_headers(client.get(&url), platform), Kind::Contract,
        wfm_client::transport::ReadKey { url, platform: platform.into(), account: None },
        std::time::Duration::from_secs(60), false,
    )?;
    parse_auctions(&body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn url_carries_type_weapon_and_buyout_policy() {
        assert_eq!(
            auctions_url("acceltra", &RivenStatFilter::default()).unwrap(),
            "https://api.warframe.market/v1/auctions/search?type=riven&weapon_url_name=acceltra&buyout_policy=with&sort_by=price_asc"
        );
    }

    #[test]
    fn url_carries_the_stat_filter_with_slashes_encoded() {
        let filter = RivenStatFilter {
            positive: vec!["critical_damage".into(), "base_damage_/_melee_damage".into()],
            negative: vec!["zoom".into()],
        };
        assert_eq!(
            auctions_url("rubico", &filter).unwrap(),
            "https://api.warframe.market/v1/auctions/search?type=riven&weapon_url_name=rubico&buyout_policy=with&sort_by=price_asc\
             &positive_stats=critical_damage,base_damage_%2F_melee_damage&negative_stats=zoom"
        );
    }

    #[test]
    fn a_filter_wfm_cannot_express_is_refused_before_any_request() {
        let four = RivenStatFilter {
            positive: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            negative: vec![],
        };
        assert!(auctions_url("rubico", &four).is_err());
        let two_negative = RivenStatFilter {
            positive: vec![],
            negative: vec!["zoom".into(), "recoil".into()],
        };
        assert!(auctions_url("rubico", &two_negative).is_err());
        let injected = RivenStatFilter {
            positive: vec!["zoom&sort_by=price_desc".into()],
            negative: vec![],
        };
        assert!(auctions_url("rubico", &injected).is_err());
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
        assert!(got
            .iter()
            .all(|a| a.id != "closed" && a.id != "priv" && a.id != "hidden"));
        assert_eq!(got[0].attributes.len(), 2);
        assert_eq!(got[0].attributes[0].url_name, "magazine_capacity");
        assert_eq!(got[0].re_rolls, 3);
        assert_eq!(got[0].owner.as_deref(), Some("Someone"));
    }

    #[test]
    fn a_row_without_a_buyout_is_dropped() {
        let mut bid_only = auction("bid-only", 1, false);
        bid_only["buyout_price"] = json!(null);
        bid_only["is_direct_sell"] = json!(false);
        bid_only["top_bid"] = json!(500);
        let body = json!({"payload": {"auctions": [
            auction("priced", 5, false),
            bid_only,
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
    fn auction_instants_are_kept_verbatim_and_unusable_ones_stay_unknown() {
        let mut stamped = auction("dated", 40, false);
        stamped["created"] = json!("2026-09-15T20:38:13.000+00:00");
        stamped["updated"] = json!("2026-09-16T01:02:03.000+00:00");
        let mut wrong_type = auction("wrong", 20, false);
        wrong_type["created"] = json!(42);
        wrong_type["updated"] = json!(null);
        let body = json!({"payload": {"auctions": [
            stamped,
            wrong_type,
            auction("absent", 10, false)
        ]}});

        let got = parse_auctions(&body).unwrap();
        let by_id = |id: &str| got.iter().find(|a| a.id == id).unwrap();
        assert_eq!(
            by_id("dated").created.as_deref(),
            Some("2026-09-15T20:38:13.000+00:00")
        );
        assert_eq!(
            by_id("dated").updated.as_deref(),
            Some("2026-09-16T01:02:03.000+00:00")
        );
        assert_eq!(by_id("wrong").created, None, "42 is not an instant");
        assert_eq!(by_id("wrong").updated, None);
        assert_eq!(by_id("absent").created, None);
        assert_eq!(by_id("absent").updated, None);
    }
}
