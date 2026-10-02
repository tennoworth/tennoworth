//! Seeing the game: capturing its window and reading text off the frame.
//!
//! A capability, not a feature. In-game features (the reward overlay today)
//! decide when to look and what the text means; this layer only captures,
//! crops and recognizes, and imports none of them.

pub(crate) mod capture;
pub(crate) mod frame;
pub(crate) mod ocr;
