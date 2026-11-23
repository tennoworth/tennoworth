//! How the cheapest asks are spread says how a new listing will be picked.
//! When the 20 cheapest sit within 10% of the 5 cheapest, buyers face a queue
//! at one price and take the cheapest, so a listing above it waits. A slow
//! queue was the first idea, but the live book rejected it: of 36 crowded
//! books in the 2026-11-17 snapshot, one held more than half a day of trades.

pub const CROWDED_MIN_ASKS: f64 = 20.0;
pub const CROWDED_SPREAD: f64 = 0.1;

/// Whether a book is a queue at one price, from its `low5_avg`, `low20_avg`
/// and `sells` (each read as `Number(x) || 0`).
pub fn crowded_book(low5: f64, low20: f64, sells: f64) -> bool {
    // low20 is floored at low5 by the pipeline; a lower value is a snapshot
    // from before that fix, and says nothing about the queue.
    if low5 <= 0.0 || low20 < low5 || sells < CROWDED_MIN_ASKS {
        return false;
    }
    low20 <= low5 * (1.0 + CROWDED_SPREAD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tight_deep_book_is_a_queue() {
        assert!(crowded_book(69.0, 70.0, 50.0));
        assert!(crowded_book(50.0, 55.0, 20.0));
        assert!(crowded_book(50.0, 50.0 * 1.1, 20.0));
    }

    #[test]
    fn a_spread_or_shallow_book_is_not() {
        assert!(!crowded_book(50.0, 55.5, 40.0));
        assert!(!crowded_book(50.0, 50.0, 19.0));
    }

    #[test]
    fn missing_or_pre_floor_depth_says_nothing() {
        assert!(!crowded_book(50.0, 0.0, 40.0));
        assert!(!crowded_book(0.0, 0.0, 40.0));
        assert!(!crowded_book(26.4, 21.5, 22.0));
    }
}
