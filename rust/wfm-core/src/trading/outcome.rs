//! What actually happened to a mutation, as one vocabulary.
//!
//! Mutations are classified in several places today, each with its own reading
//! of an HTTP response or a governor error: a 2xx and a refusal, a request the
//! governor never dispatched, and a send whose fate is unknown are decided by
//! ad-hoc string comparisons at the call sites. That is how `uncertain_mutation`
//! came to be folded into `pending` in one of them.
//!
//! This module is the single answer. It is deliberately a translation, not a new
//! wire format: [`MutationOutcome::status_str`] reproduces the exact strings the
//! frontend and the shared `wfm-access/outcomes.json` fixture already pin, so
//! adopting it changes no contract.

use wfm_client::governor::AccessError;

/// The wire spellings. Pinned by `wfm-access/outcomes.json` and read by the
/// frontend, so these are a contract rather than a naming choice.
///
/// Defined here because this module owns the vocabulary. Two of them are also
/// declared beside the journal field in `trading::pending` on an unmerged
/// branch; when that lands, that declaration should point here instead of
/// repeating the literals.
const STATUS_OK: &str = "ok";
const STATUS_ERROR: &str = "error";
const STATUS_PENDING: &str = "pending";
const STATUS_UNCERTAIN: &str = "uncertain_mutation";

/// The four things that can happen to a send, with no fourth way to say them.
///
/// The distinction that matters is between [`Self::Deferred`] and
/// [`Self::Uncertain`]: a deferred request never reached the market and can be
/// offered again, while an uncertain one may already have been applied and needs
/// reconciliation first. Collapsing those two is the defect this exists to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationOutcome {
    /// The market confirmed the change.
    Applied,
    /// The market refused it. Retrying the same request changes nothing.
    Rejected,
    /// Never dispatched - the governor refused, or the attempt was abandoned
    /// before it went out. Safe to offer again.
    Deferred,
    /// May or may not have been applied. Needs reconciliation, never a blind
    /// resend.
    Uncertain,
}

impl MutationOutcome {
    /// The wire spelling. Pinned by `tests/fixtures/wfm-access/outcomes.json`
    /// and read by the frontend, so these values are a contract, not a choice.
    pub fn status_str(self) -> &'static str {
        match self {
            Self::Applied => STATUS_OK,
            Self::Rejected => STATUS_ERROR,
            Self::Deferred => STATUS_PENDING,
            Self::Uncertain => STATUS_UNCERTAIN,
        }
    }

    /// Whether the market may already hold this change - the question a resume
    /// decision actually turns on.
    pub fn may_have_landed(self) -> bool {
        matches!(self, Self::Applied | Self::Uncertain)
    }

    /// Whether offering the same request again is meaningful.
    pub fn is_retryable(self) -> bool {
        matches!(self, Self::Deferred)
    }
}

impl From<&AccessError> for MutationOutcome {
    /// Classify a governor refusal by what it means for the request.
    ///
    /// `Cancelled` is deferred rather than rejected: the request was abandoned
    /// before dispatch, so there is nothing to retry *and* nothing that landed.
    /// `Transport` covers a failure to reach the market as well as an ambiguous
    /// one, and the governor already narrows the ambiguous case to
    /// `UncertainMutation` before it gets here, so it is deferred.
    fn from(error: &AccessError) -> Self {
        match error {
            AccessError::UncertainMutation => Self::Uncertain,
            AccessError::Cancelled
            | AccessError::Busy
            | AccessError::Cooldown(_)
            | AccessError::Paused(_)
            | AccessError::Transport
            | AccessError::Persistence => Self::Deferred,
            // A malformed request or a rejected one will be rejected again.
            AccessError::InvalidPolicy | AccessError::InvalidRequest => Self::Rejected,
            AccessError::Http(_) => Self::Rejected,
        }
    }
}

