//! Vendor-neutral contract for one of the account's own market orders.
//!
//! The market API has shipped more than one row shape, and it states a row's
//! side twice - once as the bucket a response files the row under, once as a
//! field on the row. A row is therefore not a bag of optional fields: it is
//! either understood completely, understood not to be supported, or not
//! understood at all. [`OrderRow`] makes those three states distinct so a
//! consumer cannot accidentally read a half-decoded row as a normal one.

use serde::{Deserialize, Serialize};

/// Which side of the market an order sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum OrderSide {
    Sell,
    Buy,
}

impl OrderSide {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sell => "sell",
            Self::Buy => "buy",
        }
    }
}

/// A row the decoder understood in full. Every field a consumer acts on is
/// present and already validated, so readers do not repeat the checks and
/// cannot disagree about what a missing field meant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedOrder {
    pub id: String,
    pub item_id: String,
    pub side: OrderSide,
    pub platinum: u64,
    pub quantity: u64,
    pub per_trade: Option<u64>,
    pub visible: Option<bool>,
    pub rank: Option<u64>,
    pub subtype: Option<String>,
}

/// The identity the market enforces uniqueness on: one order per item, side,
/// rank and variant. Two live orders sharing a key is not a state the market
/// should allow, which is why a consumer finding two has to treat the key as
/// unusable rather than pick one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct OrderKey {
    pub item_id: String,
    pub side: OrderSide,
    pub rank: Option<u64>,
    pub subtype: Option<String>,
}

impl NormalizedOrder {
    pub fn key(&self) -> OrderKey {
        OrderKey {
            item_id: self.item_id.clone(),
            side: self.side,
            rank: self.rank,
            subtype: self.subtype.clone(),
        }
    }
}

/// A row kept whole so a surface can still show - and act on - what the market
/// returned even when this build cannot interpret it. Dropping these rows would
/// hide live orders from the account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefusedRow {
    pub evidence: serde_json::Value,
    pub reason: String,
}

/// One row of an orders response, with the three outcomes kept apart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum OrderRow {
    Supported(NormalizedOrder),
    Unsupported(RefusedRow),
    Ambiguous(RefusedRow),
}

/// The item metadata a row's rank and subtype have to agree with. Implemented
/// by the caller's catalogue so the decoder stays free of any one catalogue
/// type, and asked by the item reference the response carries.
///
/// A reference the catalogue does not know constrains nothing: the catalogue is
/// built from a snapshot that can lag the live market, and refusing an order
/// over a missing snapshot entry would hide it from the account.
pub trait ItemConstraints {
    /// Whether `rank` and `subtype` are legal for the referenced item,
    /// including the presence rules - a ranked item needs a rank, an unranked
    /// one must not carry one, and a variant item needs a subtype from its own
    /// list.
    fn accepts(&self, item_id: &str, rank: Option<u64>, subtype: Option<&str>) -> bool;
}
