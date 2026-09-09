//! Upstream ingestion with a shared fixture-capable HTTP boundary.

mod baro;
mod calendar;
mod catalog;
mod relics;
mod rivens;
pub(crate) mod transport;
pub use baro::{carry_baro_inventory, fetch_baro};
pub use calendar::{
    fetch_calendar, fetch_vault_status, frames_in_pack_name, resurgence_rotations,
    WFSTAT_VAULT_TRADER_URL,
};
pub use catalog::{
    fetch_catalog_wfm, fetch_parent_data, fetch_wfstat_raw, fetch_wfstat_slim, slim_wfstat_items,
    CatalogFetch, WFSTAT_ITEMS_URL,
};
pub use relics::fetch_relic_rewards;
pub use rivens::{
    carry_failed_riven_platforms, fetch_riven_stats, fetch_rivens, riven_change_log,
    RivenChildOutcome, WeeklyRivenRow, RIVEN_CHANGE_RETENTION_DAYS, WFM_RIVEN_ATTRIBUTES_URL,
    WFM_RIVEN_WEAPONS_URL,
};
pub use transport::{FixtureHttp, Http, LiveHttp};
#[cfg(test)]
mod tests;
