//! Poison-recovering lock helpers.
//!
//! A panic while a `std::sync::Mutex` is held poisons it; every later
//! `lock()` then panics too, turning one crash into permanently-broken state
//! (all DB access, the session, the updater...). These helpers recover the
//! guard instead — the same convention the single-flight scan lock already
//! uses (`inventory.rs`). A panic inside a critical section is still a bug;
//! the lock just must not become a second one.

use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Lock a `Mutex`, recovering the guard from a poisoned state.
pub fn guard<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Lock a `RwLock` for reading, recovering from a poisoned state.
pub fn read_guard<T>(m: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    m.read().unwrap_or_else(PoisonError::into_inner)
}

/// Lock a `RwLock` for writing, recovering from a poisoned state.
pub fn write_guard<T>(m: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    m.write().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovers_poisoned_mutex_guard() {
        let m = Mutex::new(41u32);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = m.lock().unwrap();
            panic!("simulated critical-section panic");
        }));
        assert!(m.is_poisoned());
        assert_eq!(*guard(&m), 41);
        *guard(&m) = 42;
        // A poisoned mutex stays poisoned: raw lock() still errors afterwards.
        // Recovering the guard (not the poison) is exactly what guard() is for.
        assert_eq!(*guard(&m), 42);
        assert!(m.lock().is_err());
    }

    #[test]
    fn recovers_poisoned_rwlock_guards() {
        let m = RwLock::new("ok".to_string());
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = m.write().unwrap();
            panic!("simulated write-critical panic");
        }));
        assert!(m.is_poisoned());
        assert_eq!(*read_guard(&m), "ok");
        *write_guard(&m) = "recovered".to_string();
        assert_eq!(*read_guard(&m), "recovered");
    }
}
