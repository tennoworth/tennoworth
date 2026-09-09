// Derived from Rust wire contracts. Run the binding export test to update.

export type AccessStatus = { revision: number, reason: string, cooldown_until_ms: number, queue_count: number, outstanding: number, restrictions: Restrictions, requests: number, throttles: number, cache_hits: number, cache_misses: number, queue_rejections: number, };

export type CmdError = { code: string, message: string, };

export type Restrictions = { spacing_ms: number, concurrency: number, contract_spacing_ms: number, watch_interval_ms: number, pause_all: boolean, pause_background: boolean, pause_contracts: boolean, pause_mutations: boolean, pause_websockets: boolean, };

export type ScanReport = { url: string, opened: boolean, };

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
