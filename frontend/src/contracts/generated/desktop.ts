// Derived from Rust wire contracts. Run the binding export test to update.

export type AccessStatus = { revision: number, reason: string, cooldown_until_ms: number, queue_count: number, outstanding: number, restrictions: Restrictions, requests: number, throttles: number, cache_hits: number, cache_misses: number, queue_rejections: number, };

export type AppIconOutcome = {
/**
 * False on a GTK Wayland session: Wayland compositors take the window and
 * taskbar icon from the installed launcher entry, and GTK 3 has no way to
 * set one per window there. The tray still changes.
 */
windowIcon: boolean, };

export type AutoScanSettings = { enabled: boolean,
/**
 * Minutes between attempts while the game is running.
 */
cadenceMinutes: number,
/**
 * Whether a background scan that finishes while the app is open replaces
 * the inventory on screen. Off means the app offers it instead.
 */
adoptAutomatically: boolean, };

export type AutoScanStatus = { enabled: boolean, cadenceMinutes: number, adoptAutomatically: boolean,
/**
 * An interactive listing flow is open, so scanning is suspended.
 */
held: boolean, gameRunning: boolean,
/**
 * Unix seconds of the last successful automatic scan.
 */
lastScanAt: number | null,
/**
 * The last failure, redacted; cleared by a success or a settings change.
 */
lastError: string | null,
/**
 * Unix seconds of the next scheduled attempt, when one is scheduled.
 */
nextCheckAt: number | null, };

export type BaroNotification = { location: string, activation: string, expiry: string, stock_count: number | null, value: BaroNotificationValue | null, held: Array<string>, };

export type BaroNotificationValue = { best: Array<BaroPick>, more_tradeable: number, price_at: string, prices_stale: boolean, fodder_ducats: number | null, fodder_items: number | null, cheap_fodder: number | null, };

export type BaroPick = { name: string, platPerDucat: number, };

export type CmdError = { code: string, message: string, };

export type DigestNotification = { opportunities: Array<DigestOpportunity>, inventory_at: string, price_at: string, };

export type DigestOpportunity = { name: string, quantity: number, price: number, };

export type NotificationContent = { "kind": "baro" } & BaroNotification | { "kind": "digest" } & DigestNotification;

export type OrderSide = "sell" | "buy";

export type OwnOrder = { id: string, item_id: string, slug: string | null, name: string | null, side: OrderSide, platinum: number, quantity: number, per_trade: number | null, visible: boolean | null, rank: number | null, subtype: string | null, };

export type Restrictions = { spacing_ms: number, concurrency: number, contract_spacing_ms: number, watch_interval_ms: number, pause_all: boolean, pause_background: boolean, pause_contracts: boolean, pause_mutations: boolean, pause_websockets: boolean, };

export type WatchOutcome = { id: number, slug: string, name: string, side: string, threshold: number,
/**
 * The price the watch is judged against (lowest other ask for 'sell',
 * highest other bid for 'buy'); `None` when the book was empty or the
 * lookup failed.
 */
price: number | null,
/**
 * Condition met right now.
 */
satisfied: boolean,
/**
 * Condition met AND not notified within [`REARM_AFTER_SECS`] → notify.
 */
fire: boolean, };

export const WATCH_FIRED_EVENT = "watch-fired" as const;

export const WFM_ACCESS_EVENT = "wfm-access-changed" as const;

export const INVENTORY_SCANNED_EVENT = "inventory-scanned" as const;

export const TRADE_DETECTED_EVENT = "trade-detected" as const;

export const RECORDING_CHANGED_EVENT = "recording-changed" as const;

export const ALLOWANCE_CHANGED_EVENT = "trade-allowance-changed" as const;

export const NOTIFICATIONS_EVENT = "notifications-changed" as const;

export const MARKET_REFRESHED_EVENT = "market-refreshed" as const;

export const LIVE_TOP_PROGRESS_EVENT = "live-top-progress" as const;

export const TRAY_HINT_EVENT = "tray-hint" as const;

export const UPDATE_AVAILABLE_EVENT = "update-available" as const;

export const RELIC_OVERLAY_UPDATE_EVENT = "relic-overlay:update" as const;

export const RELIC_OVERLAY_HIDE_EVENT = "relic-overlay:hide" as const;

export const RELIC_RECOMMENDATION_CONFIDENCE = 0.9 as const;

export const AUTO_SCAN_CADENCE_CHOICES = [15, 30, 60] as const;

export const APP_ICON_COLOURS = ["blue", "ink", "rag"] as const;
