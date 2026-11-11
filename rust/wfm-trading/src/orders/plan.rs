//! The bulk-listing plan executor: turns a browser-submitted [`PlanRequest`]
//! into WFM orders, reconciling against the user's existing orders (create
//! vs. update) and persisting progress after every item so a crash mid-batch
//! is resumable via [`crate::orders::pending`].

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::identity::random_token;
use crate::market::catalog::WfmCatalogItem;
use crate::orders::decode::DecodedOrders;
use crate::orders::listing::{
    decode_user_orders, list_user_orders, patch_one_order, send_mutation, Unlocked,
};
use crate::orders::outcome::MutationOutcome;
use crate::orders::pending::{
    journal_state, write_pending_atomic, JournalState, PendingItem, PendingPlan, STATUS_ERROR,
    STATUS_OK, STATUS_PENDING, STATUS_UNCERTAIN,
};
use crate::time::chrono_now_iso;
use market_domain::limits::{MAX_PER_TRADE, MAX_PLAN_ITEMS, MAX_PLATINUM, MIN_PLATINUM};
use market_domain::orders::{NormalizedOrder, OrderRow, OrderSide};

/// Resets a plan-in-flight flag on scope exit - including early return and
/// panic - so a rejected or crashed request can't leave plan execution wedged.
/// Both adapters serialize plan runs behind an `AtomicBool` and had each
/// written this guard.
pub struct PlanGuard<'a>(&'a AtomicBool);

impl<'a> PlanGuard<'a> {
    /// Take the flag, or `None` if a plan is already running. The guard's
    /// existence IS the claim; dropping it releases.
    pub fn acquire(flag: &'a AtomicBool) -> Option<Self> {
        match flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => Some(PlanGuard(flag)),
            Err(_) => None,
        }
    }
}

impl Drop for PlanGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

const SLUG_MISMATCH_GUARD_MULTIPLIER: u32 = 3;

#[derive(Deserialize)]
pub struct PlanRequest {
    pub items: Vec<PlanItem>,
}

#[derive(Deserialize, Clone)]
pub struct PlanItem {
    /// Reviewed native game scan; retained in pending recovery.
    #[serde(default)]
    pub inventory_snapshot_id: Option<i64>,
    /// warframe.market url_name.
    pub slug: String,
    /// Plat the user wants to list at.
    pub platinum: u32,
    /// How many copies.
    pub quantity: u32,
    /// Explicit reviewed lot; absent callers retain the inferred bulk default.
    #[serde(default)]
    pub per_trade: Option<u32>,
    #[serde(default)]
    pub session: Option<SessionConstraint>,
    #[serde(default)]
    pub reviewed_order: Option<ReviewedOrder>,
    /// "sell" or "buy".
    pub order_type: String,
    /// false = invisible until manually toggled.
    pub visible: bool,
    /// Optional rank (for mods / arcanes). When `None`, we use 0 if the
    /// catalog says the item supports ranks, and omit the field otherwise.
    pub rank: Option<u32>,
    /// Optional subtype (relic refinement, veiled-riven state). When
    /// `None`, we fall back to the catalog's first listed subtype (the
    /// lowest-value default - "intact" for relics, "unrevealed" for
    /// rivens). Omitting the field for items that require it returns 400.
    #[serde(default)]
    pub subtype: Option<String>,
    /// Reference low_sell from the market snapshot, used for slug-mismatch
    /// detection. Caller is expected to populate this from market.json.
    #[serde(default)]
    pub reference_low_sell: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SessionConstraint {
    pub snapshot_id: i64,
    pub utc_day: i64,
    pub budget: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ReviewedOrder {
    New,
    Existing {
        id: String,
        platinum: u64,
        quantity: u64,
        per_trade: Option<u64>,
        visible: bool,
    },
}

#[derive(Serialize)]
pub struct PlanResponse {
    pub plan_id: String,
    pub results: Vec<ItemResult>,
    /// Local journal recording or cleanup failed. Results still describe this run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub durability_error: Option<String>,
}

#[derive(Serialize)]
pub struct ItemResult {
    pub slug: String,
    pub status: String, // "ok" | "skipped" | "error"
    pub message: Option<String>,
    /// WFM order id when status = "ok".
    pub order_id: Option<String>,
    /// "created" (new order) | "updated" (reconciled onto an existing order).
    /// None on errors and on pre-reconcile pending files.
    pub action: Option<String>,
}

#[derive(Debug)]
pub enum PlanValidationError {
    Invalid(String),
    Market(anyhow::Error),
}

impl From<String> for PlanValidationError {
    fn from(message: String) -> Self {
        Self::Invalid(message)
    }
}

impl From<&str> for PlanValidationError {
    fn from(message: &str) -> Self {
        Self::Invalid(message.into())
    }
}

impl From<wfm_client::governor::AccessError> for PlanValidationError {
    fn from(error: wfm_client::governor::AccessError) -> Self {
        Self::Market(error.into())
    }
}

impl std::fmt::Display for PlanValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => formatter.write_str(message),
            Self::Market(error) => std::fmt::Display::fmt(error, formatter),
        }
    }
}

/// Validates a batch before anything in it is sent. The validator is given
/// `Some(orders)` when the executor has just read the account's orders fresh for
/// the item about to be sent; it must check against that body rather than read
/// its own, so each send costs one orders read. `None` means nothing was read
/// for it, and the validator reads the orders itself if it needs them.
///
/// A validation failure ends the run. [`PlanValidationError::Market`] is
/// transient, so unsent items stay `pending` for an explicit resume.
/// [`PlanValidationError::Invalid`] says the batch as reviewed can no longer be
/// sent - a newer scan, protected copies, a spent allowance - and resuming would
/// only fail the same way, so every unsent item is recorded `error` with the
/// reason, wherever the failure happens: before the first send of a submit or a
/// resume, or before a later item. Confirmed rows and `uncertain_mutation` rows
/// are left as they are, so a journal holding nothing else is finished and the
/// next submit is not blocked by it.
pub fn execute_plan(
    pending_path: &std::path::Path,
    unlocked: &Unlocked,
    plan: PlanRequest,
    validate: impl FnMut(Option<&serde_json::Value>) -> Result<(), PlanValidationError>,
) -> PlanResponse {
    execute_plan_with(pending_path, unlocked, plan, |path, unlocked, pending| {
        run_pending(path, unlocked, pending, validate)
    })
}

fn execute_plan_with(
    pending_path: &std::path::Path,
    unlocked: &Unlocked,
    plan: PlanRequest,
    run: impl FnOnce(&std::path::Path, &Unlocked, &mut PendingPlan) -> PlanResponse,
) -> PlanResponse {
    let plan_id = random_token(8);

    if plan.items.is_empty() {
        return PlanResponse {
            plan_id,
            results: vec![],
            durability_error: None,
        };
    }

    // --- enforced caps (defense in depth - the browser also validates) ---
    if plan.items.len() > MAX_PLAN_ITEMS {
        return PlanResponse {
            plan_id,
            results: vec![ItemResult {
                slug: "<batch>".into(),
                status: "error".into(),
                message: Some(format!(
                    "Batch has {} items; the cap is {MAX_PLAN_ITEMS}.",
                    plan.items.len()
                )),
                order_id: None,
                action: None,
            }],
            durability_error: None,
        };
    }

    match journal_state(pending_path) {
        JournalState::Unfinished => {
            return PlanResponse {
                plan_id,
                results: vec![ItemResult {
                    slug: "<batch>".into(),
                    status: "error".into(),
                    message: Some("An unfinished batch is saved. Resume or discard it before sending another.".into()),
                    order_id: None,
                    action: None,
                }],
                durability_error: None,
            };
        }
        JournalState::Unreadable(detail) => {
            return PlanResponse {
                plan_id,
                results: vec![ItemResult {
                    slug: "<batch>".into(),
                    status: "error".into(),
                    message: Some(format!("The saved batch could not be read, so a new one cannot safely replace it. {detail}")),
                    order_id: None,
                    action: None,
                }],
                durability_error: None,
            };
        }
        JournalState::Finished => {
            return PlanResponse {
                plan_id,
                results: vec![ItemResult {
                    slug: "<batch>".into(),
                    status: "error".into(),
                    message: Some("A completed batch is still saved. Retry saving or discard its record before sending another.".into()),
                    order_id: None,
                    action: None,
                }],
                durability_error: None,
            };
        }
        JournalState::Absent => {}
    }

    // Seed the pending file before the first POST so a crash here is
    // recoverable - the browser polls /plan/pending on next connect.
    let mut pending = PendingPlan {
        plan_id: plan_id.clone(),
        started_at: chrono_now_iso(),
        items: plan
            .items
            .into_iter()
            .map(|p| PendingItem {
                inventory_snapshot_id: p.inventory_snapshot_id,
                slug: p.slug,
                platinum: p.platinum,
                quantity: p.quantity,
                per_trade: p.per_trade,
                session: p.session,
                reviewed_order: p.reviewed_order,
                order_type: p.order_type,
                visible: p.visible,
                rank: p.rank,
                subtype: p.subtype,
                reference_low_sell: p.reference_low_sell,
                status: "pending".into(),
                message: None,
                order_id: None,
                action: None,
            })
            .collect(),
    };
    if let Err(e) = write_pending_atomic(pending_path, &pending) {
        return PlanResponse {
            plan_id: pending.plan_id,
            results: vec![ItemResult {
                slug: "<batch>".into(),
                status: "error".into(),
                message: Some(format!(
                    "Could not persist this batch before posting: {e:#}"
                )),
                order_id: None,
                action: None,
            }],
            durability_error: None,
        };
    }

    run(pending_path, unlocked, &mut pending)
}

// Drives a PendingPlan to completion, skipping items already in a terminal
// state (ok / error). Used both by the initial /plan POST and /plan/resume.
// Rewrites the on-disk pending file atomically after every item so a crash
// at any point leaves a consistent record.
pub fn run_pending(
    pending_path: &std::path::Path,
    unlocked: &Unlocked,
    pending: &mut PendingPlan,
    validate: impl FnMut(Option<&serde_json::Value>) -> Result<(), PlanValidationError>,
) -> PlanResponse {
    let http = match wfm_client::build_client_with_user_agent(30, crate::user_agent()) {
        Ok(c) => c,
        Err(e) => {
            return PlanResponse {
                plan_id: pending.plan_id.clone(),
                results: vec![ItemResult {
                    slug: "<batch>".into(),
                    status: "error".into(),
                    message: Some(format!("HTTP client build failed: {e}")),
                    order_id: None,
                    action: None,
                }],
                durability_error: None,
            };
        }
    };

    if let Err(response) = reconcile_pending(
        pending_path,
        pending,
        &unlocked.catalog,
        || decode_user_orders(unlocked, &list_user_orders(unlocked)?),
        write_pending_atomic,
    ) {
        return response;
    }
    run_pending_with(pending_path, pending, validate, |item, validate| {
        execute_one(
            &http,
            unlocked,
            item,
            &mut || list_user_orders(unlocked),
            validate,
        )
    })
}

type Validator<'a> = dyn FnMut(Option<&serde_json::Value>) -> Result<(), PlanValidationError> + 'a;

