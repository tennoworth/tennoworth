//! Instant price-watch alerts from WFM's live order stream.
//!
//! The polling checker (watch.rs) stays the reliable path - every 10 minutes
//! it evaluates each watch against the real top-of-book, so state converges
//! even if this stream is down for hours. This thread adds the fast path:
//! `newOrders` pushes every order anyone posts, and a fresh order that
//! satisfies a watch notifies in seconds. Same notification, same re-arm
//! window, same Db bookkeeping as a poll hit - a fire from either path
//! re-arms both.
//!
//! Behaviour under uncertainty is deliberately conservative: anything that
//! can't be resolved (unknown itemId, stale cache, no watches) does nothing,
//! because the poll pass will catch it within 10 minutes anyway.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use wfm_core::catalog::{fetch_wfm_catalog, index_item_meta, ItemMeta};
use wfm_core::util::browser_client;
use wfm_core::ws::{order_matches_watch, run_new_orders_stream, NewOrder};

use crate::db::{Db, Watch};
use crate::watch::{describe, WatchOutcome, EVENT_WATCH_FIRED, REARM_AFTER_SECS};
use crate::wfm_session::WfmSession;

/// With no watches configured there is nothing to stream for; re-check this
/// often. (The rules prefer WS over polling, but an idle firehose for zero
/// watches is still waste.)
const IDLE_RECHECK: Duration = Duration::from_secs(5 * 60);
/// Reconnect backoff bounds after a dropped stream.
const BACKOFF_MIN: Duration = Duration::from_secs(15);
const BACKOFF_MAX: Duration = Duration::from_secs(15 * 60);
/// How stale the in-memory watch list may get before an event re-reads it.
const WATCH_CACHE_TTL: Duration = Duration::from_secs(60);

struct WatchCache {
    watches: Vec<Watch>,
    read_at: Instant,
}

