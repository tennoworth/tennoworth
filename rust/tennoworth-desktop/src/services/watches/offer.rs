//! What a fired price watch hands the player: who to whisper, and the whisper.
//!
//! The latest offer is kept for the app's banner, so a player who sees the
//! in-game card and switches over finds it ready to copy. It costs no extra
//! warframe.market traffic: the stream and the poll already carry the name.
//! Opening the app allows one recheck of that one item, triggered by the
//! player, never a background loop.

use std::sync::{Arc, Mutex};

use crate::services::wfm::session::WfmSession;
use tauri::{AppHandle, Emitter, Manager, State};
use wfm_trading::market::live_top::{fetch_live_tops, LiveTopQuery};

use crate::persistence::Watch;
use crate::services::unix_now;

pub const EVENT_WATCH_OFFER: &str = "watch-offer-changed";
/// After this long an offer is more likely gone than not; the banner drops it.
pub const OFFER_FRESH_SECS: i64 = 15 * 60;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
pub struct WatchOffer {
    pub watch_id: i64,
    pub slug: String,
    pub name: String,
    /// The watch's side: 'sell' watches asks (the player buys), 'buy' watches
    /// bids (the player sells).
    pub side: String,
    pub threshold: i64,
    pub price: i64,
    pub rank: Option<i64>,
    pub subtype: Option<String>,
    /// The other player's in-game name.
    pub player: String,
    pub whisper: String,
    /// Unix seconds when the order was seen.
    pub seen_at: i64,
    /// Unix seconds of the one recheck, once it has run.
    pub rechecked_at: Option<i64>,
    /// The recheck's answer: does an order at or past the threshold remain?
    pub still_listed: Option<bool>,
}

#[derive(Default)]
pub struct OfferState(Mutex<Option<WatchOffer>>);

impl OfferState {
    fn get(&self) -> Option<WatchOffer> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set(&self, offer: Option<WatchOffer>) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = offer;
    }
}

