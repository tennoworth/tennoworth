//! Listing caps. One definition for every place that enforces them: the listing
//! plan and order edits in `wfm-core`, the desktop's own pre-checks, and trade
//! session selection here. `frontend/src/domain/limits.ts` keeps a copy so the
//! UI can refuse an out-of-range value before the round trip; the shared
//! `tests/fixtures/limits.json` keeps the two in step.

/// Items in one listing batch.
pub const MAX_PLAN_ITEMS: usize = 50;

/// Items per single in-game trade: six slots per side in Warframe's trade
/// window. WFM rejects a larger `perTrade` with `app.field.tooBig` (verified on
/// a real relic listing, May 2026).
pub const MAX_PER_TRADE: u32 = 6;

/// Lowest price a listing may ask, per unit.
pub const MIN_PLATINUM: u32 = 5;

/// Highest price, per unit and per lot. Matches WFM's own UI cap. It was 999
/// once, which silently blocked maxed Arcanes and Galvanized mods that really
/// sell for 1500-2500p.
pub const MAX_PLATINUM: u32 = 3000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_limits_match_the_shared_fixture() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/limits.json"))
                .expect("parse the limits fixture");
        assert_eq!(fixture["max_plan_items"], MAX_PLAN_ITEMS);
        assert_eq!(fixture["max_per_trade"], MAX_PER_TRADE);
        assert_eq!(fixture["min_platinum"], MIN_PLATINUM);
        assert_eq!(fixture["max_platinum"], MAX_PLATINUM);
    }
}
