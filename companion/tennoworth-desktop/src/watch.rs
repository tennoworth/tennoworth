//! Price watches — "tell me when X drops to ≤ N" / "when someone bids ≥ N".
//!
//! WFM has no native alerts; the gap is filled today by a Discord bot, a
//! browser extension and one Windows-only app. Ours runs in the desktop's
//! background: every [`CHECK_INTERVAL`] it asks WFM's public `top` endpoint
//! for the exact tier of each watch (paced at 3 req/s, ≤100 watches so a full
//! pass is ≤35 s), evaluates, records what it saw, and fires ONE desktop
//! notification per watch per [`REARM_AFTER`] — a watch that stays satisfied
//! re-arms rather than nags. Evaluation is pure ([`evaluate`]) and tested;
//! the loop is the thin shell around it.
//!
//! The user's own orders are excluded from the book (wfm-core does that by
//! username when logged in), so a 'sell' watch can't be satisfied by your own
//! listing.

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use wfm_core::live_top::{fetch_live_tops, LiveTop, LiveTopQuery};

use crate::db::{Db, Watch};
use crate::wfm_session::WfmSession;

/// How often the background pass runs. WFM's rules ask for no tight polling;
/// 10 min × ≤100 watches is well inside "polite".
pub const CHECK_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// First pass after launch — long enough not to compete with startup work.
pub const FIRST_CHECK_DELAY: Duration = Duration::from_secs(45);
/// A satisfied watch notifies again only after this long (seconds).
pub const REARM_AFTER_SECS: i64 = 6 * 60 * 60;
/// Cap on watches per pass — a whole-market sweep is the scraper's job.
pub const MAX_WATCHES: usize = 100;

pub const EVENT_WATCH_FIRED: &str = "watch-fired";

/// One evaluated watch.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct WatchOutcome {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub side: String,
    pub threshold: i64,
    /// The price the watch is judged against (lowest other ask for 'sell',
    /// highest other bid for 'buy'); `None` when the book was empty or the
    /// lookup failed.
    pub price: Option<i64>,
    /// Condition met right now.
    pub satisfied: bool,
    /// Condition met AND not notified within [`REARM_AFTER_SECS`] → notify.
    pub fire: bool,
}

/// Judge one watch against its live top-of-book. `now` (unix seconds) decides
/// re-arming.
pub fn evaluate(w: &Watch, top: Option<&LiveTop>, now: i64) -> WatchOutcome {
    let price = top.filter(|t| t.error.is_none()).and_then(|t| match w.side.as_str() {
        "sell" => t.low_sell,
        "buy" => t.top_buy,
        _ => None,
    }).map(|p| p as i64);
    let satisfied = match (w.side.as_str(), price) {
        ("sell", Some(p)) => p <= w.threshold,
        ("buy", Some(p)) => p >= w.threshold,
        _ => false,
    };
    let armed = match w.last_fired_at {
        Some(t) => now - t >= REARM_AFTER_SECS,
        None => true,
    };
    WatchOutcome {
        id: w.id,
        slug: w.slug.clone(),
        name: w.name.clone(),
        side: w.side.clone(),
        threshold: w.threshold,
        price,
        satisfied,
        fire: satisfied && armed,
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Human line for the notification / event.
pub fn describe(o: &WatchOutcome) -> String {
    match (o.side.as_str(), o.price) {
        ("sell", Some(p)) => format!("{} is listed at {}p (you wanted ≤ {}p)", o.name, p, o.threshold),
        ("buy", Some(p)) => format!("Someone bids {}p for {} (you wanted ≥ {}p)", p, o.name, o.threshold),
        _ => format!("{}: no live data", o.name),
    }
}

/// One full pass: fetch, evaluate, record, notify. Returns every outcome (the
/// "check now" command shows them all; the loop only acts on `fire`).
/// Blocking — call from `spawn_blocking` or the checker thread.
pub fn run_pass(app: &AppHandle) -> Vec<WatchOutcome> {
    let db = app.state::<Db>();
    let session = app.state::<Arc<WfmSession>>();
    let mut watches = match db.list_watches() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("tennoworth: watch pass: list failed: {e}");
            return vec![];
        }
    };
    if watches.is_empty() {
        return vec![];
    }
    if watches.len() > MAX_WATCHES {
        eprintln!("tennoworth: watch pass: {} watches, checking the first {MAX_WATCHES}", watches.len());
        watches.truncate(MAX_WATCHES);
    }
    let (platform, me) = session
        .require_unlocked()
        .map(|u| (u.platform.clone(), Some(u.username.clone())))
        .unwrap_or_else(|_| ("pc".to_string(), None));
    // Dedupe identical tiers so two watches on one item cost one request.
    let mut queries: Vec<LiveTopQuery> = Vec::new();
    for w in &watches {
        let q = LiveTopQuery {
            slug: w.slug.clone(),
            rank: w.rank.map(|r| r.max(0) as u32),
            subtype: w.subtype.clone(),
        };
        if !queries.contains(&q) {
            queries.push(q);
        }
    }
    let tops = match fetch_live_tops(&platform, me.as_deref(), &queries, |_, _| {}) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("tennoworth: watch pass: fetch failed: {e}");
            return vec![];
        }
    };
    let now = unix_now();
    let mut out = Vec::with_capacity(watches.len());
    for w in &watches {
        let top = tops.iter().find(|t| {
            t.slug == w.slug
                && t.rank == w.rank.map(|r| r.max(0) as u32)
                && t.subtype.as_deref().unwrap_or("") == w.subtype.as_deref().unwrap_or("")
        });
        let o = evaluate(w, top, now);
        let fired_at = if o.fire { Some(now) } else { None };
        if let Err(e) = db.record_watch_check(w.id, o.price, now, fired_at) {
            eprintln!("tennoworth: watch pass: record failed for #{}: {e}", w.id);
        }
        if o.fire {
            let body = describe(&o);
            if let Err(e) = app.notification().builder().title("TennoWorth price watch").body(&body).show() {
                eprintln!("tennoworth: watch notification failed: {e}");
            }
            let _ = app.emit(EVENT_WATCH_FIRED, &o);
        }
        out.push(o);
    }
    out
}

