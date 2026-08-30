//! What happens when EE.log confirms a trade: record it in the ledger, tell
//! the user, and - for a SALE, when a WFM login is unlocked and the setting
//! allows - adjust the matching WFM listing so it doesn't keep advertising a
//! copy that just left the inventory ("sold-detection auto-close").
//!
//! The adjustment is deliberately conservative: it only ever DECREASES a
//! listing's quantity by the number sold (deleting it when that reaches
//! zero), only on `sell` orders whose item name matches an item we gave away
//! in a trade where we received plat, and never touches price or visibility.
//! EE.log does not identify rank or subtype, so ranked mods/arcanes and relic
//! refinements are never adjusted automatically. Untiered items are adjusted
//! only when exactly one matching sell order exists.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use wfm_core::listing::{delete_order, list_user_orders, update_order, Unlocked, UpdateRequest};

use crate::db::Db;
use crate::eelog::{TradeEvent, TradeItem};
use crate::wfm_session::WfmSession;

pub const EVENT_TRADE_DETECTED: &str = "trade-detected";
/// Setting key; "off" disables the WFM adjustment. Anything else (incl.
/// unset) = on - the feature is the point of watching the log.
pub const SETTING_AUTO_CLOSE: &str = "auto-close-sold";

#[derive(Debug, Clone, serde::Serialize)]
pub struct TradeDetected {
    pub id: i64,
    pub trade: TradeEvent,
    /// Listings adjusted: (item name, new quantity or 0 = deleted).
    pub adjusted: Vec<(String, i64)>,
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Display-name → slug over the WFM catalog, lowercase keys. The game prints
/// WFM's own display names ("Primed Flow", "Lith C5 Relic"), so a
/// case-insensitive exact match is the right join; no fuzzy matching, by
/// design (see module doc).
pub fn name_to_slug(unlocked: &Unlocked) -> BTreeMap<String, String> {
    unlocked
        .catalog
        .iter()
        .map(|(slug, c)| (c.display_name.to_lowercase(), slug.clone()))
        .collect()
}

/// Slugs whose WFM order identity includes information EE.log does not carry.
/// Guessing here can decrement a different mod rank or relic refinement than
/// the one the player actually traded, so these are never auto-adjusted.
fn tiered_slugs(unlocked: &Unlocked) -> BTreeSet<String> {
    unlocked
        .catalog
        .iter()
        .filter(|(_, item)| item.max_rank.is_some() || !item.subtypes.is_empty())
        .map(|(slug, _)| slug.clone())
        .collect()
}

/// One of the user's sell orders, as much of it as the adjustment needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnSellOrder {
    pub id: String,
    pub slug: String,
    pub quantity: i64,
}

/// Pull `sell` orders (with slug, thanks to catalog enrichment) out of the
/// `/orders/user` body.
pub fn own_sell_orders(body: &serde_json::Value) -> Vec<OwnSellOrder> {
    let data = body.get("data").unwrap_or(body);
    let arr: Vec<&serde_json::Value> = match data.get("sell").and_then(|v| v.as_array()) {
        Some(a) => a.iter().collect(),
        None => data
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|o| o.get("type").and_then(|t| t.as_str()) == Some("sell"))
                    .collect()
            })
            .unwrap_or_default(),
    };
    arr.into_iter()
        .filter_map(|o| {
            let id = o.get("id")?.as_str()?.to_string();
            let slug = o
                .get("item")
                .and_then(|i| i.get("slug"))
                .and_then(|s| s.as_str())?
                .to_string();
            let quantity = o.get("quantity").and_then(|q| q.as_i64()).unwrap_or(1);
            Some(OwnSellOrder { id, slug, quantity })
        })
        .collect()
}

