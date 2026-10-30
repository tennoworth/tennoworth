//! How listings made through TennoWorth did, from this device's own records.
//!
//! A batch's sell listings are recorded when it finishes; what sold is derived
//! from the trade ledger each time the summary is read, the same way price
//! reports read the ledger rather than copying it. Nothing leaves the device.
//!
//! The ledger only knows trades the game logged while TennoWorth was running,
//! and names items without rank or refinement, so a sale is matched to a
//! listing by item alone and the summary calls its counts observed. A listing
//! with no matching sale stays in the denominator: an unsold listing is the
//! outcome this exists to show, not missing data.

use std::collections::BTreeMap;

use serde::Serialize;
use wfm_trading::orders::plan::{ItemResult, PlanItem};

use crate::persistence::{Db, ListedItem, ListingOrigin, TradeRow};

/// Setting key; "off" stops recording and has already deleted the record.
pub const SETTING: &str = "listing-outcomes";
pub const WINDOW_DAYS: u32 = 30;
const DAY: i64 = 86_400;

pub fn enabled(db: &Db) -> bool {
    db.get_setting(SETTING)
        .ok()
        .flatten()
        .is_none_or(|value| value != "off")
}

/// The sell listings a finished batch confirmed. Results are paired with the
/// reviewed items by position, so a response that does not line up item for
/// item (an early refusal reports one result for the whole batch) records
/// nothing rather than attaching a status to the wrong item.
pub fn listed_items(
    plan_id: &str,
    items: &[PlanItem],
    results: &[ItemResult],
    now: i64,
) -> Vec<ListedItem> {
    if plan_id.is_empty()
        || items.len() != results.len()
        || items.iter().zip(results).any(|(item, result)| item.slug != result.slug)
    {
        return Vec::new();
    }
    items
        .iter()
        .zip(results)
        .enumerate()
        .filter(|(_, (item, result))| item.order_type == "sell" && result.status == "ok")
        .map(|(index, (item, result))| ListedItem {
            plan_id: plan_id.to_owned(),
            plan_index: i64::try_from(index).unwrap_or(i64::MAX),
            slug: item.slug.clone(),
            rank: item.rank.map(i64::from),
            subtype: item.subtype.clone(),
            platinum: i64::from(item.platinum),
            quantity: i64::from(item.quantity),
            origin: if item.session.is_some() {
                ListingOrigin::Session
            } else {
                ListingOrigin::Manual
            },
            listed_at: now,
            order_id: result.order_id.clone(),
        })
        .collect()
}

/// Best-effort: the batch already happened, and its result must not change
/// because this record could not be written.
pub fn record(db: &Db, plan_id: &str, items: &[PlanItem], results: &[ItemResult], now: i64) {
    if !enabled(db) {
        return;
    }
    let listed = listed_items(plan_id, items, results, now);
    if listed.is_empty() {
        return;
    }
    if let Err(error) = db.record_listings(&listed, now) {
        eprintln!("listing outcomes: batch not recorded: {error}");
    }
}

/// Listings old enough to be judged against a deadline, and how many of them
/// had a sale before it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, ts_rs::TS)]
pub struct WithinDeadline {
    pub eligible: u32,
    pub sold: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, ts_rs::TS)]
pub struct OutcomeGroup {
    pub listings: u32,
    pub quantity_listed: i64,
    pub quantity_sold: i64,
    pub within_day: WithinDeadline,
    pub within_week: WithinDeadline,
    /// Over listings with a sale. Absent when none sold.
    pub median_hours_to_first_sale: Option<f64>,
    /// Realised unit price over listed price, on sold copies whose trade had
    /// one unit price (plat for one kind of item). Absent when none did.
    pub price_ratio: Option<f64>,
    pub priced_quantity: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, ts_rs::TS)]
pub struct ListingOutcomes {
    pub enabled: bool,
    pub window_days: u32,
    pub session: OutcomeGroup,
    pub manual: OutcomeGroup,
}

