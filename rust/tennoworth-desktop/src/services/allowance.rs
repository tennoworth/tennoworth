//! Scan-authoritative trade allowance with conservative log reconciliation.

pub use crate::trading_contract::{AllowanceView, Observation};

pub const EVENT_ALLOWANCE_CHANGED: &str = "trade-allowance-changed";

pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}



#[cfg(test)]
mod tests {
    use crate::trading_contract::{Confidence, LogPosition};
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
