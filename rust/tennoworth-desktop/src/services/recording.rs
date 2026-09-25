//! Whether completed trades are actually being recorded.
//!
//! `handle_trade` returning "not accepted" is the tailer's business - it holds
//! the read cursor and retries. It is also the user's business, and the two are
//! not the same question: the tailer needs to know whether to advance, while the
//! user needs to know whether the ledger and the automatic listing updates are
//! still keeping up.
//!
//! Nothing outside the tailer thread used to know the answer. The only signals
//! were a stderr line and a notification, and a webview that mounts later cannot
//! ask about either. This is the readable state: it lives in memory, so it
//! survives the database failure that caused it - a health flag stored in the
//! same database that just refused a write would report health it cannot verify.

use std::sync::{Arc, Mutex};

use serde::Serialize;

/// Why recording is not keeping up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingStop {
    /// The ledger refused a completed trade. The event is held and retried, and
    /// everything behind it waits.
    Ledger { error: String },
    /// The log could not be read, so trades may be happening unobserved.
    Log { error: String },
}

/// What the ledger surface is told about recording.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingState {
    /// No tailer: the game log was never found on this machine.
    Off,
    /// Trades are read and accepted.
    Recording,
    /// Trades are being read but not recorded.
    Paused(RecordingStop),
}

/// The tailer's readable health, shared with the commands that report it.
///
/// A plain mutex rather than an atomic: this is one writer updating a small
/// struct and readers taking a copy to serialize, so the lock is held for
/// nanoseconds and never across the ledger, the network, or a notification.
pub struct Recorder {
    state: Mutex<RecordingState>,
}

impl Recorder {
    /// A recorder for a session with no tailer yet.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(RecordingState::Off),
        })
    }

    pub fn state(&self) -> RecordingState {
        wfm_core::poison::guard(&self.state).clone()
    }

    /// Replace the state. Returns whether it changed, so the caller can emit a
    /// single event per transition instead of one per poll.
    pub fn set(&self, next: RecordingState) -> bool {
        let mut state = wfm_core::poison::guard(&self.state);
        if *state == next {
            return false;
        }
        *state = next;
        true
    }

    fn set_stop(&self, stop: Option<RecordingStop>) -> bool {
        self.set(match stop {
            Some(stop) => RecordingState::Paused(stop),
            None => RecordingState::Recording,
        })
    }
}

impl Default for Recorder {
    fn default() -> Self {
        Self {
            state: Mutex::new(RecordingState::Off),
        }
    }
}

/// One tailer poll's outcome, folded into the state.
///
/// The tailer reads the log and records trades as two different activities that
/// fail independently, so the state distinguishes them: a log it cannot read
/// means trades may be happening unobserved, while a trade the ledger refused
/// means one is held. The ledger is the more specific fault and wins, because it
/// names something the user can act on.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PollOutcome {
    /// The log could not be read this poll.
    pub log_error: Option<String>,
    /// The ledger refused the trade at the cursor, with its reason.
    pub ledger_error: Option<String>,
}

impl PollOutcome {
    /// The stop this poll implies, if any.
    pub fn stop(&self) -> Option<RecordingStop> {
        if let Some(error) = &self.ledger_error {
            return Some(RecordingStop::Ledger {
                error: error.clone(),
            });
        }
        self.log_error
            .as_ref()
            .map(|error| RecordingStop::Log {
                error: error.clone(),
            })
    }

    /// Record that the ledger accepted a trade. That clears a held trade, and
    /// also a log warning: the log was read through a trade that is now
    /// recorded. Without this a single unparsable trade line left recording
    /// reported as paused until the game restarted, while every later trade
    /// was recorded normally.
    pub fn accepted(&mut self) {
        self.ledger_error = None;
        self.log_error = None;
    }
}