impl WatchCache {
    fn fresh(db: &Db) -> Self {
        WatchCache {
            watches: db.list_watches().unwrap_or_default(),
            read_at: Instant::now(),
        }
    }
    fn refresh_if_stale(&mut self, db: &Db) {
        if self.read_at.elapsed() > WATCH_CACHE_TTL {
            *self = WatchCache::fresh(db);
        }
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The pure decision: which watch (if any) does this streamed order fire?
/// Own orders never fire (listing your own item must not "satisfy" your own
/// watch); re-arming mirrors watch.rs.
pub fn match_order(
    o: &NewOrder,
    slug: &str,
    watches: &[Watch],
    me: Option<&str>,
    now: i64,
) -> Option<usize> {
    if let (Some(me), Some(by)) = (me, o.user_name.as_deref()) {
        if by.eq_ignore_ascii_case(me) {
            return None;
        }
    }
    watches.iter().position(|w| {
        w.slug == slug
            && order_matches_watch(
                o,
                &w.side,
                w.threshold,
                w.rank.map(|r| r.max(0) as u32),
                w.subtype.as_deref(),
            )
            && match w.last_fired_at {
                Some(t) => now - t >= REARM_AFTER_SECS,
                None => true,
            }
    })
}

/// Start the stream thread. Never takes the app down; every failure logs and
/// backs off.
pub fn start_stream(app: AppHandle) {
    std::thread::Builder::new()
        .name("watch-stream".into())
        .spawn(move || run(app))
        .map(|_| ())
        .unwrap_or_else(|e| eprintln!("tennoworth: watch stream thread failed to start: {e}"));
}

fn run(app: AppHandle) {
    let mut backoff = BACKOFF_MIN;
    loop {
        // No watches → no connection. Cheap Db read every 5 min.
        let have_watches = app
            .state::<Db>()
            .list_watches()
            .map(|w| !w.is_empty())
            .unwrap_or(false);
        if !have_watches {
            std::thread::sleep(IDLE_RECHECK);
            continue;
        }

        let (platform, me) = app
            .state::<Arc<WfmSession>>()
            .require_unlocked()
            .map(|u| (u.platform.clone(), Some(u.username.clone())))
            .unwrap_or_else(|_| ("pc".to_string(), None));

        // itemId → {name, slug}, once per connection. Multi-MB, so a failure
        // here backs off like a connection failure rather than retrying hot.
        let id_to_item = match browser_client(60).and_then(|c| fetch_wfm_catalog(&c, &platform)) {
            Ok(cat) => index_item_meta(&cat),
            Err(e) => {
                eprintln!("tennoworth: watch stream: catalog load failed: {e}; retrying in {backoff:?}");
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(BACKOFF_MAX);
                continue;
            }
        };

        let app2 = app.clone();
        let me2 = me.clone();
        let mut cache = WatchCache::fresh(&app.state::<Db>());
        let mut on_order = move |o: NewOrder| {
            let Some(ItemMeta { name: _, slug }) = id_to_item.get(&o.item_id) else {
                return; // item newer than this connection's catalog - poll path covers it
            };
            let db = app2.state::<Db>();
            cache.refresh_if_stale(&db);
            let now = unix_now();
            let Some(i) = match_order(&o, slug, &cache.watches, me2.as_deref(), now) else {
                return;
            };
            let w = &cache.watches[i];
            let outcome = WatchOutcome {
                id: w.id,
                slug: w.slug.clone(),
                name: w.name.clone(),
                side: w.side.clone(),
                threshold: w.threshold,
                price: Some(o.platinum as i64),
                satisfied: true,
                fire: true,
            };
            if let Err(e) = db.record_watch_check(w.id, outcome.price, now, Some(now)) {
                eprintln!("tennoworth: watch stream: record failed for #{}: {e}", w.id);
            }
            let body = describe(&outcome);
            if let Err(e) = app2.notification().builder().title("TennoWorth price watch").body(&body).show() {
                eprintln!("tennoworth: watch stream notification failed: {e}");
            }
            let _ = app2.emit(EVENT_WATCH_FIRED, &outcome);
            eprintln!("tennoworth: watch stream fired #{} ({})", w.id, body);
            // The fire stamped last_fired_at - reload so the re-arm window
            // holds even inside the cache TTL.
            cache = WatchCache::fresh(&db);
        };

        eprintln!("tennoworth: watch stream connecting ({platform})");
        let connected_at = Instant::now();
        match run_new_orders_stream(&platform, &mut on_order, &|| false) {
            Ok(()) => return, // requested stop (never, today)
            Err(e) => {
                // A connection that lived a while earned a fresh backoff; only
                // consecutive fast failures escalate.
                if connected_at.elapsed() > Duration::from_secs(300) {
                    backoff = BACKOFF_MIN;
                }
                eprintln!("tennoworth: watch stream dropped: {e}; reconnecting in {backoff:?}");
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(BACKOFF_MAX);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn watch(id: i64, slug: &str, side: &str, threshold: i64, last_fired_at: Option<i64>) -> Watch {
        Watch {
            id, slug: slug.into(), name: slug.into(), subtype: None, rank: Some(0),
            side: side.into(), threshold, created_at: String::new(),
            last_price: None, last_checked_at: None, last_fired_at,
        }
    }

    fn order(side: &str, plat: u32, by: Option<&str>) -> NewOrder {
        NewOrder {
            id: "o".into(), side: side.into(), platinum: plat, quantity: 1,
            rank: Some(0), subtype: None, item_id: "i".into(),
            user_name: by.map(String::from), user_status: None, visible: true,
        }
    }

    #[test]
    fn fires_the_matching_armed_watch_and_only_that_one() {
        let ws = vec![
            watch(1, "primed_flow", "buy", 60, None),
            watch(2, "primed_flow", "buy", 40, None),
            watch(3, "other_item", "buy", 1, None),
        ];
        // 50p bid: below watch 1's floor, at/above watch 2's.
        assert_eq!(match_order(&order("buy", 50, Some("Stranger")), "primed_flow", &ws, None, 1000), Some(1));
    }

    #[test]
    fn own_orders_and_unarmed_watches_never_fire() {
        let ws = vec![watch(1, "primed_flow", "buy", 40, Some(900))];
        // own order (case-insensitive)
        assert_eq!(match_order(&order("buy", 99, Some("prowly")), "primed_flow", &ws, Some("Prowly"), 1000), None);
        // within the re-arm window
        assert_eq!(match_order(&order("buy", 99, Some("Stranger")), "primed_flow", &ws, None, 900 + REARM_AFTER_SECS - 1), None);
        // re-armed
        assert_eq!(match_order(&order("buy", 99, Some("Stranger")), "primed_flow", &ws, None, 900 + REARM_AFTER_SECS), Some(0));
    }
}