/// Decide the adjustments for a trade without touching the network: for each
/// item we GAVE in a SALE, the matching own sell order's new quantity
/// (0 = delete). Pure, tested.
pub fn plan_adjustments(
    trade: &TradeEvent,
    names: &BTreeMap<String, String>,
    orders: &[OwnSellOrder],
    tiered: &BTreeSet<String>,
) -> Vec<(OwnSellOrder, i64)> {
    if trade.kind != "sale" {
        return vec![];
    }

    let mut sold_by_slug = BTreeMap::<String, i64>::new();
    for TradeItem {
        name,
        qty,
        direction,
    } in &trade.items
    {
        if direction != "given" {
            continue;
        }
        let Some(slug) = names.get(&name.to_lowercase()) else {
            continue;
        };
        if tiered.contains(slug) {
            continue;
        }
        *sold_by_slug.entry(slug.clone()).or_default() += qty;
    }

    let mut unique_order_by_slug = BTreeMap::<&str, Option<&OwnSellOrder>>::new();
    for order in orders {
        unique_order_by_slug
            .entry(&order.slug)
            .and_modify(|candidate| *candidate = None)
            .or_insert(Some(order));
    }

    sold_by_slug
        .into_iter()
        .filter_map(|(slug, sold)| {
            let order = unique_order_by_slug.get(slug.as_str()).copied().flatten()?;
            Some((order.clone(), (order.quantity - sold).max(0)))
        })
        .collect()
}

/// Record + notify + adjust. Blocking; called from the tailer thread.
pub fn handle_trade(app: &AppHandle, trade: TradeEvent) {
    let db = app.state::<Db>();
    let now = unix_now();
    let id = match db.insert_trade(&trade, now) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("tennoworth: ledger insert failed: {e}");
            return;
        }
    };
    let mut adjusted: Vec<(String, i64)> = Vec::new();

    let auto_close_on = db
        .get_setting(SETTING_AUTO_CLOSE)
        .ok()
        .flatten()
        .map(|v| v != "off")
        .unwrap_or(true);
    if trade.kind == "sale" && auto_close_on {
        let session = app.state::<Arc<WfmSession>>();
        if let Ok(unlocked) = session.require_unlocked() {
            match list_user_orders(&unlocked) {
                Ok(body) => {
                    let orders = own_sell_orders(&body);
                    let names = name_to_slug(&unlocked);
                    let tiered = tiered_slugs(&unlocked);
                    for (order, new_qty) in plan_adjustments(&trade, &names, &orders, &tiered) {
                        let res = if new_qty == 0 {
                            delete_order(&unlocked, &order.id).map(|_| ())
                        } else {
                            update_order(
                                &unlocked,
                                &order.id,
                                &UpdateRequest {
                                    platinum: None,
                                    quantity: Some(new_qty as u32),
                                    visible: None,
                                    rank: None,
                                },
                            )
                            .map(|_| ())
                        };
                        match res {
                            Ok(()) => {
                                let name = trade
                                    .items
                                    .iter()
                                    .find(|i| {
                                        names.get(&i.name.to_lowercase()) == Some(&order.slug)
                                    })
                                    .map(|i| i.name.clone())
                                    .unwrap_or_else(|| order.slug.clone());
                                adjusted.push((name, new_qty));
                            }
                            Err(e) => {
                                eprintln!("tennoworth: auto-close of {} failed: {e}", order.slug)
                            }
                        }
                    }
                    if !adjusted.is_empty() {
                        let _ = db.mark_trade_wfm_closed(id);
                    }
                }
                Err(e) => eprintln!("tennoworth: auto-close: could not list orders: {e}"),
            }
        }
    }

    // Tell the user.
    let title = match trade.kind.as_str() {
        "sale" => format!("Sold for {}p", trade.plat),
        "purchase" => format!("Bought for {}p", trade.plat),
        _ => "Trade completed".to_string(),
    };
    let what: Vec<String> = trade
        .items
        .iter()
        .filter(|i| {
            if trade.kind == "purchase" {
                i.direction == "received"
            } else {
                i.direction == "given"
            }
        })
        .map(|i| {
            if i.qty > 1 {
                format!("{} ×{}", i.name, i.qty)
            } else {
                i.name.clone()
            }
        })
        .collect();
    let mut body = format!("{} - with {}", what.join(", "), trade.partner);
    if !adjusted.is_empty() {
        let n = adjusted.len();
        body.push_str(&format!(
            " · {} WFM listing{} updated",
            n,
            if n == 1 { "" } else { "s" }
        ));
    }
    if let Err(e) = app
        .notification()
        .builder()
        .title(&title)
        .body(&body)
        .show()
    {
        eprintln!("tennoworth: trade notification failed: {e}");
    }
    let _ = app.emit(
        EVENT_TRADE_DETECTED,
        TradeDetected {
            id,
            trade,
            adjusted,
        },
    );
}

