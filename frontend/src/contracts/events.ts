


/** Progress event the desktop emits per item during `desktopLiveTopPrices`. */
export { LIVE_TOP_PROGRESS_EVENT } from './generated/desktop';



/** Rust emits this (a WatchOutcome) when a background pass notifies. */
export { WATCH_FIRED_EVENT } from './generated/desktop';



/**
 * Rust emits this after a scan the app started on its own - the automatic
 * scanner, or the tray's Rescan - carrying the same payload the
 * `scan_inventory` command returns. Generated from the Rust const, so both
 * halves of the channel name are pinned by the binding export test.
 */
export { INVENTORY_SCANNED_EVENT } from './generated/desktop';



/**
 * The remaining Rust-emitted channels, each generated from its Rust const so a
 * rename on either side fails the binding export test: a trade EE.log
 * confirmed, trade recording starting or stopping, the trade allowance, the
 * notification inbox, and a market refresh.
 */
export {
  TRADE_DETECTED_EVENT,
  RECORDING_CHANGED_EVENT,
  ALLOWANCE_CHANGED_EVENT,
  NOTIFICATIONS_EVENT,
  MARKET_REFRESHED_EVENT,
} from './generated/desktop';

/** The relic overlay window's result and hide pushes. */
export { RELIC_OVERLAY_UPDATE_EVENT, RELIC_OVERLAY_HIDE_EVENT } from './generated/desktop';
