//! The opt-in collectors that run on the host: the daily installation count
//! (`usage`) and anonymous price and riven-shape reports (`reports`). They share
//! the request quota and serve loop in `gate`, and nothing else: each binary is
//! its own process with its own database and quota, so neither can starve the
//! other.

pub mod gate;
pub mod reports;
pub mod usage;
