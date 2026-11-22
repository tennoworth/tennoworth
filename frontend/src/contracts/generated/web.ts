// Derived from Rust wire contracts. Run the binding export test to update.

export type PickRow = { sellable: number, patience: boolean,
/**
 * Timing says the price sits near its 90-day low.
 */
hold: boolean, score: number, };

export type PicksRequest = { rows: Array<PickRow>,
/**
 * Defaults to [`sell_priority::MAX_PICKS`].
 */
cap?: number, };

export type WebRequest = { "op": "select_picks", "input": PicksRequest };

export type WebResponse = { "op": "select_picks", "result": Array<number> };
