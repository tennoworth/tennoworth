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

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use wfm_core::trading::plan::PlanGuard;

use crate::services::wfm_session::{CmdError, WfmSession};

/// Which of the five origins is asking. Carried so a refusal can say what is in
/// the way, and so the ledger can name where a change came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationOrigin {
    /// The reviewed batch the user confirmed.
    ReviewedPlan,
    /// A single edit from the orders table.
    ManualEdit,
    /// Removing a listing.
    Delete,
    /// Toggling visibility for several listings at once.
    BulkVisibility,
    /// The automatic quantity adjustment after a sale.
    AutoAdjustment,
}

impl MutationOrigin {
    /// A stable slot value; 0 means "nobody holds it".
    fn code(self) -> u8 {
        match self {
            Self::ReviewedPlan => 1,
            Self::ManualEdit => 2,
            Self::Delete => 3,
            Self::BulkVisibility => 4,
            Self::AutoAdjustment => 5,
        }
    }

    fn from_code(code: u8) -> Self {
        match code {
            1 => Self::ReviewedPlan,
            2 => Self::ManualEdit,
            3 => Self::Delete,
            4 => Self::BulkVisibility,
            _ => Self::AutoAdjustment,
        }
    }

    /// How this origin reads mid-sentence, in a refusal the user sees.
    pub fn describe(self) -> &'static str {
        match self {
            Self::ReviewedPlan => "a listing batch",
            Self::ManualEdit => "a listing edit",
            Self::Delete => "a listing removal",
            Self::BulkVisibility => "a visibility change",
            Self::AutoAdjustment => "an automatic listing update",
        }
    }
}

/// The account-mutation coordinator.
pub struct OrderMutations {
    session: Arc<WfmSession>,
    /// Which origin holds the account, so a refusal can name what is actually in
    /// the way. Reporting the *asking* origin instead produces advice the user
    /// cannot act on - "an automatic listing update is already running" when the
    /// blocker is the batch they are looking at.
    holder: AtomicU8,
}

impl OrderMutations {
    pub fn new(session: Arc<WfmSession>) -> Self {
        Self {
            session,
            holder: AtomicU8::new(0),
        }
    }

    /// Authorize one mutation and hand back the account to perform it with.
    ///
    /// The guard is taken *before* the unlock check on purpose: a logout
    /// acquires the same guard, so holding it means the credential cannot be
    /// discarded between this check and the caller's send.
    pub fn begin(
        &self,
        origin: MutationOrigin,
    ) -> Result<(AccountGuard<'_>, Arc<wfm_core::trading::listing::Unlocked>), CmdError> {
        let guard = self.begin_only(origin)?;
        let unlocked = self.session.require_unlocked()?;
        Ok((guard, unlocked))
    }

    /// The guard alone, for a caller that already holds an account or needs to
    /// do its own error mapping - the batch plan resumes from an account it
    /// decrypted earlier, and the tailer has no account until it reads one.
    pub fn begin_only(&self, origin: MutationOrigin) -> Result<AccountGuard<'_>, CmdError> {
        let guard = self.session.begin_plan().ok_or_else(|| {
            let holder = MutationOrigin::from_code(self.holder.load(Ordering::SeqCst));
            CmdError::of("busy", format!("{} is already running.", holder.describe()))
        })?;
        self.holder.store(origin.code(), Ordering::SeqCst);
        Ok(AccountGuard {
            guard,
            holder: &self.holder,
        })
    }
}

/// The account is held until this is dropped, and the recorded holder is
/// cleared with it so the next refusal names the right origin.
pub(crate) struct AccountGuard<'a> {
    guard: PlanGuard<'a>,
    holder: &'a AtomicU8,
}

impl Drop for AccountGuard<'_> {
    fn drop(&mut self) {
        self.holder.store(0, Ordering::SeqCst);
        // `guard` releases the session's claim when this struct's fields drop.
        let _ = &self.guard;
    }
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
    /// whichever origin asks.
    #[test]
    fn a_second_origin_is_refused_while_one_holds_the_account() {
        let session = Arc::new(WfmSession::for_test(tmp_path("serialize")));
        let mutations = OrderMutations::new(Arc::clone(&session));

        let first = mutations
            .begin_only(MutationOrigin::ReviewedPlan)
            .expect("the batch takes the account");

        let refused = mutations
            .begin_only(MutationOrigin::AutoAdjustment)
            .err()
            .expect("a completion mid-batch must not also mutate");
        assert_eq!(refused.code, "busy");
        // The message names what is in the way, not just that something is.
        assert!(
            refused.message.contains("listing batch"),
            "got {:?}",
            refused.message
        );

        drop(first);
        assert!(
            mutations.begin_only(MutationOrigin::Delete).is_ok(),
            "the account is released when the holder finishes"
        );
    }

    /// The two origins that previously took no guard at all.
    #[test]
    fn the_origins_that_were_unguarded_now_contend_with_the_rest() {
        let session = Arc::new(WfmSession::for_test(tmp_path("unguarded")));
        let mutations = OrderMutations::new(Arc::clone(&session));

        for origin in [
            MutationOrigin::Delete,
            MutationOrigin::BulkVisibility,
            MutationOrigin::AutoAdjustment,
            MutationOrigin::ManualEdit,
        ] {
            let held = mutations.begin_only(origin).expect("takes the account");
            assert!(
                mutations.begin_only(MutationOrigin::ReviewedPlan).is_err(),
                "{origin:?} must block a batch while it holds the account"
            );
            drop(held);
        }
    }

    /// Authorizing without a session is a typed refusal, not a panic - the
    /// caller surfaces `needs_login`/`needs_unlock` and the SPA opens its
    /// dialogs.
    #[test]
    fn authorizing_without_an_unlocked_session_is_typed() {
        let session = Arc::new(WfmSession::for_test(tmp_path("locked")));
        let mutations = OrderMutations::new(session);
        let error = mutations
            .begin(MutationOrigin::Delete)
            .err()
            .expect("no account to mutate with");
        assert!(
            matches!(error.code, "needs_login" | "needs_unlock"),
            "got {}",
            error.code
        );
    }
}