/// One market order across every batch that touched it. WFM's update sets an
/// order's quantity, so a later row for the same order replaces what is on
/// offer; it does not add copies.
struct Order<'a> {
    first: &'a ListedItem,
    /// Copies offered over the order's life: those sold before each update
    /// plus the quantity the update left on the market.
    offered: i64,
    remaining: i64,
    platinum: i64,
    sold: i64,
    first_sale_at: Option<i64>,
    priced: i64,
    realised_value: f64,
    listed_value: f64,
}

enum Step<'a> {
    Listed(usize, &'a ListedItem),
    Sold(&'a TradeRow),
}

impl Step<'_> {
    /// A batch and a sale in the same second: the listing came first, or the
    /// sale could not have been of it.
    fn key(&self) -> (i64, u8) {
        match self {
            Step::Listed(_, row) => (row.listed_at, 0),
            Step::Sold(trade) => (trade.at, 1),
        }
    }
}

/// `listings` is the whole retained record, oldest first, so a sale is
/// allocated against every order it could belong to; only orders first listed
/// inside the window are reported. Narrowing the record before allocating
/// would hand an older order's sales to a newer one. `resolve` maps a ledger
/// item name to its slug; an unresolved name matches no listing.
pub fn summarize(
    enabled: bool,
    listings: &[ListedItem],
    sales: &[TradeRow],
    resolve: &dyn Fn(&str) -> Option<String>,
    now: i64,
) -> ListingOutcomes {
    let mut orders: Vec<Order> = Vec::new();
    let mut order_of: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_slug: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    let mut steps: Vec<Step> = Vec::new();
    for row in listings {
        let key = row
            .order_id
            .clone()
            .unwrap_or_else(|| format!("{}#{}", row.plan_id, row.plan_index));
        let index = *order_of.entry(key).or_insert_with(|| {
            by_slug.entry(row.slug.as_str()).or_default().push(orders.len());
            orders.push(Order {
                first: row,
                offered: 0,
                remaining: 0,
                platinum: row.platinum,
                sold: 0,
                first_sale_at: None,
                priced: 0,
                realised_value: 0.0,
                listed_value: 0.0,
            });
            orders.len() - 1
        });
        steps.push(Step::Listed(index, row));
    }
    steps.extend(sales.iter().filter(|trade| trade.kind == "sale").map(Step::Sold));
    steps.sort_by_key(Step::key);

    for step in steps {
        let sale = match step {
            Step::Listed(index, row) => {
                if let Some(order) = orders.get_mut(index) {
                    let quantity = row.quantity.max(0);
                    order.offered = order.sold + quantity;
                    order.remaining = quantity;
                    order.platinum = row.platinum;
                }
                continue;
            }
            Step::Sold(sale) => sale,
        };
        let given: Vec<_> = sale
            .items
            .iter()
            .filter(|item| item.direction == "given" && item.qty > 0)
            .collect();
        let one_kind = given.first().is_some_and(|first| {
            sale.items.iter().all(|item| item.direction == "given")
                && given.iter().all(|item| item.name.trim().eq_ignore_ascii_case(first.name.trim()))
        });
        let total: i64 = given.iter().map(|item| item.qty).sum();
        let unit = (one_kind && total > 0 && sale.plat > 0).then(|| sale.plat as f64 / total as f64);

        for item in given {
            let Some(slug) = resolve(item.name.trim()) else {
                continue;
            };
            let Some(candidates) = by_slug.get(slug.as_str()) else {
                continue;
            };
            let mut left = item.qty;
            // Oldest order first; one not yet listed has nothing remaining.
            for &index in candidates {
                if left == 0 {
                    break;
                }
                let Some(order) = orders.get_mut(index) else {
                    continue;
                };
                if order.remaining == 0 {
                    continue;
                }
                let take = left.min(order.remaining);
                left -= take;
                order.remaining -= take;
                order.sold += take;
                order.first_sale_at.get_or_insert(sale.at);
                if let Some(unit) = unit {
                    order.priced += take;
                    order.realised_value += unit * take as f64;
                    order.listed_value += order.platinum as f64 * take as f64;
                }
            }
        }
    }

    let since = now - i64::from(WINDOW_DAYS) * DAY;
    let mut session = Group::default();
    let mut manual = Group::default();
    for order in orders.iter().filter(|order| order.first.listed_at >= since) {
        let group = match order.first.origin {
            ListingOrigin::Session => &mut session,
            ListingOrigin::Manual => &mut manual,
        };
        group.add(order, now);
    }
    ListingOutcomes {
        enabled,
        window_days: WINDOW_DAYS,
        session: session.finish(),
        manual: manual.finish(),
    }
}

