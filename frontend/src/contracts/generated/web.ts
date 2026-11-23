// Derived from Rust wire contracts. Run the binding export test to update.

export type AlertReward = { item: string, count: number, };

export type Boost = "resource-amount" | "resource-chance" | "affinity" | "credits" | "other";

export type CalendarHolding = { slug: string, count?: number | null, };

export type CalendarRequest = {
/**
 * The snapshot's `baro`, `calendar`, `set_to_parts`, `de`,
 * `event_rewards` and `surface_provenance`.
 */
market: unknown,
/**
 * The curated announcement feed, already validated by the page's
 * reader; only the month and lane calendar shows it.
 */
feed?: unknown,
/**
 * Absent before an inventory is loaded, which is not an empty one.
 */
owned?: Array<CalendarHolding> | null, now: number, };

export type DatedEvent = {
/**
 * Stable across refreshes of the same snapshot, so a selection survives.
 */
id: string, kind: DatedKind,
/**
 * Epoch ms.
 */
from: number,
/**
 * Epoch ms; none for a single moment such as a deadline.
 */
to: number | null, text: DatedText, estimate: boolean, source: DatedSource, sourceUrl?: string,
/**
 * Held slugs this event touches; absent when that cannot be known.
 */
affects?: Array<string>, };

export type DatedKind = "official" | "event" | "baro" | "resurgence" | "vault" | "notice" | "deal" | "nightwave";

export type DatedSource = "worldstate" | "estimate" | "notice" | "official";

export type DatedText = { "kind": "baro", where: string | null, } | { "kind": "baro-estimate" } | { "kind": "resurgence", sets: Array<string>, } | { "kind": "vault", sets: Array<string>, } | { "kind": "deal", item: string, discount: number | null, price: number | null, } | { "kind": "event", title: string, rewards: Array<string>, } | { "kind": "alert", rewards: Array<AlertReward>, credits: number | null, } | { "kind": "booster", boost: Boost, factor: number | null, } | { "kind": "nightwave", intermission: boolean, } | { "kind": "official", title: string, note: string | null, type: string, } | { "kind": "given", title: string, detail: string, };

export type FeedCoverage = "complete" | "partial" | "unknown";

export type FeedItem = {
/**
 * Stable upstream id where one exists.
 */
id?: string, kind: FeedKind,
/**
 * When it starts, as the snapshot gives it.
 */
at: string,
/**
 * When it ends, where there is an end.
 */
until?: string, text: FeedText,
/**
 * Held slugs this row affects.
 */
affects: Array<string>,
/**
 * False when the row's contents cannot be resolved to slugs, so an empty
 * `affects` must not read as "this doesn't touch you".
 */
affectsKnown: boolean, reach?: FeedReach, stale?: boolean,
/**
 * Whole days since the row's surface was observed; absent when unknown.
 */
dataAgeDays?: number, };

export type FeedKind = "baro" | "vault" | "deal" | "event";

export type FeedReach = "scan" | "none" | "hits" | "partial-hits" | "unknown";

export type FeedText = { "kind": "baro", where: string | null, stock: number, } | { "kind": "vault", packs: number, } | { "kind": "deal", item: string, discount: number | null, price: number | null, } | { "kind": "event", title: string, rewards: number, credits: number, coverage: FeedCoverage, };

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

export type WebRequest = { "op": "select_picks", "input": PicksRequest } | { "op": "calendar_feed", "input": CalendarRequest } | { "op": "calendar_events", "input": CalendarRequest };

export type WebResponse = { "op": "select_picks", "result": Array<number> } | { "op": "calendar_feed", "result": Array<FeedItem> } | { "op": "calendar_events", "result": Array<DatedEvent> };