/// Fold a poll outcome into the recorder, reporting the new state when it
/// changed.
pub fn observe(recorder: &Recorder, outcome: &PollOutcome) -> Option<RecordingState> {
    recorder
        .set_stop(outcome.stop())
        .then(|| recorder.state())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_recorder_reports_no_tailer() {
        let recorder = Recorder::new();
        assert_eq!(recorder.state(), RecordingState::Off);
        assert!(matches!(recorder.state(), RecordingState::Off));
    }

    /// The tailer finding its log is not itself a trade being recorded, and a
    /// healthy poll has to say so - otherwise the surface reports a pause it
    /// cannot substantiate.
    #[test]
    fn a_clean_poll_reads_as_recording() {
        let recorder = Recorder::new();
        let changed = observe(&recorder, &PollOutcome::default());
        assert_eq!(changed, Some(RecordingState::Recording));
        assert!(matches!(recorder.state(), RecordingState::Recording));
    }

    /// The state reports only transitions, because the tailer polls four times a
    /// second and an event per poll would flood every listener.
    #[test]
    fn an_unchanged_state_is_not_reported_again() {
        let recorder = Recorder::new();
        assert!(observe(&recorder, &PollOutcome::default()).is_some());
        assert_eq!(observe(&recorder, &PollOutcome::default()), None);
    }

    #[test]
    fn a_refused_trade_pauses_with_the_ledger_reason() {
        let recorder = Recorder::new();
        let outcome = PollOutcome {
            ledger_error: Some("database is locked".into()),
            ..PollOutcome::default()
        };

        assert_eq!(
            observe(&recorder, &outcome),
            Some(RecordingState::Paused(RecordingStop::Ledger {
                error: "database is locked".into()
            }))
        );
        assert!(matches!(recorder.state(), RecordingState::Paused(_)));

        // And the pause clears once the ledger accepts again.
        assert_eq!(
            observe(&recorder, &PollOutcome::default()),
            Some(RecordingState::Recording)
        );
        assert!(matches!(recorder.state(), RecordingState::Recording));
    }

    #[test]
    fn an_unreadable_log_pauses_with_its_own_reason() {
        let recorder = Recorder::new();
        let outcome = PollOutcome {
            log_error: Some("EE.log vanished".into()),
            ..PollOutcome::default()
        };

        assert_eq!(
            observe(&recorder, &outcome),
            Some(RecordingState::Paused(RecordingStop::Log {
                error: "EE.log vanished".into()
            }))
        );
    }

    /// Both failing at once is the common shape - a disk that filled up stops
    /// the ledger and can stop the log read - and the held trade is the one the
    /// user can act on, so it is the one reported.
    #[test]
    fn a_held_trade_outranks_an_unreadable_log() {
        let recorder = Recorder::new();
        let outcome = PollOutcome {
            log_error: Some("io error".into()),
            ledger_error: Some("disk full".into()),
        };

        assert_eq!(
            observe(&recorder, &outcome),
            Some(RecordingState::Paused(RecordingStop::Ledger {
                error: "disk full".into()
            }))
        );
    }

    /// A pause is not cleared by a poll that merely stopped failing at the log:
    /// the held trade is still held until the ledger takes it.
    #[test]
    fn accepting_a_trade_is_what_clears_a_held_one() {
        let recorder = Recorder::new();
        let held = PollOutcome {
            ledger_error: Some("locked".into()),
            ..PollOutcome::default()
        };
        observe(&recorder, &held);
        assert!(matches!(recorder.state(), RecordingState::Paused(_)));

        let mut recovered = PollOutcome::default();
        recovered.accepted();
        assert_eq!(
            observe(&recorder, &recovered),
            Some(RecordingState::Recording)
        );
    }

    #[test]
    fn an_accepted_trade_clears_a_log_warning() {
        let recorder = Recorder::new();
        let mut outcome = PollOutcome {
            log_error: Some("a completed trade in the game log could not be read".into()),
            ..PollOutcome::default()
        };
        observe(&recorder, &outcome);
        assert!(matches!(recorder.state(), RecordingState::Paused(_)));

        outcome.accepted();
        assert_eq!(
            observe(&recorder, &outcome),
            Some(RecordingState::Recording)
        );
    }

    #[test]
    /// The wire shape the ledger surface matches on. Pinned here because the
    /// frontend reads it by name, and a rename would otherwise show up only as a
    /// warning that silently stops rendering.
    fn the_state_serializes_to_the_shape_the_frontend_matches_on() {
        let to = |state: RecordingState| serde_json::to_value(state).unwrap();

        assert_eq!(to(RecordingState::Off), serde_json::json!("off"));
        assert_eq!(
            to(RecordingState::Recording),
            serde_json::json!("recording")
        );
        assert_eq!(
            to(RecordingState::Paused(RecordingStop::Ledger {
                error: "locked".into()
            })),
            serde_json::json!({"paused": {"ledger": {"error": "locked"}}})
        );
        assert_eq!(
            to(RecordingState::Paused(RecordingStop::Log {
                error: "gone".into()
            })),
            serde_json::json!({"paused": {"log": {"error": "gone"}}})
        );
    }
}