#[derive(Default)]
struct Group {
    out: OutcomeGroup,
    hours: Vec<f64>,
    realised: f64,
    listed: f64,
}

impl Group {
    fn add(&mut self, order: &Order, now: i64) {
        let listed_at = order.first.listed_at;
        self.out.listings += 1;
        self.out.quantity_listed += order.offered;
        self.out.quantity_sold += order.sold;
        self.out.priced_quantity += order.priced;
        self.realised += order.realised_value;
        self.listed += order.listed_value;
        let waited = order.first_sale_at.map(|at| at - listed_at);
        if let Some(waited) = waited {
            self.hours.push(waited as f64 / 3600.0);
        }
        for (deadline, slot) in [(DAY, &mut self.out.within_day), (7 * DAY, &mut self.out.within_week)] {
            let sold_in_time = waited.is_some_and(|waited| waited <= deadline);
            // A listing younger than the deadline can be judged only if it
            // already sold; otherwise its outcome is not known yet.
            if now - listed_at >= deadline || sold_in_time {
                slot.eligible += 1;
                if sold_in_time {
                    slot.sold += 1;
                }
            }
        }
    }

    fn finish(mut self) -> OutcomeGroup {
        self.hours.sort_by(f64::total_cmp);
        self.out.median_hours_to_first_sale = median(&self.hours);
        self.out.price_ratio = (self.listed > 0.0).then(|| self.realised / self.listed);
        self.out
    }
}

fn median(sorted: &[f64]) -> Option<f64> {
    let mid = sorted.len() / 2;
    match sorted.len() {
        0 => None,
        n if n % 2 == 1 => sorted.get(mid).copied(),
        _ => Some((sorted.get(mid - 1)? + sorted.get(mid)?) / 2.0),
    }
}

/// The summary for the Ledger, over the last [`WINDOW_DAYS`].
pub fn read(
    db: &Db,
    resolve: &dyn Fn(&str) -> Option<String>,
    now: i64,
) -> rusqlite::Result<ListingOutcomes> {
    // The whole retained record, not just the window: see `summarize`.
    let listings = db.listings_since(now - crate::persistence::LISTING_RETENTION_SECS)?;
    let sales = match listings.first() {
        Some(oldest) => db.sales_since(oldest.listed_at)?,
        None => Vec::new(),
    };
    Ok(summarize(enabled(db), &listings, &sales, resolve, now))
}

