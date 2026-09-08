// Derived from Rust wire contracts. Run the binding export test to update.

export type CmdError = { code: string, message: string, };

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