/// Start the background checker. Detached thread: sleeps, runs a pass, sleeps.
/// Nothing here can take the app down — every failure is logged and the loop
/// simply tries again next interval.
pub fn start_checker(app: AppHandle) {
    std::thread::Builder::new()
        .name("watch-checker".into())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK_DELAY);
            loop {
                let fired = run_pass(&app).into_iter().filter(|o| o.fire).count();
                if fired > 0 {
                    eprintln!("tennoworth: watch pass fired {fired}");
                }
                std::thread::sleep(CHECK_INTERVAL);
            }
        })
        .map(|_| ())
        .unwrap_or_else(|e| eprintln!("tennoworth: watch checker thread failed to start: {e}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn watch(side: &str, threshold: i64, last_fired_at: Option<i64>) -> Watch {
        Watch {
            id: 1, slug: "primed_flow".into(), name: "Primed Flow".into(), subtype: None, rank: Some(0),
            side: side.into(), threshold, created_at: "2026-08-16T00:00:00Z".into(),
            last_price: None, last_checked_at: None, last_fired_at,
        }
    }
    fn top(low_sell: Option<u32>, top_buy: Option<u32>) -> LiveTop {
        LiveTop {
            slug: "primed_flow".into(), rank: Some(0), subtype: None,
            sells: low_sell.into_iter().collect(), buys: top_buy.into_iter().collect(),
            low_sell, top_buy, own_ask: None, own_bid: None, error: None,
        }
    }
    const NOON: i64 = 1_786_881_600; // 2026-08-16T12:00:00Z

    #[test]
    fn sell_watch_fires_when_the_lowest_ask_is_at_or_below_threshold() {
        let now = NOON;
        let o = evaluate(&watch("sell", 15, None), Some(&top(Some(15), Some(9))), now);
        assert!(o.satisfied && o.fire);
        assert_eq!(o.price, Some(15));
        let o = evaluate(&watch("sell", 15, None), Some(&top(Some(16), Some(9))), now);
        assert!(!o.satisfied && !o.fire);
    }

    #[test]
    fn buy_watch_fires_when_the_highest_bid_is_at_or_above_threshold() {
        let now = NOON;
        let o = evaluate(&watch("buy", 20, None), Some(&top(Some(30), Some(21))), now);
        assert!(o.fire);
        assert_eq!(o.price, Some(21));
        assert!(!evaluate(&watch("buy", 20, None), Some(&top(Some(30), Some(19))), now).satisfied);
    }

    #[test]
    fn empty_book_or_failed_lookup_never_fires() {
        let now = NOON;
        assert!(!evaluate(&watch("sell", 15, None), Some(&top(None, None)), now).satisfied);
        assert!(!evaluate(&watch("sell", 15, None), None, now).satisfied);
        let mut t = top(Some(1), None);
        t.error = Some("HTTP 503".into());
        let o = evaluate(&watch("sell", 15, None), Some(&t), now);
        assert!(!o.satisfied);
        assert_eq!(o.price, None);
    }

    #[test]
    fn a_recently_fired_watch_stays_satisfied_but_does_not_fire_again_until_rearmed() {
        let now = NOON;
        let recent = evaluate(&watch("sell", 15, Some(NOON - 2 * 3600)), Some(&top(Some(12), None)), now);
        assert!(recent.satisfied && !recent.fire);
        let old = evaluate(&watch("sell", 15, Some(NOON - REARM_AFTER_SECS - 60)), Some(&top(Some(12), None)), now);
        assert!(old.satisfied && old.fire);
    }

    #[test]
    fn describe_reads_like_a_notification() {
        let o = WatchOutcome { id: 1, slug: "s".into(), name: "Primed Flow".into(), side: "sell".into(), threshold: 15, price: Some(12), satisfied: true, fire: true };
        assert_eq!(describe(&o), "Primed Flow is listed at 12p (you wanted ≤ 15p)");
        let o = WatchOutcome { side: "buy".into(), threshold: 20, price: Some(22), ..o };
        assert_eq!(describe(&o), "Someone bids 22p for Primed Flow (you wanted ≥ 20p)");
    }
}
