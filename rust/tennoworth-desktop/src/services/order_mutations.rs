//! Who is allowed to change the account's orders, and one at a time.
//!
//! Five things mutate a WFM order: the reviewed batch plan, a single manual
//! edit, a delete, a bulk visibility toggle, and the automatic adjustment that
//! follows a completed sale. They reached the market by five different routes,
//! and only three of them took the session's plan guard. The other two - delete
//! and bulk visibility - took nothing, and the automatic adjustment took
//! nothing either, so a sale completing mid-batch could PATCH an order the
//! reviewed plan had already reconciled against, and a logout could scrub the
//! session and unlink the saved batch between an adjustment's read and its
//! write.
//!
//! This is the one place that authorizes them. It is deliberately thin: it
//! acquires the session's single guard and hands back the unlocked account, and
//! the caller performs its own send. It does not own the sends because they
//! differ in ways that matter (a batch persists between items, an adjustment
//! reads the orders first, a manual edit validates protection), and expressing
//! that as one method would either lose the differences or grow into the
//! abstraction the plan explicitly rules out.

use std::sync::Arc;

use wfm_core::trading::plan::PlanGuard;

use crate::services::wfm_session::{CmdError, WfmSession};

/// The account-mutation coordinator.
pub struct OrderMutations {
    session: Arc<WfmSession>,
}

impl OrderMutations {
    pub fn new(session: Arc<WfmSession>) -> Self {
        Self { session }
    }

    /// Authorize one mutation and hand back the account to perform it with.
    ///
    /// The guard is taken *before* the unlock check on purpose: a logout
    /// acquires the same guard, so holding it means the credential cannot be
    /// discarded between this check and the caller's send.
    pub fn begin(
        &self,
    ) -> Result<(AccountGuard<'_>, Arc<wfm_core::trading::listing::Unlocked>), CmdError> {
        let guard = self.begin_only()?;
        let unlocked = self.session.require_unlocked()?;
        Ok((guard, unlocked))
    }

    /// The guard alone, for a caller that already holds an account or needs to
    /// do its own error mapping - the batch plan resumes from an account it
    /// decrypted earlier, and the tailer has no account until it reads one.
    pub fn begin_only(&self) -> Result<AccountGuard<'_>, CmdError> {
        let guard = self.session.begin_plan().ok_or_else(|| {
            CmdError::of("busy", "Another order change is already running.")
        })?;
        Ok(AccountGuard { _guard: guard })
    }
}

/// The account is held until this is dropped.
pub(crate) struct AccountGuard<'a> {
    _guard: PlanGuard<'a>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn tmp_path(tag: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let mut p = std::env::temp_dir();
        p.push(format!(
            "ordermut-{}-{}-{}",
            std::process::id(),
            tag,
            N.fetch_add(1, Ordering::SeqCst)
        ));
        p
    }

    /// The invariant this type exists for: one account, one mutation at a time,
    /// whichever caller asks.
    #[test]
    fn a_second_caller_is_refused_while_one_holds_the_account() {
        let session = Arc::new(WfmSession::for_test(tmp_path("serialize")));
        let mutations = OrderMutations::new(Arc::clone(&session));

        let first = mutations
            .begin_only()
            .expect("the batch takes the account");

        let refused = mutations
            .begin_only()
            .err()
            .expect("a completion mid-batch must not also mutate");
        assert_eq!(refused.code, "busy");
        assert!(
            refused.message.contains("order change"),
            "got {:?}",
            refused.message
        );

        drop(first);
        assert!(
            mutations.begin_only().is_ok(),
            "the account is released when the holder finishes"
        );
    }

    #[test]
    fn two_coordinators_on_one_session_contend_for_the_same_account() {
        let session = Arc::new(WfmSession::for_test(tmp_path("two-instances")));
        let listing = OrderMutations::new(Arc::clone(&session));
        let automatic = OrderMutations::new(Arc::clone(&session));
        let batch = listing.begin_only().unwrap();

        let refused = automatic
            .begin_only()
            .err()
            .unwrap();
        assert_eq!(refused.code, "busy");
        assert!(!refused.message.contains("automatic listing update"), "{}", refused.message);
        assert!(refused.message.contains("order change"), "{}", refused.message);

        drop(batch);
        assert!(automatic.begin_only().is_ok());
    }


    /// Authorizing without a session is a typed refusal, not a panic - the
    /// caller surfaces `needs_login`/`needs_unlock` and the SPA opens its
    /// dialogs.
    #[test]
    fn authorizing_without_an_unlocked_session_is_typed() {
        let session = Arc::new(WfmSession::for_test(tmp_path("locked")));
        let mutations = OrderMutations::new(session);
        let error = mutations
            .begin()
            .err()
            .expect("no account to mutate with");
        assert!(
            matches!(error.code, "needs_login" | "needs_unlock"),
            "got {}",
            error.code
        );
    }
}