/// Turning the record off deletes it; turning it on starts empty.
pub fn set_enabled(db: &Db, on: bool) -> rusqlite::Result<()> {
    db.set_setting(SETTING, if on { "on" } else { "off" })?;
    if !on {
        db.clear_listings()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::trading::TradeItem;

    const NOW: i64 = 1_800_000_000;
    const HOUR: i64 = 3600;

    fn listing(index: i64, slug: &str, qty: i64, plat: i64, at: i64, origin: ListingOrigin) -> ListedItem {
        ListedItem {
            plan_id: "plan".into(),
            plan_index: index,
            slug: slug.into(),
            rank: None,
            subtype: None,
            platinum: plat,
            quantity: qty,
            origin,
            listed_at: at,
            order_id: None,
        }
    }

    fn on_order(mut row: ListedItem, plan: &str, order: &str) -> ListedItem {
        row.plan_id = plan.into();
        row.order_id = Some(order.into());
        row
    }

    #[test]
    fn resubmitting_an_order_does_not_count_its_copies_twice() {
        let listed = [
            on_order(listing(0, "ash_prime_systems", 10, 20, NOW - 3 * DAY, ListingOrigin::Manual), "a", "order-1"),
            on_order(listing(0, "ash_prime_systems", 10, 20, NOW - 2 * DAY, ListingOrigin::Manual), "b", "order-1"),
        ];
        let out = summarize(true, &listed, &[], &resolve, NOW).manual;
        assert_eq!((out.listings, out.quantity_listed), (1, 10));
    }

    #[test]
    fn an_update_after_a_sale_offers_what_was_sold_plus_what_remains() {
        let listed = [
            on_order(listing(0, "ash_prime_systems", 10, 20, NOW - 9 * DAY, ListingOrigin::Manual), "a", "order-1"),
            // Re-listed the seven left at a new price.
            on_order(listing(0, "ash_prime_systems", 7, 25, NOW - 8 * DAY, ListingOrigin::Manual), "b", "order-1"),
        ];
        let sales = [
            sale(1, NOW - 9 * DAY + HOUR, 60, &[("Ash Prime Systems", 3, "given")]),
            sale(2, NOW - 7 * DAY, 175, &[("Ash Prime Systems", 7, "given")]),
        ];
        let out = summarize(true, &listed, &sales, &resolve, NOW).manual;
        assert_eq!((out.listings, out.quantity_listed, out.quantity_sold), (1, 10, 10));
        // 3 at 20p then 7 at 25p, each against the price listed at the time.
        assert_eq!(out.price_ratio, Some(1.0));
        assert_eq!(out.median_hours_to_first_sale, Some(1.0));
    }

    #[test]
    fn an_order_older_than_the_window_keeps_its_own_sales() {
        let listed = [
            listing(0, "ash_prime_systems", 1, 20, NOW - 40 * DAY, ListingOrigin::Session),
            listing(1, "ash_prime_systems", 1, 30, NOW - 5 * DAY, ListingOrigin::Manual),
        ];
        let sales = [sale(1, NOW - DAY, 20, &[("Ash Prime Systems", 1, "given")])];
        let out = summarize(true, &listed, &sales, &resolve, NOW);
        assert_eq!(out.session.listings, 0, "outside the window, not reported");
        assert_eq!((out.manual.listings, out.manual.quantity_sold), (1, 0));
        assert_eq!(out.manual.price_ratio, None);
    }

    #[test]
    fn the_summary_reads_the_retained_record_not_just_the_window() {
        let db = Db::open_in_memory().expect("db");
        let item = [plan_item("ash_prime_systems", "sell", false)];
        let ok = [result("ash_prime_systems", "ok")];
        record(&db, "old", &item, &ok, NOW - 40 * DAY);
        record(&db, "new", &item, &ok, NOW - 5 * DAY);
        let trade = crate::services::game_log::eelog::TradeEvent {
            partner: "Buyer".into(),
            kind: "sale".into(),
            plat: 40,
            items: vec![TradeItem { name: "Ash Prime Systems".into(), qty: 2, direction: "given".into() }],
            log_stamp: None,
        };
        let position = crate::services::game_log::eelog::LogPosition {
            session: "s".into(),
            start: 0,
            end: 1,
            observed_after: NOW - DAY,
        };
        db.insert_trade(&trade, NOW - DAY, &position).expect("trade");
        // plan_item lists two copies: the 40-day-old order takes both.
        let out = read(&db, &resolve, NOW).expect("summary").manual;
        assert_eq!((out.listings, out.quantity_sold), (1, 0));
    }

    fn sale(id: i64, at: i64, plat: i64, items: &[(&str, i64, &str)]) -> TradeRow {
        TradeRow {
            id,
            at,
            partner: "Buyer".into(),
            kind: "sale".into(),
            plat,
            items: items
                .iter()
                .map(|(name, qty, direction)| TradeItem {
                    name: (*name).into(),
                    qty: *qty,
                    direction: (*direction).into(),
                })
                .collect(),
            log_stamp: None,
            wfm_closed: false,
        }
    }

    fn resolve(name: &str) -> Option<String> {
        match name.to_lowercase().as_str() {
            "ash prime systems" => Some("ash_prime_systems".into()),
            "nova prime set" => Some("nova_prime_set".into()),
            _ => None,
        }
    }

    #[test]
    fn a_priced_sale_counts_quantity_price_and_time() {
        let listed = [listing(0, "ash_prime_systems", 3, 20, NOW - 10 * DAY, ListingOrigin::Manual)];
        let sales = [sale(1, NOW - 10 * DAY + 6 * HOUR, 40, &[("Ash Prime Systems", 2, "given")])];
        let out = summarize(true, &listed, &sales, &resolve, NOW).manual;
        assert_eq!((out.listings, out.quantity_listed, out.quantity_sold), (1, 3, 2));
        assert_eq!(out.within_day, WithinDeadline { eligible: 1, sold: 1 });
        assert_eq!(out.median_hours_to_first_sale, Some(6.0));
        assert_eq!(out.price_ratio, Some(1.0));
        assert_eq!(out.priced_quantity, 2);
    }

    #[test]
    fn a_mixed_trade_counts_quantity_but_not_price() {
        let listed = [listing(0, "ash_prime_systems", 1, 20, NOW - 2 * DAY, ListingOrigin::Manual)];
        let sales = [sale(
            1,
            NOW - DAY,
            60,
            &[("Ash Prime Systems", 1, "given"), ("Nova Prime Set", 1, "given")],
        )];
        let out = summarize(true, &listed, &sales, &resolve, NOW).manual;
        assert_eq!(out.quantity_sold, 1);
        assert_eq!(out.priced_quantity, 0);
        assert_eq!(out.price_ratio, None);
    }

    #[test]
    fn a_sale_before_the_listing_is_not_its_outcome() {
        let listed = [listing(0, "ash_prime_systems", 1, 20, NOW - DAY, ListingOrigin::Manual)];
        let sales = [sale(1, NOW - 2 * DAY, 20, &[("Ash Prime Systems", 1, "given")])];
        let out = summarize(true, &listed, &sales, &resolve, NOW).manual;
        assert_eq!(out.quantity_sold, 0);
        assert_eq!(out.median_hours_to_first_sale, None);
    }

    #[test]
    fn sales_fill_the_oldest_listing_first_and_never_exceed_it() {
        let listed = [
            listing(0, "ash_prime_systems", 1, 20, NOW - 9 * DAY, ListingOrigin::Session),
            listing(1, "ash_prime_systems", 1, 25, NOW - 8 * DAY, ListingOrigin::Manual),
        ];
        let sales = [sale(1, NOW - 7 * DAY, 60, &[("Ash Prime Systems", 3, "given")])];
        let out = summarize(true, &listed, &sales, &resolve, NOW);
        assert_eq!(out.session.quantity_sold, 1);
        assert_eq!(out.manual.quantity_sold, 1);
        assert_eq!(out.session.price_ratio, Some(1.0));
        assert_eq!(out.manual.price_ratio, Some(0.8));
    }

    #[test]
    fn unsold_listings_stay_in_the_denominator_once_they_can_be_judged() {
        let listed = [
            listing(0, "ash_prime_systems", 1, 20, NOW - 8 * DAY, ListingOrigin::Manual),
            listing(1, "nova_prime_set", 1, 90, NOW - 2 * HOUR, ListingOrigin::Manual),
        ];
        let out = summarize(true, &listed, &[], &resolve, NOW).manual;
        assert_eq!(out.listings, 2);
        assert_eq!(out.quantity_sold, 0);
        assert_eq!(out.within_day, WithinDeadline { eligible: 1, sold: 0 });
        assert_eq!(out.within_week, WithinDeadline { eligible: 1, sold: 0 });
    }

    #[test]
    fn an_unresolved_name_or_a_purchase_matches_nothing() {
        let listed = [listing(0, "ash_prime_systems", 1, 20, NOW - DAY, ListingOrigin::Manual)];
        let mut bought = sale(2, NOW - HOUR, 20, &[("Ash Prime Systems", 1, "received")]);
        bought.kind = "purchase".into();
        let sales = [sale(1, NOW - HOUR, 20, &[("Unknown Thing", 1, "given")]), bought];
        assert_eq!(summarize(true, &listed, &sales, &resolve, NOW).manual.quantity_sold, 0);
    }

    fn plan_item(slug: &str, order_type: &str, session: bool) -> PlanItem {
        serde_json::from_value(serde_json::json!({
            "slug": slug, "platinum": 20, "quantity": 2, "order_type": order_type,
            "visible": true, "rank": null,
            "session": if session { serde_json::json!({"snapshot_id": 1, "utc_day": 20_000, "budget": 5}) } else { serde_json::Value::Null },
        }))
        .expect("plan item")
    }

    fn result(slug: &str, status: &str) -> ItemResult {
        ItemResult {
            slug: slug.into(),
            status: status.into(),
            message: None,
            order_id: None,
            action: None,
        }
    }

    #[test]
    fn only_confirmed_sell_listings_are_recorded() {
        let items = [
            plan_item("a", "sell", false),
            plan_item("b", "sell", false),
            plan_item("c", "buy", false),
        ];
        let results = [result("a", "ok"), result("b", "error"), result("c", "ok")];
        let listed = listed_items("plan", &items, &results, NOW);
        assert_eq!(listed.len(), 1);
        assert_eq!((listed[0].slug.as_str(), listed[0].plan_index), ("a", 0));
        assert_eq!(listed[0].origin, ListingOrigin::Manual);
    }

    #[test]
    fn a_response_that_does_not_line_up_records_nothing() {
        let items = [plan_item("a", "sell", false), plan_item("b", "sell", false)];
        assert!(listed_items("plan", &items, &[result("", "error")], NOW).is_empty());
        assert!(listed_items("plan", &items, &[result("b", "ok"), result("a", "ok")], NOW).is_empty());
    }

    #[test]
    fn recording_is_idempotent_and_off_deletes_it() {
        let db = Db::open_in_memory().expect("db");
        let items = [plan_item("a", "sell", false)];
        let results = [result("a", "ok")];
        record(&db, "plan", &items, &results, NOW);
        record(&db, "plan", &items, &results, NOW + 60);
        assert_eq!(db.listings_since(0).expect("read").len(), 1);

        set_enabled(&db, false).expect("off");
        assert!(db.listings_since(0).expect("read").is_empty());
        record(&db, "plan-2", &items, &results, NOW);
        assert!(db.listings_since(0).expect("read").is_empty());
        assert!(!read(&db, &resolve, NOW).expect("summary").enabled);
    }

    #[test]
    fn old_listings_are_pruned_on_write() {
        let db = Db::open_in_memory().expect("db");
        let results = [result("a", "ok")];
        let items = [plan_item("a", "sell", false)];
        record(&db, "old", &items, &results, NOW - crate::persistence::LISTING_RETENTION_SECS - 1);
        record(&db, "new", &items, &results, NOW);
        let kept = db.listings_since(0).expect("read");
        assert_eq!(kept.iter().map(|l| l.plan_id.as_str()).collect::<Vec<_>>(), ["new"]);
    }
}
