// Derived from Rust wire contracts. Run the binding export test to update.

export type AccessStatus = { revision: number, reason: string, cooldown_until_ms: number, queue_count: number, outstanding: number, restrictions: Restrictions, requests: number, throttles: number, cache_hits: number, cache_misses: number, queue_rejections: number, };

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

export type CmdError = { code: string, message: string, };

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

export const AUTO_SCAN_CADENCE_CHOICES = [15, 30, 60] as const;
