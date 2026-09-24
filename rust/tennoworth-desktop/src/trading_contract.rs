//! Data contracts shared by the trading services and the storage layer.
//!
//! Leaf module on purpose. `services::eelog` reads the game log and
//! `services::allowance` reconciles the trade allowance, while
//! `persistence::{trades,records}` store what both produce - so both layers have
//! to name these shapes, and while they lived in the services storage had to
//! reach upward for the very rows it writes.
//!
//! Only data and the rules purely about it live here. Reading the log, deciding
//! an allowance, and recording either stay in their owners.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LogPosition {
    pub session: String,
    pub start: u64,
    pub end: u64,
    /// Lower observation bound, not the callback's delivery time.
    pub observed_after: i64,
}

/// A trade the game confirmed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TradeEvent {
    /// The other Tenno's in-game name (platform glyph stripped).
    pub partner: String,
    /// "sale" (we received plat only), "purchase" (we gave plat only), "trade".
    pub kind: String,
    /// Plat that changed hands: received on a sale, spent on a purchase.
    pub plat: i64,
    pub items: Vec<TradeItem>,
    /// The game's uptime stamp at the head of the dialog line, if present -
    /// distinguishes two identical trades in one session.
    pub log_stamp: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TradeItem {
    /// Display name as the game printed it ("Primed Flow", "Lith C5 Relic").
    pub name: String,
    pub qty: i64,
    /// "given" (left our inventory) or "received".
    pub direction: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Scanned,
    Tracked,
    Estimated,
    Unknown,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AllowanceView {
    pub remaining: Option<u32>,
    pub mastery_rank: Option<u32>,
    pub observed_at: Option<i64>,
    pub utc_day: i64,
    pub snapshot_id: Option<i64>,
    pub confidence: Confidence,
    pub reason: Option<String>,
    pub monitoring: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Observation {
    pub account_key: String,
    pub run_id: String,
    pub remaining: Option<u32>,
    pub mastery_rank: Option<u32>,
    pub observed_at: i64,
    pub utc_day: i64,
    pub snapshot_id: i64,
    pub confidence: Confidence,
    pub reason: Option<String>,
    pub before: Option<LogPosition>,
    pub after: Option<LogPosition>,
    pub monitoring: bool,
}

impl Observation {
    pub fn scanned(
        account_key: String,
        snapshot_id: i64,
        raw: &serde_json::Value,
        before: Option<LogPosition>,
        after: Option<LogPosition>,
        started_at: i64,
        observed_at: i64,
    ) -> Self {
        let integer = |key: &str| {
            raw.get(key)
                .and_then(|v| v.as_u64())
                .and_then(|n| u32::try_from(n).ok())
        };
        let mastery_rank = integer("PlayerLevel");
        let remaining = integer("TradesRemaining");
        let monitoring = matches!((&before, &after), (Some(a), Some(b)) if a.session == b.session && a.end <= b.end);
        let mut result = Self {
            account_key,
            run_id: String::new(),
            remaining: remaining.or(mastery_rank),
            mastery_rank,
            observed_at,
            utc_day: observed_at.div_euclid(86_400),
            snapshot_id,
            confidence: if remaining.is_some() {
                Confidence::Scanned
            } else if mastery_rank.is_some() {
                Confidence::Estimated
            } else {
                Confidence::Unknown
            },
            reason: remaining
                .is_none()
                .then(|| "Scan did not contain a usable remaining-trade count.".into()),
            before,
            after,
            monitoring,
        };
        if started_at.div_euclid(86_400) != result.utc_day {
            result.remaining = mastery_rank;
            result.mark_uncertain("Scan crossed the UTC reset; scan again to confirm.");
        } else if !monitoring && remaining.is_some() {
            result.reason =
                Some("Remaining count is from the scan; log monitoring is unavailable.".into());
        } else if !monitoring && mastery_rank.is_some() {
            result.reason = Some(
                "Remaining count was missing; using mastery as an estimate without log monitoring."
                    .into(),
            );
        }
        result
    }

    pub fn mark_uncertain(&mut self, reason: &str) {
        self.confidence = if self.remaining.is_some() {
            Confidence::Estimated
        } else {
            Confidence::Unknown
        };
        self.reason = Some(reason.into());
        self.monitoring = false;
    }

    pub fn rollover(&mut self, now: i64) {
        let day = now.div_euclid(86_400);
        if day > self.utc_day {
            self.utc_day = day;
            self.remaining = self.mastery_rank;
            self.confidence = if self.remaining.is_some() {
                Confidence::Estimated
            } else {
                Confidence::Unknown
            };
            self.reason = Some(
                "UTC reset: mastery-based estimate; scan to confirm the account allowance.".into(),
            );
        } else if day < self.utc_day {
            self.mark_uncertain("System clock moved backwards; scan again to confirm.");
        }
    }

    pub fn reconcile(&mut self, position: &LogPosition, now: i64) {
        self.rollover(now);
        let (Some(before), Some(after)) = (&self.before, &self.after) else {
            return;
        };
        if position.session != after.session {
            self.mark_uncertain(
                "Game log changed; scan again to confirm the account and allowance.",
            );
            return;
        }
        if position.end <= before.end {
            return;
        }
        if !self.monitoring {
            return;
        }
        // Dialogs already underway during acquisition can be included in the
        // inventory response even when their success callback arrives later.
        if position.start < after.end || position.observed_after < self.observed_at {
            self.mark_uncertain("A trade overlapped the scan; remaining count needs confirmation.");
            return;
        }
        if position.observed_after.div_euclid(86_400) != self.utc_day {
            self.mark_uncertain("A trade crossed the UTC reset; scan again to confirm.");
            return;
        }
        self.remaining = self.remaining.map(|n| n.saturating_sub(1));
        if self.confidence == Confidence::Scanned {
            self.confidence = Confidence::Tracked;
        }
    }

    pub fn view(&self, run_id: &str, now: i64) -> AllowanceView {
        let mut current = self.clone();
        current.rollover(now);
        if current.run_id != run_id {
            current.mark_uncertain(
                "Monitoring restarted; scan again to confirm the account and remaining trades.",
            );
        }
        AllowanceView {
            remaining: current.remaining,
            mastery_rank: current.mastery_rank,
            observed_at: Some(current.observed_at),
            utc_day: current.utc_day,
            snapshot_id: Some(current.snapshot_id),
            confidence: current.confidence,
            reason: current.reason,
            monitoring: current.monitoring,
        }
    }
}

/// The view for an account whose allowance has never been read.
pub fn unknown(now: i64) -> AllowanceView {
    AllowanceView {
        remaining: None,
        mastery_rank: None,
        observed_at: None,
        utc_day: now.div_euclid(86_400),
        snapshot_id: None,
        confidence: Confidence::Unknown,
        reason: Some("Scan the game to read its remaining trade allowance.".into()),
        monitoring: false,
    }
}
