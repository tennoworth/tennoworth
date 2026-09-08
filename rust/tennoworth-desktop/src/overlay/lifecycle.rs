use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Capture work and displayed identity have different lifetimes: a late price
/// response or hide timer must not replace a newer preview or result.
#[derive(Default)]
pub(super) struct CaptureLifecycle {
    pub(super) busy: AtomicBool,
    pub(super) current: Mutex<Option<String>>,
}

impl CaptureLifecycle {
    pub(super) fn try_begin(&self) -> bool {
        !self.busy.swap(true, Ordering::AcqRel)
    }

    pub(super) fn finish(&self) {
        self.busy.store(false, Ordering::Release);
    }

    pub(super) fn present(&self, id: String) {
        *self.current.lock().unwrap_or_else(|e| e.into_inner()) = Some(id);
    }

    pub(super) fn is_current(&self, id: &str) -> bool {
        self.current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_deref()
            == Some(id)
    }

    pub(super) fn clear(&self) {
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
}