fn run_pending_with(
    pending_path: &std::path::Path,
    pending: &mut PendingPlan,
    validate: impl FnMut(Option<&serde_json::Value>) -> Result<(), PlanValidationError>,
    execute: impl FnMut(&PlanItem, &mut Validator<'_>) -> ItemResult,
) -> PlanResponse {
    run_pending_with_store(
        pending_path,
        pending,
        validate,
        execute,
        write_pending_atomic,
    )
}

fn run_pending_with_store(
    pending_path: &std::path::Path,
    pending: &mut PendingPlan,
    mut validate: impl FnMut(Option<&serde_json::Value>) -> Result<(), PlanValidationError>,
    mut execute: impl FnMut(&PlanItem, &mut Validator<'_>) -> ItemResult,
    mut persist: impl FnMut(&std::path::Path, &PendingPlan) -> anyhow::Result<()>,
) -> PlanResponse {
    if let Err(error) = validate(None) {
        return validation_failure(pending_path, pending, error, &mut persist);
    }
    // The check before each send is the executor's, against the orders it has
    // just read for that item; another check here would cost another read.
    for i in 0..pending.items.len() {
        let Some(item) = pending.items.get(i) else {
            continue;
        };
        if item.status != STATUS_PENDING {
            continue;
        }
        if let Err(error) = wfm_client::governor::process().check(
            wfm_client::governor::Kind::Mutation,
            &wfm_client::governor::context(),
        ) {
            return validation_failure(pending_path, pending, error, &mut persist);
        }
        let plan_item = PlanItem::from(item);
        let prior_message = item.message.clone();
        // A crash after the market accepts the mutation but before its result
        // reaches disk must never leave this item eligible for blind replay.
        if let Some(item) = pending.items.get_mut(i) {
            item.status = STATUS_UNCERTAIN.into();
            item.message = Some("The app stopped while handling this listing. Check My Orders before listing it again.".into());
        }
        if let Err(error) = persist(pending_path, pending) {
            if let Some(item) = pending.items.get_mut(i) {
                item.status = STATUS_PENDING.into();
                item.message = prior_message;
            }
            return durability_failure(
                pending,
                format!("Could not save the listing before sending it: {error}"),
            );
        }
        let mut refused = None;
        let result = execute(&plan_item, &mut |orders: Option<&serde_json::Value>| {
            let outcome = validate(orders);
            if let Err(PlanValidationError::Invalid(message)) = &outcome {
                refused = Some(message.clone());
            }
            outcome
        });
        // Both stop the batch, but they do not mean the same thing and the
        // journal must not say they do. `pending` was never dispatched, so
        // re-offering it is safe. `uncertain_mutation` may already have been
        // applied, so it is recorded as itself and only a resume's check of
        // current orders can move it.
        let interrupted = matches!(result.status.as_str(), STATUS_PENDING | STATUS_UNCERTAIN);
        if let Some(item) = pending.items.get_mut(i) {
            item.status = result.status.clone();
            item.message = result.message.clone();
            item.order_id = result.order_id.clone();
            item.action = result.action.clone();
        }
        if let Err(error) = persist(pending_path, pending) {
            return durability_failure(pending, format!("Could not save the confirmed listing result: {error}. Check My Orders before resuming or listing it again."));
        }
        if let Some(message) = refused {
            return validation_failure(
                pending_path,
                pending,
                PlanValidationError::Invalid(message),
                &mut persist,
            );
        }
        if interrupted {
            break;
        }
    }

    plan_rows(pending, None)
}

fn plan_rows(pending: &PendingPlan, durability_error: Option<String>) -> PlanResponse {
    PlanResponse {
        plan_id: pending.plan_id.clone(),
        results: pending
            .items
            .iter()
            .map(|item| ItemResult {
                slug: item.slug.clone(),
                status: item.status.clone(),
                message: item.message.clone(),
                order_id: item.order_id.clone(),
                action: item.action.clone(),
            })
            .collect(),
        durability_error,
    }
}

fn durability_failure(pending: &PendingPlan, message: String) -> PlanResponse {
    plan_rows(pending, Some(message))
}

/// Ends a run on a validation failure, under the rule documented on
/// [`execute_plan`]. Every row is returned, so the desktop can still record the
/// confirmed ones.
fn validation_failure(
    pending_path: &std::path::Path,
    pending: &mut PendingPlan,
    error: impl Into<PlanValidationError>,
    persist: &mut impl FnMut(&std::path::Path, &PendingPlan) -> anyhow::Result<()>,
) -> PlanResponse {
    let error = error.into();
    let message = error.to_string();
    if let PlanValidationError::Market(_) = error {
        let mut response = plan_rows(pending, None);
        for row in &mut response.results {
            if row.status == STATUS_PENDING {
                row.message = Some(message.clone());
            }
        }
        return response;
    }
    let mut refused = false;
    for item in &mut pending.items {
        if item.status == STATUS_PENDING {
            item.status = STATUS_ERROR.into();
            item.message = Some(message.clone());
            refused = true;
        }
    }
    if refused {
        if let Err(error) = persist(pending_path, pending) {
            return durability_failure(
                pending,
                format!("Could not save that this batch was refused before sending: {error}"),
            );
        }
    }
    plan_rows(pending, None)
}

/// How current orders explain an item whose send went unconfirmed.
enum Reconciled {
    /// The listing is live as the item intended. `action` is None when an
    /// older journal did not record whether a create or an update was meant.
    Applied {
        order_id: String,
        action: Option<&'static str>,
    },
    /// The mutation clearly did not land, so sending it is not a replay.
    NotApplied,
    Unclear,
}

/// Resolve `uncertain_mutation` items against one fresh read of the account's
/// orders before a resume sends anything. Only unambiguous evidence moves an
/// item: a single order with the item's identity in the intended state is
/// recorded `ok`; a reviewed create with no such order, or a reviewed update
/// whose order still holds exactly the reviewed prior state, returns to
/// `pending` and is sent by this resume after the usual checks. Everything
/// else - duplicates, a partial match, an older journal with nothing reviewed to
/// compare against, a failed or unreadable read - stays uncertain for the user
/// to check.
fn reconcile_pending(
    pending_path: &std::path::Path,
    pending: &mut PendingPlan,
    catalog: &BTreeMap<String, WfmCatalogItem>,
    read_orders: impl FnOnce() -> anyhow::Result<DecodedOrders>,
    mut persist: impl FnMut(&std::path::Path, &PendingPlan) -> anyhow::Result<()>,
) -> Result<(), PlanResponse> {
    if !pending
        .items
        .iter()
        .any(|item| item.status == STATUS_UNCERTAIN)
    {
        return Ok(());
    }
    let orders = read_orders();
    for item in pending
        .items
        .iter_mut()
        .filter(|item| item.status == STATUS_UNCERTAIN)
    {
        let orders = match &orders {
            Ok(orders) => orders,
            Err(error) => {
                item.message = Some(format!("Current orders could not be read to check this listing: {error:#}. Resume again to retry, or check My Orders."));
                continue;
            }
        };
        match reconcile_item(&PlanItem::from(&*item), catalog, orders) {
            Reconciled::Applied { order_id, action } => {
                item.status = STATUS_OK.into();
                item.message =
                    Some("Confirmed in current orders after the send went unconfirmed.".into());
                item.order_id = Some(order_id);
                item.action = action.map(str::to_string);
            }
            Reconciled::NotApplied => {
                item.status = STATUS_PENDING.into();
                item.message = Some(
                    "Current orders show this listing was not applied, so it can be sent.".into(),
                );
            }
            Reconciled::Unclear => {
                item.message = Some("Current orders do not show clearly whether this listing was applied. Check My Orders before listing it again, then discard this batch.".into());
            }
        }
    }
    persist(pending_path, pending).map_err(|error| {
        durability_failure(
            pending,
            format!("Could not save what current orders showed: {error}. Check My Orders before resuming or listing it again."),
        )
    })
}

fn reconcile_item(
    item: &PlanItem,
    catalog: &BTreeMap<String, WfmCatalogItem>,
    orders: &DecodedOrders,
) -> Reconciled {
    let Some(cat) = catalog.get(&item.slug) else {
        return Reconciled::Unclear;
    };
    let key = plan_item_key(item, cat);
    let live: Vec<&NormalizedOrder> = orders
        .orders
        .iter()
        .filter_map(|row| match row {
            OrderRow::Supported(order) if order.key() == key => Some(order),
            _ => None,
        })
        .collect();
    let order = match (live.as_slice(), &item.reviewed_order) {
        ([], Some(ReviewedOrder::New)) => return Reconciled::NotApplied,
        ([order], _) => *order,
        _ => return Reconciled::Unclear,
    };
    // The lots mirror execute_one: a create sends the reviewed or inferred lot,
    // an update keeps the prior lot unless one was reviewed.
    let create_lot = if cat.bulk_tradable {
        u64::from(
            item.per_trade
                .unwrap_or_else(|| per_trade_for(item.quantity)),
        )
    } else {
        1
    };
    let update_lot = |prior: Option<u64>| {
        if cat.bulk_tradable {
            item.per_trade.map_or(prior.unwrap_or(1), u64::from)
        } else {
            1
        }
    };
    let applied = |action| Reconciled::Applied {
        order_id: order.id.clone(),
        action,
    };
    match &item.reviewed_order {
        Some(ReviewedOrder::New) => {
            if is_intended(item, order, create_lot) {
                applied(Some("created"))
            } else {
                Reconciled::Unclear
            }
        }
        Some(reviewed @ ReviewedOrder::Existing { id, per_trade, .. }) => {
            if order.id != *id {
                Reconciled::Unclear
            } else if is_intended(item, order, update_lot(*per_trade)) {
                applied(Some("updated"))
            } else if review_matches(reviewed, Some(&existing_order(order))) {
                Reconciled::NotApplied
            } else {
                Reconciled::Unclear
            }
        }
        None => {
            if is_intended(item, order, create_lot)
                || is_intended(item, order, update_lot(order.per_trade))
            {
                applied(None)
            } else {
                Reconciled::Unclear
            }
        }
    }
}

fn is_intended(item: &PlanItem, order: &NormalizedOrder, lot: u64) -> bool {
    order.platinum == u64::from(item.platinum).saturating_mul(lot)
        && order.quantity == u64::from(item.quantity)
        && order.per_trade.unwrap_or(1) == lot
}

fn existing_order(order: &NormalizedOrder) -> ExistingOrder {
    ExistingOrder {
        id: order.id.clone(),
        platinum: order.platinum,
        quantity: order.quantity,
        per_trade: order.per_trade,
        visible: order.visible,
        ambiguous: false,
    }
}

impl From<&PendingItem> for PlanItem {
    fn from(item: &PendingItem) -> Self {
        Self {
            inventory_snapshot_id: item.inventory_snapshot_id,
            slug: item.slug.clone(),
            platinum: item.platinum,
            quantity: item.quantity,
            per_trade: item.per_trade,
            session: item.session.clone(),
            reviewed_order: item.reviewed_order.clone(),
            order_type: item.order_type.clone(),
            visible: item.visible,
            rank: item.rank,
            subtype: item.subtype.clone(),
            reference_low_sell: item.reference_low_sell,
        }
    }
}

// `perTrade` must EVENLY DIVIDE `quantity` on bulk-tradable items (relics
// and similar). Listing qty=27 with perTrade=6 returns
// `app.field.orders.perTradeMustDivideQuantity` because 27/6 is not an
// integer. We pick the largest divisor of `quantity` that fits under
// MAX_PER_TRADE. Examples:
//   qty=27 → 3   (divisors: 1, 3, 9, 27; only 3 fits ≤ 6)
//   qty=10 → 5   (1, 2, 5, 10; 5 is the largest ≤ 6)
//   qty=12 → 6   (1, 2, 3, 4, 6, 12; 6 fits exactly)
//   qty=7  → 1   (1, 7; only 1 fits)
//   qty=1  → 1
pub fn per_trade_for(quantity: u32) -> u32 {
    if quantity == 0 {
        return 1;
    }
    let start = quantity.min(MAX_PER_TRADE);
    for d in (1..=start).rev() {
        if quantity.is_multiple_of(d) {
            return d;
        }
    }
    1
}

pub fn validate_lot(
    quantity: u32,
    per_trade: Option<u32>,
    bulk_tradable: bool,
) -> Result<(), String> {
    if quantity == 0 {
        return Err("quantity must be > 0".into());
    }
    if let Some(lot) = per_trade {
        if lot == 0 || lot > MAX_PER_TRADE || !quantity.is_multiple_of(lot) {
            return Err(format!(
                "units per trade must be 1–{MAX_PER_TRADE} and divide quantity"
            ));
        }
        if !bulk_tradable && lot != 1 {
            return Err("this item does not support bulk trading; use one unit per trade".into());
        }
    }
    Ok(())
}

fn build_order_patch(item: &PlanItem, cat: &WfmCatalogItem, lot: u32) -> serde_json::Value {
    let mut body = serde_json::Map::new();
    body.insert(
        "platinum".into(),
        serde_json::json!(item.platinum.saturating_mul(lot)),
    );
    body.insert("quantity".into(), serde_json::json!(item.quantity));
    // Omit visibility so updating a visible order cannot silently hide it.
    // Legacy callers did not edit the lot on PATCH; only explicit review does.
    if cat.bulk_tradable {
        if let Some(lot) = item.per_trade {
            body.insert("perTrade".into(), serde_json::json!(lot));
        }
    }
    serde_json::Value::Object(body)
}

/// One of the user's live WFM orders, as much as reconciliation needs.
pub struct ExistingOrder {
    pub id: String,
    pub platinum: u64,
    pub quantity: u64,
    pub per_trade: Option<u64>,
    pub visible: Option<bool>,
    pub ambiguous: bool,
}

/// The identity the market enforces uniqueness on - a second order with the
/// same key 403s with `app.order.error.exceededOrderLimitSamePrice`. The
/// contract owns the shape; this alias keeps the call sites reading as they did.
pub type OrderKey = market_domain::orders::OrderKey;

fn index_one_order(out: &mut BTreeMap<OrderKey, ExistingOrder>, order: &NormalizedOrder) {
    let key = order.key();
    if let Some(prior) = out.get_mut(&key) {
        prior.ambiguous = true;
        return;
    }
    out.insert(
        key,
        ExistingOrder {
            id: order.id.clone(),
            platinum: order.platinum,
            quantity: order.quantity,
            per_trade: order.per_trade,
            visible: order.visible,
            ambiguous: false,
        },
    );
}

/// Index a decoded orders response by [`OrderKey`]. Rows the decoder could not
/// read whole are absent rather than guessed at, so a plan cannot reconcile
/// against an order nobody could describe.
pub fn index_existing_orders(decoded: &DecodedOrders) -> BTreeMap<OrderKey, ExistingOrder> {
    let mut out = BTreeMap::new();
    for row in &decoded.orders {
        if let OrderRow::Supported(order) = row {
            index_one_order(&mut out, order);
        }
    }
    out
}

/// The OrderKey this plan item will occupy on WFM. MUST mirror
/// build_order_body's rank/subtype normalization - if the body would send
/// rank 0 by default, the key says Some(0), so it collides with exactly the
/// order WFM would reject as a duplicate.
pub fn plan_item_key(item: &PlanItem, cat: &WfmCatalogItem) -> OrderKey {
    let rank = cat.max_rank.map(|_| u64::from(item.rank.unwrap_or(0)));
    let subtype = if cat.subtypes.is_empty() {
        None
    } else {
        Some(
            item.subtype
                .clone()
                .filter(|s| cat.subtypes.contains(s))
                .unwrap_or_else(|| cat.subtypes.first().cloned().unwrap_or_default()),
        )
    };
    OrderKey {
        item_id: cat.item_id.clone(),
        side: match item.order_type.as_str() {
            "buy" => OrderSide::Buy,
            _ => OrderSide::Sell,
        },
        rank,
        subtype,
    }
}

// Constructs the JSON body for `POST /v2/order`. Per-field rules captured
// from WFM 400 responses (May 2026):
//   - `itemId`, `type` (not `order_type`!), `platinum`, `quantity`,
//     `visible` are always required.
//   - `perTrade` is required only for bulkTradable items and forbidden
//     otherwise. Use the reviewed lot, or the legacy divisor default.
//   - `rank` is required for items with `maxRank` in the catalog, and is
//     `app.field.notAllowed` for items without it. Default to 0 (unranked).
//   - `subtype` is required for items with `subtypes[]` in the catalog.
//     Default to the first listed subtype - that's the lowest-value
//     variant by WFM convention (intact relic, unrevealed riven) and
//     matches what the user almost always wants to dump first.
pub fn build_order_body(item: &PlanItem, cat: &WfmCatalogItem) -> serde_json::Value {
    let mut body = serde_json::Map::new();
    body.insert("itemId".into(), serde_json::json!(cat.item_id));
    body.insert("type".into(), serde_json::json!(item.order_type));
    let lot = if cat.bulk_tradable {
        item.per_trade
            .unwrap_or_else(|| per_trade_for(item.quantity))
    } else {
        1
    };
    body.insert(
        "platinum".into(),
        serde_json::json!(item.platinum.saturating_mul(lot)),
    );
    body.insert("quantity".into(), serde_json::json!(item.quantity));
    body.insert("visible".into(), serde_json::json!(item.visible));
    if cat.bulk_tradable {
        body.insert(
            "perTrade".into(),
            serde_json::json!(item
                .per_trade
                .unwrap_or_else(|| per_trade_for(item.quantity))),
        );
    }
    if cat.max_rank.is_some() {
        body.insert("rank".into(), serde_json::json!(item.rank.unwrap_or(0)));
    }
    if !cat.subtypes.is_empty() {
        let chosen = item
            .subtype
            .clone()
            .filter(|s| cat.subtypes.contains(s))
            .unwrap_or_else(|| cat.subtypes.first().cloned().unwrap_or_default());
        body.insert("subtype".into(), serde_json::json!(chosen));
    }
    serde_json::Value::Object(body)
}

fn execute_one(
    http: &Client,
    unlocked: &Unlocked,
    item: &PlanItem,
    read_orders: &mut impl FnMut() -> anyhow::Result<serde_json::Value>,
    validate: &mut Validator<'_>,
) -> ItemResult {
    let mk_err = |msg: String| ItemResult {
        slug: item.slug.clone(),
        status: "error".into(),
        message: Some(msg),
        order_id: None,
        action: None,
    };

    // --- safety caps ---
    if item.platinum < MIN_PLATINUM {
        return mk_err(format!("price {}p < min {MIN_PLATINUM}p", item.platinum));
    }
    if item.platinum > MAX_PLATINUM {
        return mk_err(format!("price {}p > max {MAX_PLATINUM}p", item.platinum));
    }
    // A price this far on the wrong side of the market - a sell far below it,
    // a buy far above it - more likely means the reference belongs to another
    // item than that the user means it. The other direction is just a cheap
    // buy or a hopeful sell.
    if let Some(low) = item.reference_low_sell.filter(|low| *low > 0) {
        if item.order_type == "buy" {
            if item.platinum > low.saturating_mul(SLUG_MISMATCH_GUARD_MULTIPLIER) {
                return mk_err(format!(
                    "our {}p is more than {SLUG_MISMATCH_GUARD_MULTIPLIER}× ref low_sell {low}p; \
                     likely a slug mismatch - refusing",
                    item.platinum
                ));
            }
        } else if low > item.platinum.saturating_mul(SLUG_MISMATCH_GUARD_MULTIPLIER) {
            return mk_err(format!(
                "ref low_sell {low}p is more than {SLUG_MISMATCH_GUARD_MULTIPLIER}× our {}p; \
                 likely a slug mismatch - refusing",
                item.platinum
            ));
        }
    }
    if !matches!(item.order_type.as_str(), "sell" | "buy") {
        return mk_err(format!(
            "order_type {:?} not in (sell, buy)",
            item.order_type
        ));
    }
    if item.quantity == 0 {
        return mk_err("quantity must be > 0".into());
    }

    // --- resolve slug → item_id ---
    let cat = match unlocked.catalog.get(&item.slug) {
        Some(c) => c,
        None => return mk_err(format!("slug {:?} not in WFM catalog", item.slug)),
    };
    if let Err(message) = validate_lot(item.quantity, item.per_trade, cat.bulk_tradable) {
        return mk_err(message);
    }

    let unread = |error: anyhow::Error| ItemResult {
        slug: item.slug.clone(),
        status: "pending".into(),
        message: Some(format!("Could not revalidate current orders: {error}")),
        order_id: None,
        action: None,
    };
    let orders = match read_orders() {
        Ok(body) => body,
        Err(error) => return unread(error),
    };
    let existing = match decode_user_orders(unlocked, &orders) {
        Ok(decoded) => index_existing_orders(&decoded),
        Err(error) => return unread(error),
    };
    if let Some(reviewed) = &item.reviewed_order {
        if !review_matches(reviewed, existing.get(&plan_item_key(item, cat))) {
            return mk_err(
                "Order state changed or is ambiguous. Review the batch again before posting."
                    .into(),
            );
        }
    }
    if let Err(error) = validate(Some(&orders)) {
        let mut result = mk_err(error.to_string());
        if matches!(error, PlanValidationError::Market(_)) {
            result.status = "pending".into();
        }
        return result;
    }

    // An order with this exact identity already exists → PATCH it. The plan's
    // quantities come from the current inventory scan (they already count the
    // listed copies), so overwrite price + quantity - never sum. Visibility is
    // left alone: the existing order keeps whatever the user chose on WFM.
    if let Some(prior) = existing.get(&plan_item_key(item, cat)) {
        if prior.ambiguous {
            return mk_err(
                "More than one existing order matches this item; resolve it in My orders.".into(),
            );
        }
        let lot = if cat.bulk_tradable {
            match item.per_trade {
                Some(lot) => lot,
                None => match u32::try_from(prior.per_trade.unwrap_or(1)) {
                    Ok(lot) => lot,
                    Err(_) => return mk_err("Existing order has an invalid lot size.".into()),
                },
            }
        } else {
            1
        };
        if let Err(message) = validate_lot(item.quantity, Some(lot), cat.bulk_tradable) {
            return mk_err(message);
        }
        if item.platinum.saturating_mul(lot) > MAX_PLATINUM {
            return mk_err(format!(
                "Lot total exceeds {MAX_PLATINUM}p; reduce price or units per trade."
            ));
        }
        let patch = build_order_patch(item, cat, lot);
        let r = patch_one_order(http, unlocked, &prior.id, &patch);
        return if r.status == "ok" {
            ItemResult {
                slug: item.slug.clone(),
                status: "ok".into(),
                message: Some(format!(
                    "updated existing order (was {}p × {})",
                    prior.platinum, prior.quantity
                )),
                order_id: Some(prior.id.clone()),
                action: Some("updated".into()),
            }
        } else {
            ItemResult {
                slug: item.slug.clone(),
                status: r.status,
                message: r.message,
                order_id: Some(prior.id.clone()),
                action: None,
            }
        };
    }

    let lot = if cat.bulk_tradable {
        item.per_trade
            .unwrap_or_else(|| per_trade_for(item.quantity))
    } else {
        1
    };
    if item.platinum.saturating_mul(lot) > MAX_PLATINUM {
        return mk_err(format!(
            "Lot total exceeds {MAX_PLATINUM}p; reduce price or units per trade."
        ));
    }
    let body = build_order_body(item, cat);

    // Order-creation endpoint (verified via the WFM frontend's actual
    // network call, May 2026): POST /v2/order. Singular. /v2/me/orders
    // returns 404 for POST - that path is for GET-list semantics, not
    // create. v2 endpoints rely on the JWT cookie that the website sets
    // (not the Authorization header). We send both so either auth path
    // works - the WFM server picks whichever it understands for v1 vs v2.
    // Header set captured from the live frontend's preflight:
    //   access-control-request-headers: content-type, crossplay, language, platform
    // It uses pure cookie auth - no Authorization header. We mirror that.
    let resp = send_mutation(
        wfm_client::wfm_authed_headers(
            http.post("https://api.warframe.market/v2/order"),
            &unlocked.platform,
            &unlocked.jwt,
        )
        .json(&body),
    );
    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            return ItemResult {
                slug: item.slug.clone(),
                status: MutationOutcome::from(&e).status_str().into(),
                message: Some(e.to_string()),
                order_id: None,
                action: None,
            }
        }
    };
    let status = resp.status();
    let resp_body: serde_json::Value = match resp.json() {
        Ok(body) => body,
        Err(_) if status.is_success() => return ItemResult { slug: item.slug.clone(), status: MutationOutcome::Uncertain.status_str().into(), message: Some("The listing may have been applied, but its response could not be read. Resume checks current orders before sending anything for it again.".into()), order_id: None, action: None },
        Err(_) => serde_json::Value::Null,
    };
    if !status.is_success() {
        // v2 puts errors under `.error` (object or array of strings); v1 used
        // a top-level `.error` string. Render whatever we can find verbatim
        // so the user sees the real validation message.
        let msg = resp_body
            .get("error")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "(no error message)".to_string());
        return mk_err(format!("WFM HTTP {status}: {msg}"));
    }
    // v2 returns the created order under .data; v1 used .payload.order. Try both.
    let order_id = resp_body
        .pointer("/data/id")
        .or_else(|| resp_body.pointer("/payload/order/id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    if order_id.is_none() {
        return ItemResult { slug: item.slug.clone(), status: MutationOutcome::Uncertain.status_str().into(), message: Some("The market accepted the listing but did not say which order it is. Resume checks current orders before sending anything for it again.".into()), order_id: None, action: None };
    }
    ItemResult {
        slug: item.slug.clone(),
        status: "ok".into(),
        message: None,
        order_id,
        action: Some("created".into()),
    }
}

