use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Capture work and displayed identity have different lifetimes: a late price
/// response or hide timer must not replace a newer preview or result.
#[derive(Default)]
pub(super) struct CaptureLifecycle {
    pub(super) busy: AtomicBool,
    pub(super) current: Mutex<Option<String>>,
    cancellation: Mutex<Arc<AtomicBool>>,
}

impl CaptureLifecycle {
    pub(super) fn try_begin(&self) -> bool {
        !self.busy.swap(true, Ordering::AcqRel)
    }

    pub(super) fn finish(&self) {
        self.busy.store(false, Ordering::Release);
    }

    pub(super) fn present(&self, id: String) {
        let mut token = self.cancellation.lock().unwrap_or_else(|e| e.into_inner());
        token.store(true, Ordering::Release);
        *token = Arc::new(AtomicBool::new(false));
        *self.current.lock().unwrap_or_else(|e| e.into_inner()) = Some(id);
    }

    pub(super) fn is_current(&self, id: &str) -> bool {
        self.current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_deref()
            == Some(id)
    }

    pub(super) fn cancellation(&self, id: &str) -> Arc<AtomicBool> {
        let token = self.cancellation.lock().unwrap_or_else(|e| e.into_inner());
        if self.is_current(id) { token.clone() } else { Arc::new(AtomicBool::new(true)) }
    }

    pub(super) fn clear(&self) {
        self.cancellation.lock().unwrap_or_else(|e| e.into_inner()).store(true, Ordering::Release);
        *self.current.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_allows_retry_without_changing_the_displayed_capture() {
        let state = CaptureLifecycle::default();
        assert!(state.try_begin());
        assert!(!state.try_begin());
        state.present("first".into());
        state.finish();
        assert!(state.is_current("first"));
        assert!(state.try_begin());
    }

    #[test]
    fn newer_results_and_explicit_hide_invalidate_old_callbacks() {
        let state = CaptureLifecycle::default();
        state.present("first".into());
        state.present("second".into());
        assert!(!state.is_current("first"));
        assert!(state.is_current("second"));
        state.clear();
        assert!(!state.is_current("second"));
    }
    #[test]
    fn old_reward_work_cannot_borrow_a_new_screens_cancellation_token() {
        let state = CaptureLifecycle::default();
        state.present("first".into());
        let first = state.cancellation("first");
        state.present("second".into());
        assert!(first.load(Ordering::Acquire));
        assert!(state.cancellation("first").load(Ordering::Acquire));
        assert!(!state.cancellation("second").load(Ordering::Acquire));
        state.clear();
        assert!(state.cancellation("second").load(Ordering::Acquire));
    }

}
