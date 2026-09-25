import type { History } from '../../domain/history';
import type { Verdict } from '../../domain/advisor';
import type { RelicPlanEntry } from '../../domain/relic-planner';
import type { SetReco } from '../../domain/set-recos';
import type { ScoredInventoryFact } from '../../contracts/generated/domain';
import type { Market, OwnedRecord } from '../../contracts/data';

/**
 * What each Sell-view calculation needs before it can run.
 *
 * The shell runs six of these side by side, and their gates are not alike: one
 * reads the raw inventory, one the availability-adjusted copy, one needs a
 * calendar, another needs set recipes, a third relic rewards. While those rules
 * lived inline in `$effect` bodies, the only way to learn whether a snapshot
 * carried enough for a calculation was to mount the shell and watch.
 *
 * A policy returns what to run, or null to clear - so "the inputs went away" is
 * the same code path as "they were never there", which is what the shell did in
 * both cases.
 *
 * One thing deliberately left with the shell: each effect also reads
 * `calculationEpoch`, the value that makes a user-requested recompute re-fire it.
 * That is Svelte reactivity, not an input to the calculation, so it stays where
 * the effect is.
 */

/** One owned-item record, keyed by its identifying key. */
type Owned = Map<string, OwnedRecord>;

export interface CalcInputs {
  /** The current inventory, as resolved from the last scan. */
  owned: Owned;
  /** The inventory a previous scan produced, for the change comparison. */
  previousOwned: Owned | null;
  /** The inventory with protected copies removed, used where a plan is built. */
  availableOwned: Owned;
  market: Market | null;
  /** Copies the user asked to keep back from every calculation. */
  reserve: number;
  /** Per-key quantities that may actually be listed. */
  available: ReadonlyMap<string, number>;
  /** Whether the table is filtered to spares. */
  sparesOnly: boolean;
  /** Loaded on demand; advice degrades to calendar-only rules without it. */
  advisorHistory: History | null;
}

export interface ScoreRun {
  owned: Owned;
  market: Market;
  reserve: number;
  sparesOnly: boolean;
  available?: ReadonlyMap<string, number>;
}

/** Which of the three inventory scorings is being asked for. */
export type ScoreKind = 'default' | 'spare' | 'previous';

/**
 * Scoring never needs a calendar or a plan's worth of metadata - only items and
 * a market - which is why it can start the moment a scan lands.
 */
export function scoreInput(inputs: CalcInputs, kind: ScoreKind): ScoreRun | null {
  if (!inputs.market) return null;
  const market = inputs.market;

  if (kind === 'previous') {
    const previous = inputs.previousOwned;
    if (!previous?.size) return null;
    // No availability map: a previous scan has no allowance to apply, and the
    // comparison is about what changed, not what may be listed.
    return { owned: previous, market, reserve: inputs.reserve, sparesOnly: inputs.sparesOnly };
  }

  if (!inputs.owned.size) return null;
  if (kind === 'spare') {
    // Withheld until asked for, so toggling the filter cannot disturb the full
    // set the table is already showing.
    if (!inputs.sparesOnly) return null;
    return {
      owned: inputs.owned,
      market,
      reserve: inputs.reserve,
      sparesOnly: true,
      available: inputs.available,
    };
  }
  return {
    owned: inputs.owned,
    market,
    reserve: inputs.reserve,
    sparesOnly: false,
    available: inputs.available,
  };
}

export interface AdvisorRun {
  slugs: string[];
  market: Market;
  history: History | null;
}

/** Advice additionally needs a calendar to reason about; without one it cannot
 *  be given, even with a full inventory. */
export function advisorInput(inputs: CalcInputs): AdvisorRun | null {
  if (!inputs.owned.size) return null;
  const market = inputs.market;
  if (!market?.calendar?.primes) return null;
  return {
    slugs: [...inputs.owned.values()].map((row) => row.slug),
    market,
    history: inputs.advisorHistory,
  };
}

export interface SetRun {
  owned: Owned;
  market: Market;
}

/** Part sets are built from what may be listed, so they read the
 *  availability-adjusted inventory rather than the raw one. */
export function setInput(inputs: CalcInputs): SetRun | null {
  if (!inputs.availableOwned.size) return null;
  const market = inputs.market;
  if (!market?.set_to_parts) return null;
  return { owned: inputs.availableOwned, market };
}

export interface RelicRun {
  owned: Owned;
  market: Market;
}

/** The relic planner ranks what the player holds, so it reads the raw
 *  inventory - a relic reserved from listing is still worth cracking. */
export function relicInput(inputs: CalcInputs): RelicRun | null {
  if (!inputs.owned.size) return null;
  const market = inputs.market;
  if (!market?.relic_rewards) return null;
  return { owned: inputs.owned, market };
}

/** The shapes a caller hands back to the shell's `DomainResult`s. */
export type ScoreFacts = Map<string, ScoredInventoryFact>;
export type AdvisorMap = Map<string, Verdict>;
export type SetRecos = SetReco[];
export type RelicPlan = RelicPlanEntry[];