/// Start tailing EE.log if it can be found. Silent no-op otherwise (the SPA
/// shows the "not found" state via `eelog_status`).
pub fn start_tailer(app: AppHandle) -> Option<std::path::PathBuf> {
    let path = crate::eelog::locate_log()?;
    let p = path.clone();
    let reward_path = path.clone();
    let reward_app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("eelog-tailer".into())
        .spawn(move || {
            let overlay_app = app.clone();
            crate::eelog::tail_forever_with_lines(
                &p,
                // Reward choices live for only 15 seconds. A two-second poll
                // let the retry hotkey beat the automatic trigger and start a
                // scan before the four slot markers had arrived.
                std::time::Duration::from_millis(250),
                move |line| crate::overlay::handle_log_line(&overlay_app, line),
                |trade| handle_trade(&app, trade),
            );
        });
    match spawned {
        Ok(_) => {
            if let Err(error) = std::thread::Builder::new()
                .name("reward-log-watcher".into())
                .spawn(move || {
                    crate::eelog::watch_recent_text(
                        &reward_path,
                        std::time::Duration::from_millis(250),
                        move |text| crate::overlay::handle_log_snapshot(&reward_app, text),
                    );
                })
            {
                eprintln!("tennoworth: reward log watcher failed to start: {error}");
            }
            Some(path)
        }
        Err(e) => {
            eprintln!("tennoworth: eelog tailer thread failed to start: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wfm_core::catalog::WfmCatalogItem;

    fn names() -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        m.insert("primed flow".into(), "primed_flow".into());
        m.insert("lith c5 relic".into(), "lith_c5_relic".into());
        m.insert("loki prime set".into(), "loki_prime_set".into());
        m.insert("ash prime set".into(), "ash_prime_set".into());
        m
    }
    fn untiered() -> BTreeSet<String> {
        BTreeSet::new()
    }
    fn order(id: &str, slug: &str, q: i64) -> OwnSellOrder {
        OwnSellOrder {
            id: id.into(),
            slug: slug.into(),
            quantity: q,
        }
    }
    fn sale(items: Vec<(&str, i64, &str)>) -> TradeEvent {
        TradeEvent {
            partner: "Buyer".into(),
            kind: "sale".into(),
            plat: 40,
            items: items
                .into_iter()
                .map(|(n, q, d)| TradeItem {
                    name: n.into(),
                    qty: q,
                    direction: d.into(),
                })
                .collect(),
            log_stamp: None,
        }
    }

    #[test]
    fn a_sale_decrements_the_matching_listing_and_deletes_at_zero() {
        let orders = vec![
            order("o1", "loki_prime_set", 3),
            order("o2", "ash_prime_set", 1),
        ];
        let plan = plan_adjustments(
            &sale(vec![
                ("Loki Prime Set", 1, "given"),
                ("Ash Prime Set", 1, "given"),
            ]),
            &names(),
            &orders,
            &untiered(),
        );
        assert_eq!(
            plan,
            vec![
                (order("o2", "ash_prime_set", 1), 0),
                (order("o1", "loki_prime_set", 3), 2),
            ]
        );
    }

    #[test]
    fn purchases_trades_received_items_and_unknown_names_adjust_nothing() {
        let orders = vec![order("o1", "primed_flow", 3)];
        let mut t = sale(vec![("Primed Flow", 1, "given")]);
        t.kind = "purchase".into();
        assert!(plan_adjustments(&t, &names(), &orders, &untiered()).is_empty());
        assert!(plan_adjustments(
            &sale(vec![("Primed Flow", 1, "received")]),
            &names(),
            &orders,
            &untiered(),
        )
        .is_empty());
        assert!(plan_adjustments(
            &sale(vec![("Some Unlisted Thing", 1, "given")]),
            &names(),
            &orders,
            &untiered(),
        )
        .is_empty());
        assert!(plan_adjustments(
            &sale(vec![("Primed Flow", 1, "given")]),
            &names(),
            &[],
            &untiered(),
        )
        .is_empty());
    }

    #[test]
    fn multiple_orders_for_one_slug_are_ambiguous_and_adjust_nothing() {
        let orders = vec![
            order("o1", "loki_prime_set", 1),
            order("o2", "loki_prime_set", 4),
        ];
        let plan = plan_adjustments(
            &sale(vec![("Loki Prime Set", 1, "given")]),
            &names(),
            &orders,
            &untiered(),
        );
        assert!(plan.is_empty());
    }

    #[test]
    fn ranked_and_subtyped_items_are_never_inferred_from_a_name_only_trade() {
        let orders = vec![
            order("rank-0", "primed_flow", 3),
            order("radiant", "lith_c5_relic", 2),
        ];
        let tiered = BTreeSet::from(["primed_flow".into(), "lith_c5_relic".into()]);
        let plan = plan_adjustments(
            &sale(vec![
                ("Primed Flow", 1, "given"),
                ("Lith C5 Relic", 1, "given"),
            ]),
            &names(),
            &orders,
            &tiered,
        );
        assert!(plan.is_empty());
    }

    #[test]
    fn a_tiered_item_does_not_block_an_unambiguous_untiered_adjustment() {
        let orders = vec![
            order("rank-0", "primed_flow", 3),
            order("set", "loki_prime_set", 2),
        ];
        let tiered = BTreeSet::from(["primed_flow".into()]);
        let plan = plan_adjustments(
            &sale(vec![
                ("Primed Flow", 1, "given"),
                ("Loki Prime Set", 1, "given"),
            ]),
            &names(),
            &orders,
            &tiered,
        );
        assert_eq!(plan, vec![(order("set", "loki_prime_set", 2), 1)]);
    }

    #[test]
    fn catalog_metadata_marks_ranked_and_subtyped_items_as_tiered() {
        let mut catalog = BTreeMap::new();
        catalog.insert(
            "primed_flow".into(),
            WfmCatalogItem {
                item_id: "mod".into(),
                display_name: "Primed Flow".into(),
                max_rank: Some(10),
                subtypes: vec![],
            },
        );
        catalog.insert(
            "lith_c5_relic".into(),
            WfmCatalogItem {
                item_id: "relic".into(),
                display_name: "Lith C5 Relic".into(),
                max_rank: None,
                subtypes: vec!["intact".into(), "radiant".into()],
            },
        );
        catalog.insert(
            "loki_prime_set".into(),
            WfmCatalogItem {
                item_id: "set".into(),
                display_name: "Loki Prime Set".into(),
                max_rank: None,
                subtypes: vec![],
            },
        );
        let unlocked = Unlocked {
            jwt: "test".into(),
            username: "test".into(),
            platform: "pc".into(),
            catalog: Arc::new(catalog),
            id_to_item: Arc::new(BTreeMap::new()),
        };
        assert_eq!(
            tiered_slugs(&unlocked),
            BTreeSet::from(["lith_c5_relic".into(), "primed_flow".into()]),
        );
    }

    #[test]
    fn repeated_trade_lines_are_aggregated_before_one_adjustment() {
        let orders = vec![order("o1", "loki_prime_set", 5)];
        let plan = plan_adjustments(
            &sale(vec![
                ("Loki Prime Set", 1, "given"),
                ("Loki Prime Set", 2, "given"),
            ]),
            &names(),
            &orders,
            &untiered(),
        );
        assert_eq!(plan, vec![(order("o1", "loki_prime_set", 5), 2)]);
    }

    #[test]
    fn repeated_lines_do_not_make_ambiguous_orders_safe_to_adjust() {
        let orders = vec![
            order("o1", "loki_prime_set", 2),
            order("o2", "loki_prime_set", 5),
        ];
        let plan = plan_adjustments(
            &sale(vec![
                ("Loki Prime Set", 1, "given"),
                ("loki prime set", 9, "given"),
            ]),
            &names(),
            &orders,
            &untiered(),
        );
        assert!(plan.is_empty());
    }

    #[test]
    fn own_sell_orders_reads_the_v2_split_shape_and_the_flat_shape() {
        let body = serde_json::json!({"data": {"sell": [
            {"id": "a", "quantity": 2, "item": {"slug": "primed_flow"}},
            {"id": "b", "item": {"name": "no slug → skipped"}}
        ], "buy": [{"id": "c", "quantity": 1, "item": {"slug": "x"}}]}});
        assert_eq!(own_sell_orders(&body), vec![order("a", "primed_flow", 2)]);
        let flat = serde_json::json!({"data": [
            {"id": "a", "type": "sell", "quantity": 1, "item": {"slug": "primed_flow"}},
            {"id": "c", "type": "buy", "quantity": 1, "item": {"slug": "x"}}
        ]});
        assert_eq!(own_sell_orders(&flat), vec![order("a", "primed_flow", 1)]);
    }
}