/// warframe.market in-game names are letters, digits, `-`, `_` and `.`. A name
/// outside that is not whispered: the text goes straight into game chat, so
/// nothing that could read as another command may reach it.
fn is_whisperable(player: &str) -> bool {
    (1..=32).contains(&player.len())
        && player
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// The whisper warframe.market's own copy button produces, unchanged, with this
/// app's name after it: its terms (section 11) require a copy/paste message
/// "in the same format as seen on the Site" that includes the application
/// name. `side` is the watch's side.
pub(crate) fn whisper(
    side: &str,
    player: &str,
    item: &str,
    rank: Option<i64>,
    subtype: Option<&str>,
    price: i64,
) -> Option<String> {
    if !is_whisperable(player) || price <= 0 {
        return None;
    }
    let verb = match side {
        "sell" => "buy",
        "buy" => "sell",
        _ => return None,
    };
    let item: String = item
        .chars()
        .filter(|c| !c.is_control() && *c != '"')
        .collect();
    let label = match (rank, subtype.filter(|s| !s.is_empty())) {
        (Some(rank), _) => format!("{item} (rank {rank})"),
        (None, Some(subtype)) => format!("{item} ({subtype})"),
        (None, None) => item,
    };
    Some(format!(
        "/w {player} Hi! I want to {verb}: \"{label}\" for {price} platinum. (warframe.market) - TennoWorth"
    ))
}

/// The offer a fired watch would hand over, when the order names its player.
pub(crate) fn offer_for(
    watch: &Watch,
    price: i64,
    player: &str,
    seen_at: i64,
) -> Option<WatchOffer> {
    let whisper = whisper(
        &watch.side,
        player,
        &watch.name,
        watch.rank,
        watch.subtype.as_deref(),
        price,
    )?;
    Some(WatchOffer {
        watch_id: watch.id,
        slug: watch.slug.clone(),
        name: watch.name.clone(),
        side: watch.side.clone(),
        threshold: watch.threshold,
        price,
        rank: watch.rank,
        subtype: watch.subtype.clone(),
        player: player.into(),
        whisper,
        seen_at,
        rechecked_at: None,
        still_listed: None,
    })
}

pub(crate) fn remember(app: &AppHandle, offer: WatchOffer) {
    if let Some(state) = app.try_state::<OfferState>() {
        state.set(Some(offer));
        let _ = app.emit(EVENT_WATCH_OFFER, ());
    }
}

fn fresh(offer: Option<WatchOffer>, now: i64) -> Option<WatchOffer> {
    offer.filter(|o| now.saturating_sub(o.seen_at) < OFFER_FRESH_SECS)
}

#[tauri::command]
pub fn latest_watch_offer(state: State<'_, OfferState>) -> Option<WatchOffer> {
    fresh(state.get(), unix_now())
}

#[tauri::command]
pub fn dismiss_watch_offer(app: AppHandle, state: State<'_, OfferState>) {
    state.set(None);
    let _ = app.emit(EVENT_WATCH_OFFER, ());
}

/// Copy the offer's whisper from Rust: the webview's clipboard API is not
/// available in every desktop webview.
#[tauri::command]
pub fn copy_watch_whisper(state: State<'_, OfferState>) -> Result<(), String> {
    let offer = fresh(state.get(), unix_now()).ok_or("This offer has expired.")?;
    crate::services::clipboard::copy(&offer.whisper)
        .map_err(|_| "Could not copy the whisper. Copy it from the text instead.".to_string())
}

/// Apply a recheck's best price and holder to the offer.
fn rechecked(mut offer: WatchOffer, best: Option<(i64, Option<String>)>, now: i64) -> WatchOffer {
    offer.rechecked_at = Some(now);
    let still = best.and_then(|(price, player)| {
        let meets = match offer.side.as_str() {
            "sell" => price <= offer.threshold,
            "buy" => price >= offer.threshold,
            _ => false,
        };
        meets.then_some((price, player?))
    });
    match still.and_then(|(price, player)| {
        whisper(
            &offer.side,
            &player,
            &offer.name,
            offer.rank,
            offer.subtype.as_deref(),
            price,
        )
        .map(|w| (price, player, w))
    }) {
        Some((price, player, whisper)) => {
            offer.price = price;
            offer.player = player;
            offer.whisper = whisper;
            offer.still_listed = Some(true);
        }
        None => offer.still_listed = Some(false),
    }
    offer
}

/// The one recheck an offer gets, when the player opens the app to act on it.
/// A second call returns the answer already known instead of asking again.
#[tauri::command]
pub async fn recheck_watch_offer(app: AppHandle) -> Result<Option<WatchOffer>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<OfferState>();
        let Some(offer) = fresh(state.get(), unix_now()) else {
            return Ok(None);
        };
        if offer.rechecked_at.is_some() {
            return Ok(Some(offer));
        }
        let session = app.state::<Arc<WfmSession>>();
        let (platform, me) = session
            .require_unlocked()
            .map(|u| (u.platform.clone(), Some(u.username.clone())))
            .unwrap_or_else(|_| ("pc".to_string(), None));
        let query = LiveTopQuery {
            slug: offer.slug.clone(),
            rank: offer.rank.map(|r| r.max(0) as u32),
            subtype: offer.subtype.clone(),
        };
        let top = fetch_live_tops(&platform, me.as_deref(), &[query], |_, _| {})
            .map_err(|_| "Could not recheck warframe.market right now.".to_string())?
            .into_iter()
            .next()
            .filter(|t| t.error.is_none())
            .ok_or("Could not recheck warframe.market right now.")?;
        let best = match offer.side.as_str() {
            "sell" => top.low_sell.map(|p| (p, top.low_sell_by.clone())),
            _ => top.top_buy.map(|p| (p, top.top_buy_by.clone())),
        }
        .and_then(|(p, by)| super::poll::price_for_watch(&offer.side, p).map(|p| (p, by)));
        let updated = rechecked(offer, best, unix_now());
        // The banner may have been dismissed while the request ran.
        if state
            .get()
            .is_some_and(|current| current.watch_id == updated.watch_id)
        {
            state.set(Some(updated.clone()));
            let _ = app.emit(EVENT_WATCH_OFFER, ());
        }
        Ok(Some(updated))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn watch(side: &str, rank: Option<i64>, subtype: Option<&str>) -> Watch {
        Watch {
            id: 7,
            slug: "ash_prime_set".into(),
            name: "Ash Prime Set".into(),
            subtype: subtype.map(String::from),
            rank,
            side: side.into(),
            threshold: 120,
            created_at: String::new(),
            last_price: None,
            last_checked_at: None,
            last_fired_at: None,
        }
    }

    /// Copied from the site's own whisper button (2026-11-04); this app may
    /// only add its name after it.
    #[test]
    fn a_ranked_mod_whisper_is_the_site_text_with_the_app_name_after_it() {
        let site = "/w victor896 Hi! I want to buy: \"Abating Link (rank 0)\" for 13 platinum. (warframe.market)";
        assert_eq!(
            whisper("sell", "victor896", "Abating Link", Some(0), None, 13),
            Some(format!("{site} - TennoWorth"))
        );
    }

    #[test]
    fn whispers_follow_the_market_format_and_name_this_app() {
        assert_eq!(
            whisper("sell", "Seller_1", "Ash Prime Set", None, None, 110).as_deref(),
            Some("/w Seller_1 Hi! I want to buy: \"Ash Prime Set\" for 110 platinum. (warframe.market) - TennoWorth")
        );
        assert_eq!(
            whisper("buy", "Buyer.2", "Primed Flow", Some(10), None, 90).as_deref(),
            Some("/w Buyer.2 Hi! I want to sell: \"Primed Flow (rank 10)\" for 90 platinum. (warframe.market) - TennoWorth")
        );
        assert!(
            whisper("buy", "B", "Lith A1 Relic", None, Some("radiant"), 9)
                .is_some_and(|w| w.contains("\"Lith A1 Relic (radiant)\""))
        );
    }

    #[test]
    fn a_name_that_could_smuggle_a_chat_command_is_never_whispered() {
        for name in [
            "",
            "two words",
            "x\n/unstuck",
            "name;",
            "ünïcode",
            &"a".repeat(33),
        ] {
            assert!(
                whisper("sell", name, "Ash Prime Set", None, None, 110).is_none(),
                "{name:?}"
            );
        }
        assert!(whisper("sell", "ok", "Ash Prime Set", None, None, 0).is_none());
        assert!(whisper("sideways", "ok", "Ash Prime Set", None, None, 5).is_none());
        let quoted = whisper("sell", "ok", "Odd \"Name\"\n", None, None, 5).unwrap_or_default();
        assert!(quoted.contains("\"Odd Name\""), "{quoted}");
    }

    #[test]
    fn offers_expire_after_fifteen_minutes() {
        let offer = offer_for(&watch("sell", None, None), 110, "Seller", 1000);
        assert!(fresh(offer.clone(), 1000 + OFFER_FRESH_SECS - 1).is_some());
        assert!(fresh(offer, 1000 + OFFER_FRESH_SECS).is_none());
    }

    #[test]
    fn a_recheck_follows_the_current_best_order_or_says_it_is_gone() {
        let Some(offer) = offer_for(&watch("sell", None, None), 110, "Seller", 1000) else {
            panic!("offer");
        };
        let moved = rechecked(offer.clone(), Some((115, Some("Other".into()))), 1100);
        assert_eq!(
            (moved.price, moved.player.as_str(), moved.still_listed),
            (115, "Other", Some(true))
        );
        assert!(moved.whisper.starts_with("/w Other "));
        let gone = rechecked(offer.clone(), Some((130, Some("Other".into()))), 1100);
        assert_eq!(
            (gone.price, gone.player.as_str(), gone.still_listed),
            (110, "Seller", Some(false))
        );
        let empty = rechecked(offer.clone(), None, 1100);
        assert_eq!(empty.still_listed, Some(false));
        let anonymous = rechecked(offer, Some((100, None)), 1100);
        assert_eq!(anonymous.still_listed, Some(false));
        assert_eq!(anonymous.rechecked_at, Some(1100));
    }

    #[test]
    fn a_buy_watch_still_listed_means_a_bid_at_or_above_the_threshold() {
        let Some(offer) = offer_for(&watch("buy", None, None), 130, "Buyer", 1000) else {
            panic!("offer");
        };
        assert_eq!(
            rechecked(offer.clone(), Some((120, Some("B".into()))), 1).still_listed,
            Some(true)
        );
        assert_eq!(
            rechecked(offer, Some((119, Some("B".into()))), 1).still_listed,
            Some(false)
        );
    }
}
