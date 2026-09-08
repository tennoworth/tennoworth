//! Scan-authoritative trade allowance with conservative log reconciliation.

use serde::{Deserialize, Serialize};

use crate::services::eelog::LogPosition;

pub const EVENT_ALLOWANCE_CHANGED: &str = "trade-allowance-changed";

pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_name_matches_the_frontend_fixture() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/trade-session/events.json"
        ))
        .unwrap();
        assert_eq!(fixture["allowance_changed"], EVENT_ALLOWANCE_CHANGED);
    }

    fn position(start: u64, end: u64, observed_after: i64) -> LogPosition {
        LogPosition {
            session: "session".into(),
            start,
            end,
            observed_after,
        }
    }

    fn scan(raw: serde_json::Value) -> Observation {
        let mut observation = Observation::scanned(
            "account".into(),
            1,
            &raw,
            Some(position(100, 100, 100)),
            Some(position(200, 200, 102)),
            100,
            102,
        );
        observation.run_id = "run".into();
        observation
    }

    #[test]
    fn missing_and_invalid_metadata_do_not_turn_into_zero() {
        for raw in [
            serde_json::json!({}),
            serde_json::json!({"TradesRemaining":-1}),
            serde_json::json!({"TradesRemaining":"4"}),
            serde_json::json!({"TradesRemaining":4.5}),
        ] {
            let observed = scan(raw);
            assert_eq!(observed.remaining, None);
            assert_eq!(observed.confidence, Confidence::Unknown);
        }
        let zero = scan(serde_json::json!({"TradesRemaining":0,"PlayerLevel":24}));
        assert_eq!(zero.remaining, Some(0));
        assert_eq!(zero.confidence, Confidence::Scanned);
        let estimate = scan(serde_json::json!({"PlayerLevel":24}));
        assert_eq!(estimate.remaining, Some(24));
        assert_eq!(estimate.confidence, Confidence::Estimated);
    }

    #[test]
    fn callbacks_before_scan_are_ignored_and_overlaps_are_uncertain() {
        let mut observed = scan(serde_json::json!({"TradesRemaining":8}));
        observed.reconcile(&position(80, 100, 90), 110);
        assert_eq!(observed.remaining, Some(8));
        assert_eq!(observed.confidence, Confidence::Scanned);
        observed.reconcile(&position(120, 220, 100), 110);
        assert_eq!(observed.remaining, Some(8));
        assert_eq!(observed.confidence, Confidence::Estimated);
        assert!(!observed.monitoring);
    }

    #[test]
    fn fresh_trade_decrements_once_not_once_per_item_and_never_below_zero() {
        let mut observed = scan(serde_json::json!({"TradesRemaining":1}));
        observed.reconcile(&position(201, 230, 103), 110);
        assert_eq!(observed.remaining, Some(0));
        assert_eq!(observed.confidence, Confidence::Tracked);
        observed.reconcile(&position(240, 260, 111), 120);
        assert_eq!(observed.remaining, Some(0));
    }

    #[test]
    fn reset_uses_mastery_not_partial_day_remaining_or_inferred_bonus() {
        let mut observed = scan(serde_json::json!({"TradesRemaining":26,"PlayerLevel":24}));
        observed.rollover(86_400);
        assert_eq!(observed.remaining, Some(24));
        assert_eq!(observed.confidence, Confidence::Estimated);
        assert_eq!(observed.observed_at, 102, "retain scan age");
        observed.reconcile(&position(201, 230, 86_410), 86_412);
        assert_eq!(observed.remaining, Some(23));
        assert_eq!(observed.confidence, Confidence::Estimated);
    }

    #[test]
    fn previous_day_replay_cannot_spend_the_new_day() {
        let mut observed = scan(serde_json::json!({"TradesRemaining":8,"PlayerLevel":24}));
        observed.reconcile(&position(201, 230, 86_399), 86_401);
        assert_eq!(observed.remaining, Some(24));
        assert!(!observed.monitoring);
    }

    #[test]
    fn restarted_or_rotated_monitoring_needs_another_scan() {
        let mut observed = scan(serde_json::json!({"TradesRemaining":8}));
        let view = observed.view("another-run", 120);
        assert_eq!(view.confidence, Confidence::Estimated);
        assert!(!view.monitoring);
        let mut rotated = position(201, 230, 103);
        rotated.session = "new-game".into();
        observed.reconcile(&rotated, 110);
        assert_eq!(observed.remaining, Some(8));
        assert!(!observed.monitoring);
    }

    #[test]
    fn scan_crossing_midnight_does_not_claim_authority_for_either_day() {
        let observed = Observation::scanned(
            "account".into(),
            1,
            &serde_json::json!({"TradesRemaining":3,"PlayerLevel":24}),
            None,
            None,
            86_399,
            86_401,
        );
        assert_eq!(observed.remaining, Some(24));
        assert_eq!(observed.confidence, Confidence::Estimated);
    }
}