/// A mutation's outcome plus the order it concerns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationRecord {
    pub order_id: Option<String>,
    pub outcome: MutationOutcome,
    /// Why, when there is something to say - the market's message or the
    /// governor's.
    pub reason: Option<String>,
}

impl MutationRecord {
    pub fn new(outcome: MutationOutcome) -> Self {
        Self {
            order_id: None,
            outcome,
            reason: None,
        }
    }

    pub fn for_order(order_id: impl Into<String>, outcome: MutationOutcome) -> Self {
        Self {
            order_id: Some(order_id.into()),
            outcome,
            reason: None,
        }
    }

    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    pub fn status_str(&self) -> &'static str {
        self.outcome.status_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire spellings are read by the frontend and pinned by the shared
    /// fixture. If one of these changes, four surfaces change meaning with it.
    #[test]
    fn the_status_strings_are_the_ones_the_fixture_pins() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/wfm-access/outcomes.json"
        ))
        .unwrap();

        assert_eq!(
            MutationOutcome::Deferred.status_str(),
            fixture["pending"]["status"]
        );
        assert_eq!(
            MutationOutcome::Uncertain.status_str(),
            fixture["uncertain_mutation"]["status"]
        );
        // `ok` and `error` have no fixture entry; the frontend's ItemResult
        // union is what pins them.
        assert_eq!(MutationOutcome::Applied.status_str(), "ok");
        assert_eq!(MutationOutcome::Rejected.status_str(), "error");
    }

    /// The whole point of the vocabulary: these two must never be conflated.
    #[test]
    fn a_deferred_request_and_an_uncertain_one_are_opposites() {
        assert!(MutationOutcome::Deferred.is_retryable());
        assert!(!MutationOutcome::Deferred.may_have_landed());

        assert!(!MutationOutcome::Uncertain.is_retryable());
        assert!(MutationOutcome::Uncertain.may_have_landed());

        // And the two settled ones.
        assert!(MutationOutcome::Applied.may_have_landed());
        assert!(!MutationOutcome::Applied.is_retryable());
        assert!(!MutationOutcome::Rejected.may_have_landed());
        assert!(!MutationOutcome::Rejected.is_retryable());
    }

    #[test]
    fn a_governor_refusal_is_classified_by_what_it_means_for_the_send() {
        // Ambiguous: the market may hold it.
        assert_eq!(
            MutationOutcome::from(&AccessError::UncertainMutation),
            MutationOutcome::Uncertain
        );
        // Never dispatched: safe to offer again.
        for error in [
            AccessError::Cancelled,
            AccessError::Busy,
            AccessError::Cooldown(30),
            AccessError::Paused("maintenance".into()),
            AccessError::Transport,
            AccessError::Persistence,
        ] {
            assert_eq!(
                MutationOutcome::from(&error),
                MutationOutcome::Deferred,
                "{error:?} leaves the request unsent"
            );
        }
        // Refused on its merits: retrying changes nothing.
        for error in [
            AccessError::InvalidPolicy,
            AccessError::InvalidRequest,
            AccessError::Http(400),
            AccessError::Http(403),
        ] {
            assert_eq!(
                MutationOutcome::from(&error),
                MutationOutcome::Rejected,
                "{error:?} will be refused again"
            );
        }
    }

    #[test]
    fn a_record_carries_the_order_and_the_reason_when_there_is_one() {
        let bare = MutationRecord::new(MutationOutcome::Applied);
        assert_eq!(bare.order_id, None);
        assert_eq!(bare.reason, None);

        let detailed = MutationRecord::for_order("order-1", MutationOutcome::Rejected)
            .with_reason("HTTP 403: app.order.error.exceededOrderLimitSamePrice");
        assert_eq!(detailed.order_id.as_deref(), Some("order-1"));
        assert_eq!(detailed.status_str(), "error");
        assert!(detailed.reason.unwrap().contains("exceededOrderLimitSamePrice"));
    }
}