fn review_matches(reviewed: &ReviewedOrder, prior: Option<&ExistingOrder>) -> bool {
    match (reviewed, prior) {
        (ReviewedOrder::New, None) => true,
        (
            ReviewedOrder::Existing {
                id,
                platinum,
                quantity,
                per_trade,
                visible,
            },
            Some(prior),
        ) => {
            !prior.ambiguous
                && *id == prior.id
                && *platinum == prior.platinum
                && *quantity == prior.quantity
                && *per_trade == prior.per_trade
                && Some(*visible) == prior.visible
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orders::pending::clear_finished_pending;
    use std::time::Duration;

    /// The executor's own contract for a validator: it refuses the send with
    /// the validation message, and a transient failure leaves the item unsent.
    fn send_after(item: &PlanItem, validate: &mut Validator<'_>) -> ItemResult {
        match validate(None) {
            Ok(()) => ItemResult {
                slug: item.slug.clone(),
                status: "ok".into(),
                message: None,
                order_id: Some(format!("remote-{}", item.slug)),
                action: Some("created".into()),
            },
            Err(error) => ItemResult {
                slug: item.slug.clone(),
                status: if matches!(error, PlanValidationError::Market(_)) {
                    STATUS_PENDING.into()
                } else {
                    STATUS_ERROR.into()
                },
                message: Some(error.to_string()),
                order_id: None,
                action: None,
            },
        }
    }

    /// A refusal part-way through a batch ends it: the rows already confirmed
    /// are kept for history, and the rest are recorded as refused rather than
    /// left pending, because a resume would only be refused the same way.
    #[test]
    fn late_validation_failure_preserves_confirmed_rows_for_history() {
        let path = std::env::temp_dir().join(format!("late-validation-{}.json", random_token(8)));
        let mut pending: PendingPlan = serde_json::from_value(serde_json::json!({
            "plan_id":"late-validation", "started_at":"test", "items":[
                {"slug":"sent","quantity":1,"platinum":12,"order_type":"sell","visible":false,"status":"pending"},
                {"slug":"refused","quantity":1,"platinum":12,"order_type":"sell","visible":false,"status":"pending"},
                {"slug":"later","quantity":1,"platinum":12,"order_type":"sell","visible":false,"status":"pending"}
            ]
        })).unwrap();
        write_pending_atomic(&path, &pending).unwrap();
        let mut validations = 0;
        let mut attempted = vec![];
        let response = run_pending_with(
            &path,
            &mut pending,
            |_| {
                validations += 1;
                if validations == 3 {
                    Err(PlanValidationError::Invalid("inventory changed".into()))
                } else {
                    Ok(())
                }
            },
            |item, validate| {
                attempted.push(item.slug.clone());
                send_after(item, validate)
            },
        );
        assert_eq!(attempted, ["sent", "refused"]);
        assert_eq!(response.results.len(), 3);
        assert_eq!(response.results[0].order_id.as_deref(), Some("remote-sent"));
        for row in &response.results[1..] {
            assert_eq!(row.status, STATUS_ERROR);
            assert!(row
                .message
                .as_deref()
                .unwrap()
                .contains("inventory changed"));
        }
        let saved = crate::orders::pending::load_pending(&path)
            .unwrap()
            .unwrap();
        assert!(saved.is_finished());
        clear_finished_pending(&path, &saved).unwrap();
        assert!(matches!(journal_state(&path), JournalState::Absent));
    }

    /// A submit refused before anything was sent must not leave a journal that
    /// blocks the next submit; a transient refusal keeps it for Resume.
    #[test]
    fn a_batch_refused_before_any_send_does_not_block_the_next_submit() {
        let unlocked = Unlocked {
            jwt: "fixture".into(),
            username: "fixture".into(),
            platform: "pc".into(),
            catalog: std::sync::Arc::new(BTreeMap::new()),
            id_to_item: std::sync::Arc::new(BTreeMap::new()),
        };
        let path = std::env::temp_dir().join(format!("refused-{}.json", random_token(8)));
        let submit = |error: fn() -> PlanValidationError| {
            execute_plan_with(
                &path,
                &unlocked,
                PlanRequest {
                    items: vec![plan_item("one", None, None), plan_item("two", None, None)],
                },
                |path, _, pending| {
                    run_pending_with(
                        path,
                        pending,
                        |_| Err(error()),
                        |_, _| panic!("a refused batch must not send"),
                    )
                },
            )
        };

        let response = submit(|| PlanValidationError::Invalid("Inventory changed".into()));
        assert!(response
            .results
            .iter()
            .all(|row| row.status == STATUS_ERROR
                && row.message.as_deref() == Some("Inventory changed")));
        let saved = crate::orders::pending::load_pending(&path)
            .unwrap()
            .unwrap();
        clear_finished_pending(&path, &saved).unwrap();
        assert!(matches!(journal_state(&path), JournalState::Absent));

        let response = submit(|| wfm_client::governor::AccessError::Cooldown(30).into());
        assert!(response
            .results
            .iter()
            .all(|row| row.status == STATUS_PENDING));
        let saved = crate::orders::pending::load_pending(&path)
            .unwrap()
            .unwrap();
        clear_finished_pending(&path, &saved).unwrap();
        assert!(matches!(journal_state(&path), JournalState::Unfinished));
        let _ = std::fs::remove_file(&path);
    }

    /// Each send reads the account's orders once, and the check right before it
    /// uses that read; only the check before the batch may read on its own.
    #[test]
    fn each_send_costs_one_orders_read() {
        let reads = std::cell::Cell::new(0);
        // A validator that reads only when it is not handed a body, which is
        // what the desktop's does.
        let validator = |orders: Option<&serde_json::Value>| {
            if orders.is_none() {
                reads.set(reads.get() + 1);
            }
            Ok(())
        };
        let mut pending: PendingPlan = serde_json::from_value(serde_json::json!({
            "plan_id":"reads", "started_at":"test", "items":(0..3).map(|index| serde_json::json!({
                "slug":format!("item-{index}"), "quantity":1, "platinum":12,
                "order_type":"sell", "visible":false, "status":"pending"
            })).collect::<Vec<_>>()
        }))
        .unwrap();
        let path = std::env::temp_dir().join(format!("reads-{}.json", random_token(8)));
        run_pending_with(&path, &mut pending, validator, |item, validate| {
            reads.set(reads.get() + 1);
            let body = serde_json::json!({"data": []});
            validate(Some(&body)).unwrap();
            ItemResult {
                slug: item.slug.clone(),
                status: "ok".into(),
                message: None,
                order_id: Some(item.slug.clone()),
                action: Some("created".into()),
            }
        });
        assert_eq!(reads.get(), 1 + 3);
        let _ = std::fs::remove_file(&path);

        let item = plan_item("waiting", None, None);
        let session = Unlocked {
            jwt: "fixture".into(),
            username: "fixture".into(),
            platform: "pc".into(),
            catalog: std::sync::Arc::new(BTreeMap::from([(
                "waiting".into(),
                cat("item", None, &[]),
            )])),
            id_to_item: std::sync::Arc::new(BTreeMap::new()),
        };
        let http = Client::builder()
            .retry(reqwest::retry::never())
            .redirect(wfm_client::redirect_policy())
            .proxy(reqwest::Proxy::all("http://127.0.0.1:9").unwrap())
            .timeout(Duration::from_millis(100))
            .build()
            .unwrap();
        reads.set(0);
        let mut validated_against = None;
        let result = execute_one(
            &http,
            &session,
            &item,
            &mut || {
                reads.set(reads.get() + 1);
                Ok(serde_json::json!({"data": [], "read": reads.get()}))
            },
            &mut |orders| {
                validated_against = orders.cloned();
                Err("stop before sending".into())
            },
        );
        assert_eq!(result.status, STATUS_ERROR);
        assert_eq!(reads.get(), 1);
        assert_eq!(
            validated_against,
            Some(serde_json::json!({"data": [], "read": 1}))
        );
    }

    #[test]
    fn crash_after_send_keeps_a_final_or_middle_item_out_of_retryable_pending() {
        for count in [1, 3] {
            let path = std::env::temp_dir().join(format!("durable-crash-{}.json", random_token(8)));
            let mut pending: PendingPlan = serde_json::from_value(serde_json::json!({
                "plan_id":"crash", "started_at":"test", "items":(0..count).map(|index| serde_json::json!({
                    "slug":format!("item-{index}"), "quantity":1, "platinum":12,
                    "order_type":"sell", "visible":false, "status":"pending"
                })).collect::<Vec<_>>()
            })).unwrap();
            write_pending_atomic(&path, &pending).unwrap();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_pending_with(
                    &path,
                    &mut pending,
                    |_| Ok(()),
                    |item, _| {
                        if item.slug == format!("item-{}", count - 1) {
                            panic!("simulated crash after remote application");
                        }
                        ItemResult {
                            slug: item.slug.clone(),
                            status: "ok".into(),
                            message: None,
                            order_id: Some(item.slug.clone()),
                            action: Some("created".into()),
                        }
                    },
                )
            }));
            assert!(result.is_err());
            let mut restored = crate::orders::pending::load_pending(&path)
                .unwrap()
                .unwrap();
            assert_eq!(
                restored.items[count - 1].status,
                crate::orders::pending::STATUS_UNCERTAIN
            );
            let mut resent = Vec::new();
            run_pending_with(
                &path,
                &mut restored,
                |_| Ok(()),
                |item, _| {
                    resent.push(item.slug.clone());
                    ItemResult {
                        slug: item.slug.clone(),
                        status: "ok".into(),
                        message: None,
                        order_id: None,
                        action: None,
                    }
                },
            );
            assert!(resent.is_empty());
            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn failed_result_rewrite_reports_known_outcome_and_preserves_disk_marker() {
        for count in [1, 3] {
            let path =
                std::env::temp_dir().join(format!("durable-rewrite-{}.json", random_token(8)));
            let mut pending: PendingPlan = serde_json::from_value(serde_json::json!({
                "plan_id":"rewrite", "started_at":"test", "items":(0..count).map(|index| serde_json::json!({
                    "slug":format!("item-{index}"), "quantity":1, "platinum":12,
                    "order_type":"sell", "visible":false, "status":"pending"
                })).collect::<Vec<_>>()
            })).unwrap();
            write_pending_atomic(&path, &pending).unwrap();
            let mut writes = 0;
            let response = run_pending_with_store(
                &path,
                &mut pending,
                |_| Ok(()),
                |item, _| ItemResult {
                    slug: item.slug.clone(),
                    status: "ok".into(),
                    message: None,
                    order_id: Some(format!("remote-{}", item.slug)),
                    action: Some("created".into()),
                },
                |path, plan| {
                    writes += 1;
                    if writes == count * 2 {
                        anyhow::bail!("injected disk failure")
                    }
                    write_pending_atomic(path, plan)
                },
            );
            assert!(response
                .durability_error
                .as_deref()
                .unwrap()
                .contains("injected disk failure"));
            assert_eq!(response.results[count - 1].status, "ok");
            assert_eq!(
                response.results[count - 1].order_id.as_deref(),
                Some(format!("remote-item-{}", count - 1).as_str())
            );
            let restored = crate::orders::pending::load_pending(&path)
                .unwrap()
                .unwrap();
            assert_eq!(
                restored.items[count - 1].status,
                crate::orders::pending::STATUS_UNCERTAIN
            );
            assert!(!restored.is_finished());
            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn finished_in_memory_does_not_clear_an_unrecorded_final_result() {
        let path = std::env::temp_dir().join(format!("durable-final-{}.json", random_token(8)));
        let unlocked = Unlocked {
            jwt: "fixture".into(),
            username: "fixture".into(),
            platform: "pc".into(),
            catalog: std::sync::Arc::new(BTreeMap::new()),
            id_to_item: std::sync::Arc::new(BTreeMap::new()),
        };
        let response = execute_plan_with(
            &path,
            &unlocked,
            PlanRequest {
                items: vec![plan_item("final", None, None)],
            },
            |path, _, pending| {
                let mut writes = 0;
                run_pending_with_store(
                    path,
                    pending,
                    |_| Ok(()),
                    |item, _| ItemResult {
                        slug: item.slug.clone(),
                        status: "ok".into(),
                        message: None,
                        order_id: Some("remote-final".into()),
                        action: Some("created".into()),
                    },
                    |path, plan| {
                        writes += 1;
                        if writes == 2 {
                            anyhow::bail!("injected result write failure")
                        }
                        write_pending_atomic(path, plan)
                    },
                )
            },
        );
        assert_eq!(
            response.results[0].order_id.as_deref(),
            Some("remote-final")
        );
        assert!(response.durability_error.is_some());
        let saved = crate::orders::pending::load_pending(&path)
            .unwrap()
            .expect("journal retained");
        assert_eq!(
            saved.items[0].status,
            crate::orders::pending::STATUS_UNCERTAIN
        );
        let mut finished_in_memory = saved.clone();
        finished_in_memory.items[0].status = crate::orders::pending::STATUS_OK.into();
        clear_finished_pending(&path, &finished_in_memory).unwrap();
        assert!(
            path.exists(),
            "an uncertain disk marker must survive a finished memory snapshot"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn failed_pre_send_marker_stops_before_remote_mutation() {
        let path = std::env::temp_dir().join(format!("durable-presend-{}.json", random_token(8)));
        let mut pending: PendingPlan = serde_json::from_value(serde_json::json!({
            "plan_id":"presend", "started_at":"test", "items":[
                {"slug":"unsent","quantity":1,"platinum":12,"order_type":"sell","visible":false,"status":"pending"}
            ]
        })).unwrap();
        write_pending_atomic(&path, &pending).unwrap();
        let mut sent = false;
        let response = run_pending_with_store(
            &path,
            &mut pending,
            |_| Ok(()),
            |item, _| {
                sent = true;
                ItemResult {
                    slug: item.slug.clone(),
                    status: "ok".into(),
                    message: None,
                    order_id: None,
                    action: None,
                }
            },
            |_, _| anyhow::bail!("injected marker write failure"),
        );
        assert!(!sent);
        assert!(response
            .durability_error
            .as_deref()
            .unwrap()
            .contains("before sending"));
        assert_eq!(
            response.results[0].status,
            crate::orders::pending::STATUS_PENDING
        );
        assert_eq!(
            crate::orders::pending::load_pending(&path)
                .unwrap()
                .unwrap()
                .items[0]
                .status,
            crate::orders::pending::STATUS_PENDING
        );
        let _ = std::fs::remove_file(&path);
    }

    /// The whole path an orders response takes to become a reconciliation
    /// index, starting from the body the endpoint would return. A catalogue
    /// constrains nothing here so a test states the row shape alone.
    fn index_orders_body(
        body: &serde_json::Value,
    ) -> anyhow::Result<BTreeMap<OrderKey, ExistingOrder>> {
        Ok(index_existing_orders(
            &crate::orders::decode::decode_orders(body, &Unconstrained)?,
        ))
    }

    struct Unconstrained;

    impl market_domain::orders::ItemConstraints for Unconstrained {
        fn accepts(&self, _item_id: &str, _rank: Option<u64>, _subtype: Option<&str>) -> bool {
            true
        }
    }

    fn key(item_id: &str, side: OrderSide, rank: Option<u64>, subtype: Option<&str>) -> OrderKey {
        OrderKey {
            item_id: item_id.into(),
            side,
            rank,
            subtype: subtype.map(str::to_string),
        }
    }

    #[test]
    fn uncertain_only_or_final_send_survives_execution_and_restart() {
        use crate::orders::pending::{journal_state, load_pending, JournalState, STATUS_UNCERTAIN};

        let unlocked = Unlocked {
            jwt: "fixture".into(),
            username: "fixture".into(),
            platform: "pc".into(),
            catalog: std::sync::Arc::new(BTreeMap::new()),
            id_to_item: std::sync::Arc::new(BTreeMap::new()),
        };
        for item_count in [1, 2] {
            let path =
                std::env::temp_dir().join(format!("uncertain-boundary-{}.json", random_token(8)));
            let items = (0..item_count)
                .map(|index| plan_item(&format!("item-{index}"), None, None))
                .collect();
            let response = execute_plan_with(
                &path,
                &unlocked,
                PlanRequest { items },
                |path, _, pending| {
                    run_pending_with(
                        path,
                        pending,
                        |_| Ok(()),
                        |item, _| ItemResult {
                            slug: item.slug.clone(),
                            status: if item.slug == format!("item-{}", item_count - 1) {
                                STATUS_UNCERTAIN.into()
                            } else {
                                "ok".into()
                            },
                            message: None,
                            order_id: None,
                            action: None,
                        },
                    )
                },
            );
            assert_eq!(response.results.last().unwrap().status, STATUS_UNCERTAIN);
            let mut restarted = load_pending(&path)
                .unwrap()
                .expect("unresolved journal survives");
            assert!(matches!(journal_state(&path), JournalState::Unfinished));
            let replacement = execute_plan_with(
                &path,
                &unlocked,
                PlanRequest {
                    items: vec![plan_item("replacement", None, None)],
                },
                |_, _, _| panic!("an unresolved journal must block execution"),
            );
            assert_eq!(replacement.results[0].status, "error");
            assert_eq!(
                load_pending(&path).unwrap().unwrap().plan_id,
                restarted.plan_id
            );
            let mut resent = Vec::new();
            run_pending_with(
                &path,
                &mut restarted,
                |_| Ok(()),
                |item, _| {
                    resent.push(item.slug.clone());
                    ItemResult {
                        slug: item.slug.clone(),
                        status: "ok".into(),
                        message: None,
                        order_id: None,
                        action: None,
                    }
                },
            );
            clear_finished_pending(&path, &restarted).unwrap();
            assert!(
                resent.is_empty(),
                "restart must not replay an uncertain send"
            );
            assert!(matches!(journal_state(&path), JournalState::Unfinished));
            let _ = std::fs::remove_file(&path);
        }
    }

    /// A send whose outcome is unknown is not the same as a send that never
    /// happened. The governor says so - `pending` means "saved for explicit
    /// resume" while `uncertain_mutation` means "refresh and reconcile before
    /// resending" - and the journal has to keep that distinction, because the
    /// resume loop only re-offers what it believes was never sent.
    ///
    /// Collapsing the two made the journal claim "never sent" about a request
    /// that may already have been applied, and let the plan be cleared while
    /// that claim stood.
    #[test]
    fn an_uncertain_send_stays_uncertain_in_the_journal_and_is_not_reoffered() {
        let path = std::env::temp_dir().join(format!("uncertain-{}.json", random_token(8)));
        let mut pending: PendingPlan = serde_json::from_value(serde_json::json!({
            "plan_id":"uncertain", "started_at":"test", "items":[
                {"slug":"ambiguous","quantity":1,"platinum":12,"order_type":"sell","visible":false,"status":"pending"},
                {"slug":"later","quantity":1,"platinum":12,"order_type":"sell","visible":false,"status":"pending"}
            ]
        })).unwrap();
        write_pending_atomic(&path, &pending).unwrap();

        let mut attempted: Vec<String> = Vec::new();
        let response = run_pending_with(
            &path,
            &mut pending,
            |_| Ok(()),
            |item, _validate| {
                attempted.push(item.slug.clone());
                ItemResult {
                    slug: item.slug.clone(),
                    status: "uncertain_mutation".into(),
                    message: Some(
                        "The listing may have been applied, but its response could not be read."
                            .into(),
                    ),
                    order_id: None,
                    action: None,
                }
            },
        );

        // Only the first item was attempted: an unresolved outcome stops the
        // batch rather than sending later items ahead of it.
        assert_eq!(attempted, ["ambiguous"]);
        assert_eq!(
            response
                .results
                .iter()
                .map(|row| row.status.as_str())
                .collect::<Vec<_>>(),
            ["uncertain_mutation", "pending"]
        );

        // The journal records what actually happened, and the later item is
        // still waiting.
        let saved = crate::orders::pending::load_pending(&path)
            .expect("journal readable")
            .expect("journal kept");
        assert_eq!(
            saved.items[0].status, "uncertain_mutation",
            "an unknown outcome must not be recorded as never-sent"
        );
        assert_eq!(saved.items[1].status, "pending");

        // And a resume must not re-send it as though nothing had been sent.
        let mut pending = crate::orders::pending::load_pending(&path)
            .unwrap()
            .unwrap();
        let mut resumed: Vec<String> = Vec::new();
        run_pending_with(
            &path,
            &mut pending,
            |_| Ok(()),
            |item, _validate| {
                resumed.push(item.slug.clone());
                ItemResult {
                    slug: item.slug.clone(),
                    status: "ok".into(),
                    message: None,
                    order_id: Some("o".into()),
                    action: None,
                }
            },
        );
        // The unresolved item is skipped; the rest of the batch is not held
        // behind it. Collapsing to `pending` did the opposite: it re-offered the
        // ambiguous send AND blocked every later item behind a decision nobody
        // had made.
        assert_eq!(
            resumed,
            ["later"],
            "an unresolved outcome is left for reconciliation, and does not block the batch"
        );
        clear_finished_pending(&path, &pending).unwrap();
        assert!(matches!(journal_state(&path), JournalState::Unfinished));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn interrupted_final_validation_persists_unsent_items_for_explicit_resume() {
        use wfm_client::governor::AccessError;
        for error in [
            AccessError::Cancelled,
            AccessError::Cooldown(1),
            AccessError::Paused("Maintenance".into()),
            AccessError::Busy,
            AccessError::Transport,
            AccessError::Persistence,
            AccessError::Http(503),
        ] {
            let path =
                std::env::temp_dir().join(format!("validation-resume-{}.json", random_token(8)));
            let mut pending: PendingPlan = serde_json::from_value(serde_json::json!({
                "plan_id":"resume", "started_at":"test", "items":[
                    {"slug":"finished","quantity":1,"platinum":12,"order_type":"sell","visible":false,"status":"ok","order_id":"existing"},
                    {"slug":"waiting","quantity":1,"platinum":12,"order_type":"sell","visible":false,"status":"pending"},
                    {"slug":"later","quantity":1,"platinum":12,"order_type":"sell","visible":false,"status":"pending"}
                ]
            })).unwrap();
            write_pending_atomic(&path, &pending).unwrap();
            let session = Unlocked {
                jwt: "fixture".into(),
                username: "fixture".into(),
                platform: "pc".into(),
                catalog: std::sync::Arc::new(BTreeMap::from([(
                    "waiting".into(),
                    cat("item", None, &[]),
                )])),
                id_to_item: std::sync::Arc::new(BTreeMap::new()),
            };
            let http = Client::builder()
                .proxy(reqwest::Proxy::all("http://127.0.0.1:9").unwrap())
                .timeout(Duration::from_millis(100))
                .build()
                .unwrap();
            let validating_order = std::cell::Cell::new(false);
            let mut attempted = vec![];
            let response = run_pending_with(
                &path,
                &mut pending,
                |_| {
                    if validating_order.get() {
                        Err(error.clone().into())
                    } else {
                        Ok(())
                    }
                },
                |item, validate| {
                    attempted.push(item.slug.clone());
                    execute_one(
                        &http,
                        &session,
                        item,
                        &mut || {
                            validating_order.set(true);
                            Ok(serde_json::json!({"data": []}))
                        },
                        validate,
                    )
                },
            );
            assert_eq!(attempted, ["waiting"]);
            assert_eq!(
                response
                    .results
                    .iter()
                    .map(|row| row.status.as_str())
                    .collect::<Vec<_>>(),
                ["ok", "pending", "pending"]
            );
            let mut saved = crate::orders::pending::load_pending(&path)
                .expect("readable")
                .expect("present");
            assert_eq!(saved.items[1].status, "pending");
            assert_eq!(
                saved.items[1].message.as_deref(),
                Some(error.to_string().as_str())
            );
            assert_eq!(saved.items[0].order_id.as_deref(), Some("existing"));

            let mut resumed = vec![];
            let validations = std::cell::Cell::new(0);
            let response = run_pending_with(
                &path,
                &mut saved,
                |_| {
                    validations.set(validations.get() + 1);
                    Ok(())
                },
                |item, validate| {
                    validate(None).unwrap();
                    resumed.push(item.slug.clone());
                    ItemResult {
                        slug: item.slug.clone(),
                        status: "ok".into(),
                        message: None,
                        order_id: Some(item.slug.clone()),
                        action: Some("created".into()),
                    }
                },
            );
            assert_eq!(resumed, ["waiting", "later"]);
            assert!(validations.get() >= resumed.len());
            assert!(response.results.iter().all(|item| item.status == "ok"));
            assert!(crate::orders::pending::load_pending(&path)
                .expect("readable")
                .expect("present")
                .items
                .iter()
                .all(|item| item.status == "ok"));
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn final_inventory_validation_errors_remain_terminal() {
        let item = plan_item("waiting", None, None);
        let session = Unlocked {
            jwt: "fixture".into(),
            username: "fixture".into(),
            platform: "pc".into(),
            catalog: std::sync::Arc::new(BTreeMap::from([(
                "waiting".into(),
                cat("item", None, &[]),
            )])),
            id_to_item: std::sync::Arc::new(BTreeMap::new()),
        };
        let http = Client::builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:9").unwrap())
            .timeout(Duration::from_millis(100))
            .build()
            .unwrap();
        let response = execute_one(
            &http,
            &session,
            &item,
            &mut || Ok(serde_json::json!({"data": []})),
            &mut |_| Err("Inventory changed".into()),
        );
        assert_eq!(response.status, "error");
        assert_eq!(response.message.as_deref(), Some("Inventory changed"));
    }

    #[test]
    fn transient_validation_failure_stays_interrupted_after_the_cooldown_ends() {
        let mut pending: PendingPlan = serde_json::from_value(serde_json::json!({
            "plan_id":"test", "started_at":"test", "items":[
                {"slug":"waiting","quantity":1,"platinum":12,"order_type":"sell","visible":false,"status":"pending"}
            ]
        })).unwrap();
        let response = validation_failure(
            std::path::Path::new("unused"),
            &mut pending,
            wfm_client::governor::AccessError::Cooldown(0),
            &mut |_, _| -> anyhow::Result<()> { panic!("an interruption rewrites nothing") },
        );
        assert_eq!(response.results[0].status, "pending");
        assert_eq!(pending.items[0].status, "pending");
        assert_eq!(response.results[0].slug, "waiting");
    }

    #[test]
    fn review_rejects_any_changed_or_ambiguous_order_detail() {
        let expected = ReviewedOrder::Existing {
            id: "order".into(),
            platinum: 20,
            quantity: 6,
            per_trade: Some(3),
            visible: true,
        };
        let mut prior = ExistingOrder {
            id: "order".into(),
            platinum: 20,
            quantity: 6,
            per_trade: Some(3),
            visible: Some(true),
            ambiguous: false,
        };
        assert!(review_matches(&expected, Some(&prior)));
        prior.visible = Some(false);
        assert!(!review_matches(&expected, Some(&prior)));
        prior.visible = Some(true);
        prior.quantity = 3;
        assert!(!review_matches(&expected, Some(&prior)));
        prior.quantity = 6;
        prior.per_trade = Some(1);
        assert!(!review_matches(&expected, Some(&prior)));
        prior.per_trade = Some(3);
        prior.platinum = 21;
        assert!(!review_matches(&expected, Some(&prior)));
        prior.platinum = 20;
        prior.ambiguous = true;
        assert!(!review_matches(&expected, Some(&prior)));
        assert!(!review_matches(&expected, None));
        assert!(review_matches(&ReviewedOrder::New, None));
        assert!(!review_matches(&ReviewedOrder::New, Some(&prior)));
    }

    #[test]
    fn duplicate_order_keys_remain_ambiguous() {
        let orders = index_orders_body(&serde_json::json!({"data":[
            {"id":"one","itemId":"item","type":"sell","quantity":1,"platinum":10},
            {"id":"two","itemId":"item","type":"sell","quantity":2,"platinum":20}
        ]}))
        .unwrap();
        assert!(orders.values().next().unwrap().ambiguous);
    }

    /// A resume refused by validation sends nothing and records the unsent
    /// items as refused: the same check would refuse every later resume, and a
    /// journal left pending would block new batches until it was discarded.
    #[test]
    fn failed_recovery_revalidation_refuses_pending_items_without_sending() {
        let mut pending: PendingPlan = serde_json::from_value(serde_json::json!({
            "plan_id":"test", "started_at":"test", "items":[
                {"slug":"example","quantity":1,"platinum":12,"order_type":"sell","visible":false,"status":"pending"}
            ]
        })).unwrap();
        let unlocked = Unlocked {
            jwt: "test".into(),
            username: "test".into(),
            platform: "pc".into(),
            catalog: std::sync::Arc::new(BTreeMap::new()),
            id_to_item: std::sync::Arc::new(BTreeMap::new()),
        };
        let path = std::env::temp_dir().join(format!("not-submitted-{}", random_token(8)));
        let result = run_pending(&path, &unlocked, &mut pending, |_| {
            Err("Inventory changed".into())
        });
        assert_eq!(
            result.results[0].message.as_deref(),
            Some("Inventory changed")
        );
        assert_eq!(result.results[0].status, STATUS_ERROR);
        assert!(result.results[0].order_id.is_none());
        let saved = crate::orders::pending::load_pending(&path)
            .unwrap()
            .unwrap();
        assert_eq!(saved.items[0].status, STATUS_ERROR);
        assert!(saved.is_finished());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn reviewed_lots_match_validation_and_wire_fixture() {
        #[derive(Deserialize)]
        struct Case {
            name: String,
            quantity: u32,
            per_trade: Option<u32>,
            bulk_tradable: bool,
            valid: bool,
            create_lot: Option<u32>,
            patch_lot: Option<u32>,
        }
        let cases: Vec<Case> = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/trade-session/lots.json"
        ))
        .unwrap();
        for case in cases {
            let mut item = plan_item("example", None, None);
            item.quantity = case.quantity;
            item.per_trade = case.per_trade;
            let mut catalog = cat("example", None, &[]);
            catalog.bulk_tradable = case.bulk_tradable;
            assert_eq!(
                validate_lot(item.quantity, item.per_trade, catalog.bulk_tradable).is_ok(),
                case.valid,
                "{}",
                case.name
            );
            if !case.valid {
                continue;
            }
            let create = build_order_body(&item, &catalog);
            let patch = build_order_patch(&item, &catalog, item.per_trade.unwrap_or(1));
            assert_eq!(
                create.get("perTrade").and_then(|v| v.as_u64()),
                case.create_lot.map(u64::from),
                "{}",
                case.name
            );
            assert_eq!(
                patch.get("perTrade").and_then(|v| v.as_u64()),
                case.patch_lot.map(u64::from),
                "{}",
                case.name
            );
            assert_eq!(
                patch["quantity"], item.quantity,
                "replacement, not addition"
            );
            assert!(
                patch.get("visible").is_none(),
                "preserve existing visibility"
            );
            assert_eq!(create["visible"], false);
            assert_eq!(
                create["platinum"],
                item.platinum * case.create_lot.unwrap_or(1),
                "create uses the total lot price"
            );
            assert_eq!(
                patch["platinum"],
                item.platinum * item.per_trade.unwrap_or(1),
                "patch uses the total lot price"
            );
        }
    }

    #[test]
    fn plan_request_accepts_legacy_and_explicit_lots() {
        let legacy = r#"{"items":[{"slug":"example","platinum":12,"quantity":12,"order_type":"sell","visible":false}]}"#;
        let request: PlanRequest = serde_json::from_str(legacy).unwrap();
        assert_eq!(request.items[0].per_trade, None);
        let mut explicit: serde_json::Value = serde_json::from_str(legacy).unwrap();
        explicit["items"][0]["per_trade"] = serde_json::json!(3);
        let request: PlanRequest = serde_json::from_value(explicit).unwrap();
        assert_eq!(request.items[0].per_trade, Some(3));
    }

    #[test]
    fn invalid_lot_is_rejected_before_market_access() {
        let mut item = plan_item("example", None, None);
        item.quantity = 7;
        item.per_trade = Some(3);
        let catalog = cat("example", None, &[]);
        let unlocked = Unlocked {
            jwt: "test".into(),
            username: "test".into(),
            platform: "pc".into(),
            catalog: std::sync::Arc::new(BTreeMap::from([("example".into(), catalog)])),
            id_to_item: std::sync::Arc::new(BTreeMap::new()),
        };
        // Even a validation regression must never send a unit test to WFM.
        let http = Client::builder()
            .retry(reqwest::retry::never())
            .redirect(wfm_client::redirect_policy())
            .proxy(reqwest::Proxy::all("http://127.0.0.1:9").unwrap())
            .timeout(Duration::from_millis(100))
            .build()
            .unwrap();
        let mut reads = 0;
        let result = execute_one(
            &http,
            &unlocked,
            &item,
            &mut || {
                reads += 1;
                Ok(serde_json::json!({"data": []}))
            },
            &mut |_| Ok(()),
        );
        assert_eq!(reads, 0);
        assert_eq!(result.status, "error");
        assert!(result.message.unwrap().contains("divide quantity"));
        assert!(result.order_id.is_none());
        assert!(result.action.is_none());
    }

    #[test]
    fn index_existing_orders_reads_both_response_shapes() {
        // Bucketed shape: type comes from the bucket name.
        let bucketed = serde_json::json!({
            "data": {
                "sell": [
                    {"id": "o1", "itemId": "item-a", "platinum": 20, "quantity": 3, "rank": 0},
                ],
                "buy": [
                    {"id": "o2", "itemId": "item-a", "platinum": 5, "quantity": 1, "rank": 0},
                ]
            }
        });
        let idx = index_orders_body(&bucketed).unwrap();
        let sell = idx
            .get(&key("item-a", OrderSide::Sell, Some(0), None))
            .unwrap();
        assert_eq!(
            (sell.id.as_str(), sell.platinum, sell.quantity),
            ("o1", 20, 3)
        );
        assert!(idx.contains_key(&key("item-a", OrderSide::Buy, Some(0), None)));

        // Flat shape: type is a field on the order.
        let flat = serde_json::json!({
            "data": [
                {"id": "o3", "itemId": "item-b", "type": "sell", "platinum": 9, "quantity": 2,
                 "subtype": "radiant"},
            ]
        });
        let idx = index_orders_body(&flat).unwrap();
        assert!(idx.contains_key(&key("item-b", OrderSide::Sell, None, Some("radiant"))));
    }

    #[test]
    fn plan_item_key_mirrors_build_order_body_defaults() {
        // Ranked item, no explicit rank: the body sends rank 0, so the key
        // must say Some(0) - that's the order WFM would call a duplicate.
        let ranked = WfmCatalogItem {
            item_id: "item-r".into(),
            bulk_tradable: false,
            session_supported: true,
            display_name: "Some Arcane".into(),
            max_rank: Some(5),
            subtypes: vec![],
        };
        let item = PlanItem {
            inventory_snapshot_id: None,
            slug: "some_arcane".into(),
            platinum: 20,
            quantity: 1,
            per_trade: None,
            session: None,
            reviewed_order: None,
            order_type: "sell".into(),
            visible: false,
            rank: None,
            subtype: None,
            reference_low_sell: None,
        };
        let key = plan_item_key(&item, &ranked);
        let body = build_order_body(&item, &ranked);
        assert_eq!(key.rank, body.get("rank").and_then(|v| v.as_u64()));
        assert_eq!(key.subtype, None);

        // Subtyped item, bogus requested subtype: both fall back to the
        // catalog's first entry.
        let relic = WfmCatalogItem {
            item_id: "item-s".into(),
            bulk_tradable: true,
            session_supported: false,
            display_name: "Axi A1 Relic".into(),
            max_rank: None,
            subtypes: vec!["intact".into(), "radiant".into()],
        };
        let item = PlanItem {
            inventory_snapshot_id: None,
            subtype: Some("nonsense".into()),
            ..item
        };
        let key = plan_item_key(&item, &relic);
        let body = build_order_body(&item, &relic);
        assert_eq!(key.rank, None);
        assert_eq!(
            key.subtype.as_deref(),
            body.get("subtype").and_then(|v| v.as_str())
        );
        assert_eq!(key.subtype.as_deref(), Some("intact"));
    }

    fn cat(name: &str, max_rank: Option<u32>, subtypes: &[&str]) -> WfmCatalogItem {
        WfmCatalogItem {
            item_id: format!("id-{name}"),
            bulk_tradable: true,
            session_supported: true,
            display_name: name.into(),
            max_rank,
            subtypes: subtypes.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn plan_item(slug: &str, rank: Option<u32>, subtype: Option<&str>) -> PlanItem {
        PlanItem {
            inventory_snapshot_id: None,
            slug: slug.into(),
            platinum: 12,
            quantity: 3,
            per_trade: None,
            session: None,
            reviewed_order: None,
            order_type: "sell".into(),
            visible: false,
            rank,
            subtype: subtype.map(|s| s.into()),
            reference_low_sell: None,
        }
    }

    #[test]
    fn order_body_for_relic_includes_subtype_omits_rank() {
        // Reproducer for the May 2026 400: {"rank":"app.field.notAllowed",
        // "subtype":"app.field.required","perTrade":"app.field.required"}.
        //
        // The four refinements below (here and at the other `cat(...,
        // &["intact", ...])` call sites in this module) are test data only -
        // production reads subtypes[] from WFM's live catalog. Update them if
        // WFM ever adds a refinement tier.
        let cat = cat(
            "neo_b2_relic",
            None,
            &["intact", "exceptional", "flawless", "radiant"],
        );
        let item = plan_item("neo_b2_relic", None, None);
        let body = build_order_body(&item, &cat);
        assert_eq!(body["itemId"], "id-neo_b2_relic");
        assert_eq!(body["type"], "sell");
        assert_eq!(body["platinum"], 36);
        assert_eq!(body["quantity"], 3);
        assert_eq!(body["visible"], false);
        assert_eq!(body["perTrade"], 3);
        assert_eq!(body["subtype"], "intact"); // default to first
        assert!(
            body.get("rank").is_none(),
            "rank must be absent for non-rankable items"
        );
    }

    #[test]
    fn order_body_for_mod_includes_rank_omits_subtype() {
        let cat = cat("creeping_bullseye", Some(5), &[]);
        let item = plan_item("creeping_bullseye", None, None);
        let body = build_order_body(&item, &cat);
        assert_eq!(body["rank"], 0); // default for unmaxed
        assert!(body.get("subtype").is_none());
    }

    #[test]
    fn order_body_respects_explicit_rank_for_mods() {
        let cat = cat("creeping_bullseye", Some(5), &[]);
        let item = plan_item("creeping_bullseye", Some(5), None);
        let body = build_order_body(&item, &cat);
        assert_eq!(body["rank"], 5);
    }

    #[test]
    fn order_body_uses_user_subtype_when_valid() {
        let cat = cat(
            "neo_b2_relic",
            None,
            &["intact", "exceptional", "flawless", "radiant"],
        );
        let item = plan_item("neo_b2_relic", None, Some("radiant"));
        let body = build_order_body(&item, &cat);
        assert_eq!(body["subtype"], "radiant");
    }

    #[test]
    fn order_body_falls_back_to_first_when_user_subtype_invalid() {
        // Don't silently send a bogus subtype WFM will reject.
        let cat = cat(
            "neo_b2_relic",
            None,
            &["intact", "exceptional", "flawless", "radiant"],
        );
        let item = plan_item("neo_b2_relic", None, Some("super-radiant"));
        let body = build_order_body(&item, &cat);
        assert_eq!(body["subtype"], "intact");
    }

    #[test]
    fn per_trade_picks_largest_divisor_under_cap() {
        // Reproducer for `app.field.orders.perTradeMustDivideQuantity` -
        // WFM rejects when perTrade does not evenly divide quantity.
        assert_eq!(per_trade_for(27), 3); // {1,3,9,27} ∩ ≤6 → 3
        assert_eq!(per_trade_for(10), 5); // {1,2,5,10} ∩ ≤6 → 5
        assert_eq!(per_trade_for(12), 6); // {1,2,3,4,6,12} ∩ ≤6 → 6
        assert_eq!(per_trade_for(6), 6); // exact fit
        assert_eq!(per_trade_for(7), 1); // prime > 6 → only 1 divides
        assert_eq!(per_trade_for(11), 1); // prime > 6 → 1
        assert_eq!(per_trade_for(1), 1);
        assert_eq!(per_trade_for(0), 1); // defensive
    }

    #[test]
    fn order_body_per_trade_divides_quantity_for_27_relic_stack() {
        // Reproducer for the May 2026 400 on a 27-relic stack:
        // {"inputs":{"perTrade":"app.field.orders.perTradeMustDivideQuantity"}}.
        // perTrade must EVENLY DIVIDE quantity. Largest divisor of 27 ≤ 6 is 3.
        let cat = cat(
            "neo_b2_relic",
            None,
            &["intact", "exceptional", "flawless", "radiant"],
        );
        let mut item = plan_item("neo_b2_relic", None, None);
        item.quantity = 27;
        let body = build_order_body(&item, &cat);
        assert_eq!(body["quantity"], 27);
        assert_eq!(body["perTrade"], 3);
        // Sanity: 27 must divide perfectly.
        assert_eq!(
            body["quantity"].as_u64().unwrap() % body["perTrade"].as_u64().unwrap(),
            0
        );
    }

    #[test]
    fn order_body_per_trade_uses_quantity_when_quantity_under_cap() {
        let cat = cat("neo_b2_relic", None, &["intact"]);
        let mut item = plan_item("neo_b2_relic", None, None);
        item.quantity = 3;
        let body = build_order_body(&item, &cat);
        assert_eq!(body["perTrade"], 3);
    }

    #[test]
    fn pending_and_uncertain_results_match_the_shared_frontend_contract() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/wfm-access/outcomes.json"
        ))
        .unwrap();
        let item = plan_item("accelerated_blast", None, None);
        let session = Unlocked {
            jwt: "fixture".into(),
            username: "fixture".into(),
            platform: "pc".into(),
            catalog: std::sync::Arc::new(BTreeMap::from([(
                "accelerated_blast".into(),
                cat("item", None, &[]),
            )])),
            id_to_item: std::sync::Arc::new(BTreeMap::new()),
        };
        let http = Client::builder()
            .retry(reqwest::retry::never())
            .redirect(wfm_client::redirect_policy())
            .proxy(reqwest::Proxy::all("http://127.0.0.1:9").unwrap())
            .timeout(Duration::from_millis(100))
            .build()
            .unwrap();
        let pending = execute_one(
            &http,
            &session,
            &item,
            &mut || Err(anyhow::anyhow!("offline")),
            &mut |_| Ok(()),
        );
        assert_eq!(
            serde_json::to_value(pending).unwrap()["status"],
            fixture["pending"]["status"]
        );
        let uncertain = execute_one(
            &http,
            &session,
            &item,
            &mut || Ok(serde_json::json!({"data": []})),
            &mut |_| Ok(()),
        );
        assert_eq!(
            serde_json::to_value(uncertain).unwrap()["status"],
            fixture["uncertain_mutation"]["status"]
        );
    }

    #[test]
    fn slug_mismatch_guard_refuses_the_wrong_side_of_the_market_per_order_side() {
        let session = Unlocked {
            jwt: "fixture".into(),
            username: "fixture".into(),
            platform: "pc".into(),
            catalog: std::sync::Arc::new(BTreeMap::from([(
                "guarded".into(),
                cat("guarded", None, &[]),
            )])),
            id_to_item: std::sync::Arc::new(BTreeMap::new()),
        };
        let http = Client::builder()
            .retry(reqwest::retry::never())
            .redirect(wfm_client::redirect_policy())
            .proxy(reqwest::Proxy::all("http://127.0.0.1:9").unwrap())
            .timeout(Duration::from_millis(100))
            .build()
            .unwrap();
        for (side, platinum, reference, refused) in [
            ("sell", 10, 31, true),
            ("sell", 10, 30, false),
            ("sell", 100, 31, false),
            ("buy", 10, 31, false),
            ("buy", 94, 31, true),
            ("buy", 93, 31, false),
        ] {
            let mut item = plan_item("guarded", None, None);
            item.order_type = side.into();
            item.platinum = platinum;
            item.reference_low_sell = Some(reference);
            let mut reads = 0;
            // Past the guard the order read fails, so nothing is ever sent.
            let result = execute_one(
                &http,
                &session,
                &item,
                &mut || {
                    reads += 1;
                    Err(anyhow::anyhow!("offline"))
                },
                &mut |_| Ok(()),
            );
            let case = format!("{side} {platinum}p against {reference}p");
            if refused {
                assert_eq!(result.status, STATUS_ERROR, "{case}");
                assert!(result.message.unwrap().contains("slug mismatch"), "{case}");
                assert_eq!(reads, 0, "{case}");
            } else {
                assert_eq!(result.status, STATUS_PENDING, "{case}");
                assert_eq!(reads, 1, "{case}");
            }
        }
    }

    fn reconcile_catalog() -> BTreeMap<String, WfmCatalogItem> {
        let mut plain = cat("plain", None, &[]);
        plain.bulk_tradable = false;
        BTreeMap::from([
            ("plain".into(), plain),
            ("bulk".into(), cat("bulk", None, &[])),
        ])
    }

    fn uncertain_plan(items: Vec<serde_json::Value>) -> PendingPlan {
        let items = items
            .into_iter()
            .map(|mut item| {
                let defaults = serde_json::json!({
                    "slug":"plain", "quantity":2, "platinum":10, "order_type":"sell",
                    "visible":false, "status":"uncertain_mutation"
                });
                for (key, value) in defaults.as_object().unwrap() {
                    item.as_object_mut()
                        .unwrap()
                        .entry(key.clone())
                        .or_insert(value.clone());
                }
                item
            })
            .collect::<Vec<_>>();
        serde_json::from_value(serde_json::json!({
            "plan_id":"reconcile", "started_at":"test", "items":items
        }))
        .unwrap()
    }

    fn reconcile_one(
        item: serde_json::Value,
        orders: anyhow::Result<serde_json::Value>,
    ) -> PendingItem {
        let mut pending = uncertain_plan(vec![item]);
        let path = std::env::temp_dir().join(format!("reconcile-{}.json", random_token(8)));
        reconcile_pending(
            &path,
            &mut pending,
            &reconcile_catalog(),
            || crate::orders::decode::decode_orders(&orders?, &Unconstrained),
            write_pending_atomic,
        )
        .unwrap_or_else(|_| panic!("reconciliation could not be saved"));
        let saved = crate::orders::pending::load_pending(&path)
            .unwrap()
            .unwrap();
        std::fs::remove_file(&path).unwrap();
        saved.items.into_iter().next().unwrap()
    }

    fn order(id: &str, item: &str, platinum: u64, quantity: u64) -> serde_json::Value {
        serde_json::json!({"id":id, "itemId":format!("id-{item}"), "type":"sell",
            "platinum":platinum, "quantity":quantity, "visible":true})
    }

    #[test]
    fn resume_reconciles_unknown_outcomes_only_on_unambiguous_evidence() {
        let new = serde_json::json!({"reviewed_order":{"state":"new"}});
        let existing = serde_json::json!({"reviewed_order":{
            "state":"existing", "id":"o1", "platinum":8, "quantity":1, "per_trade":null, "visible":true
        }});
        let legacy = serde_json::json!({});
        let bulk_new = serde_json::json!({"slug":"bulk", "quantity":6, "per_trade":3,
            "reviewed_order":{"state":"new"}});
        let orders = |rows: Vec<serde_json::Value>| Ok(serde_json::json!({ "data": rows }));
        // Case, journal item, orders read, then the expected status, order id and action.
        type Case<'a> = (
            &'a str,
            serde_json::Value,
            anyhow::Result<serde_json::Value>,
            &'a str,
            Option<&'a str>,
            Option<&'a str>,
        );
        let cases: Vec<Case> = vec![
            (
                "create that did not land",
                new.clone(),
                orders(vec![]),
                STATUS_PENDING,
                None,
                None,
            ),
            (
                "create that landed",
                new.clone(),
                orders(vec![order("o1", "plain", 10, 2)]),
                STATUS_OK,
                Some("o1"),
                Some("created"),
            ),
            (
                "create with duplicates",
                new.clone(),
                orders(vec![
                    order("o1", "plain", 10, 2),
                    order("o2", "plain", 10, 2),
                ]),
                STATUS_UNCERTAIN,
                None,
                None,
            ),
            (
                "create with a different price",
                new.clone(),
                orders(vec![order("o1", "plain", 11, 2)]),
                STATUS_UNCERTAIN,
                None,
                None,
            ),
            (
                "bulk create that landed",
                bulk_new.clone(),
                orders(vec![{
                    let mut row = order("o1", "bulk", 30, 6);
                    row["perTrade"] = 3.into();
                    row
                }]),
                STATUS_OK,
                Some("o1"),
                Some("created"),
            ),
            (
                "bulk create with another lot",
                bulk_new,
                orders(vec![{
                    let mut row = order("o1", "bulk", 30, 6);
                    row["perTrade"] = 2.into();
                    row
                }]),
                STATUS_UNCERTAIN,
                None,
                None,
            ),
            (
                "update that landed",
                existing.clone(),
                orders(vec![order("o1", "plain", 10, 2)]),
                STATUS_OK,
                Some("o1"),
                Some("updated"),
            ),
            (
                "update that did not land",
                existing.clone(),
                orders(vec![order("o1", "plain", 8, 1)]),
                STATUS_PENDING,
                None,
                None,
            ),
            (
                "update half applied",
                existing.clone(),
                orders(vec![order("o1", "plain", 10, 1)]),
                STATUS_UNCERTAIN,
                None,
                None,
            ),
            (
                "updated order gone",
                existing.clone(),
                orders(vec![]),
                STATUS_UNCERTAIN,
                None,
                None,
            ),
            (
                "updated order replaced",
                existing,
                orders(vec![order("o2", "plain", 10, 2)]),
                STATUS_UNCERTAIN,
                None,
                None,
            ),
            (
                "older journal, no order",
                legacy.clone(),
                orders(vec![]),
                STATUS_UNCERTAIN,
                None,
                None,
            ),
            (
                "older journal, intended order",
                legacy.clone(),
                orders(vec![order("o1", "plain", 10, 2)]),
                STATUS_OK,
                Some("o1"),
                None,
            ),
            (
                "older journal, other order",
                legacy,
                orders(vec![order("o1", "plain", 8, 2)]),
                STATUS_UNCERTAIN,
                None,
                None,
            ),
            (
                "orders unreadable",
                new.clone(),
                orders(vec![
                    serde_json::json!({"itemId":"id-plain", "type":"sell"}),
                ]),
                STATUS_UNCERTAIN,
                None,
                None,
            ),
            (
                "orders read failed",
                new.clone(),
                Err(anyhow::anyhow!("offline")),
                STATUS_UNCERTAIN,
                None,
                None,
            ),
            (
                "item not in catalogue",
                serde_json::json!({"slug":"unknown", "reviewed_order":{"state":"new"}}),
                orders(vec![]),
                STATUS_UNCERTAIN,
                None,
                None,
            ),
        ];
        for (case, item, orders, status, order_id, action) in cases {
            let item = reconcile_one(item, orders);
            assert_eq!(item.status, status, "{case}");
            assert_eq!(item.order_id.as_deref(), order_id, "{case}");
            assert_eq!(item.action.as_deref(), action, "{case}");
            if status == STATUS_UNCERTAIN {
                assert!(
                    item.message.as_deref().unwrap().contains("check My Orders")
                        || item.message.as_deref().unwrap().contains("Check My Orders"),
                    "{case}: {:?}",
                    item.message
                );
            }
        }
    }

    /// What reconciliation shows did not land is sent by the same resume, after
    /// the usual checks; what it confirms or cannot explain is not sent.
    #[test]
    fn a_resume_sends_only_what_current_orders_show_was_not_applied() {
        let path = std::env::temp_dir().join(format!("reconcile-resume-{}.json", random_token(8)));
        let mut pending = uncertain_plan(vec![
            serde_json::json!({"slug":"plain", "reviewed_order":{"state":"new"}}),
            serde_json::json!({"slug":"bulk", "quantity":6, "per_trade":3, "reviewed_order":{"state":"new"}}),
            serde_json::json!({"slug":"plain", "order_type":"buy"}),
        ]);
        write_pending_atomic(&path, &pending).unwrap();
        let mut reads = 0;
        reconcile_pending(
            &path,
            &mut pending,
            &reconcile_catalog(),
            || {
                reads += 1;
                crate::orders::decode::decode_orders(
                    &serde_json::json!({"data":[{"id":"o9", "itemId":"id-bulk", "type":"sell",
                        "platinum":30, "quantity":6, "perTrade":3}]}),
                    &Unconstrained,
                )
            },
            write_pending_atomic,
        )
        .unwrap_or_else(|_| panic!("reconciliation could not be saved"));
        assert_eq!(reads, 1, "one orders read covers every unknown outcome");
        let mut sent = vec![];
        let response = run_pending_with(
            &path,
            &mut pending,
            |_| Ok(()),
            |item, validate| {
                sent.push((item.slug.clone(), item.order_type.clone()));
                send_after(item, validate)
            },
        );
        assert_eq!(sent, [("plain".to_string(), "sell".to_string())]);
        assert_eq!(
            response
                .results
                .iter()
                .map(|row| (row.status.as_str(), row.order_id.as_deref()))
                .collect::<Vec<_>>(),
            [
                (STATUS_OK, Some("remote-plain")),
                (STATUS_OK, Some("o9")),
                (STATUS_UNCERTAIN, None)
            ]
        );
        assert!(matches!(journal_state(&path), JournalState::Unfinished));
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_resume_with_nothing_unconfirmed_reads_no_orders_to_reconcile() {
        let mut pending: PendingPlan = serde_json::from_value(serde_json::json!({
            "plan_id":"clean", "started_at":"test", "items":[
                {"slug":"plain","quantity":1,"platinum":12,"order_type":"sell","visible":false,"status":"pending"}
            ]
        }))
        .unwrap();
        let result = reconcile_pending(
            std::path::Path::new("unused"),
            &mut pending,
            &reconcile_catalog(),
            || panic!("no unknown outcome, no read"),
            |_, _| panic!("nothing changed, nothing written"),
        );
        assert!(result.is_ok());
    }
}
