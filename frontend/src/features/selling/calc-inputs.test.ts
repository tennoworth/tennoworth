import { describe, expect, it } from 'vitest';
import {
  advisorInput,
  relicInput,
  scoreInput,
  setInput,
  type CalcInputs,
} from './calc-inputs';

/**
 * Each calculator in the shell is gated by a hand-written policy: it runs only
 * when its inputs exist, it clears when they vanish, and the filters it depends
 * on differ per calculator - which is why "spares only" recomputes one of them
 * and not the others. Those policies were inline in `$effect` bodies, so the
 * only way to observe them was to mount the shell.
 */

const owned = new Map([['primed_flow|', { slug: 'primed_flow' } as never]]);
const market = { items: {}, calendar: { primes: [1] } } as never;

function inputs(over: Partial<CalcInputs> = {}): CalcInputs {
  return {
    owned,
    previousOwned: null,
    availableOwned: owned,
    market,
    reserve: 2,
    available: new Map([['primed_flow|', 3]]),
    sparesOnly: false,
    advisorHistory: null,
    ...over,
  };
}

describe('scoreInput', () => {
  it('cannot run without inventory or a market', () => {
    expect(scoreInput(inputs(), 'default')).not.toBeNull();
    expect(scoreInput(inputs({ owned: new Map() }), 'default')).toBeNull();
    expect(scoreInput(inputs({ market: null }), 'default')).toBeNull();
  });

  // The previous inventory is what "since last scan" compares against, so it has
  // its own existence rule rather than following the current one.
  it('the previous-scan calculation has its own inventory to wait for', () => {
    expect(scoreInput(inputs(), 'previous')).toBeNull();
    expect(scoreInput(inputs({ previousOwned: owned }), 'previous')).not.toBeNull();
    // A market is still required alongside it.
    expect(scoreInput(inputs({ previousOwned: owned, market: null }), 'previous')).toBeNull();
  });

  // The reason `sparesOnly` exists as a separate calculation: toggling it must
  // not disturb the full set.
  it('spares-only is withheld until it is asked for', () => {
    expect(scoreInput(inputs(), 'spare')).toBeNull();
    expect(scoreInput(inputs({ sparesOnly: true }), 'spare')).not.toBeNull();
    // And the full set runs regardless of the toggle.
    expect(scoreInput(inputs({ sparesOnly: true }), 'default')).not.toBeNull();
  });

  it('carries the reserve the user set and the availability it was given', () => {
    const base = inputs({ reserve: 7 });
    const run = scoreInput(base, 'default')!;
    expect(run.reserve).toBe(7);
    expect(run.available).toBe(base.available);
  });
});

describe('advisorInput', () => {
  it('waits for inventory and a calendar to advise from', () => {
    expect(advisorInput(inputs())).not.toBeNull();
    expect(advisorInput(inputs({ owned: new Map() }))).toBeNull();
    // A market without a calendar cannot support advice, even with inventory.
    expect(advisorInput(inputs({ market: { items: {} } as never }))).toBeNull();
  });

  it('passes the history it was given, including none', () => {
    expect(advisorInput(inputs())!.history).toBeNull();
    const history = { years: [] } as never;
    expect(advisorInput(inputs({ advisorHistory: history }))!.history).toBe(history);
  });

  it('hands over one slug per owned record', () => {
    const two = new Map([
      ['a|', { slug: 'a' } as never],
      ['b|radiant', { slug: 'b' } as never],
    ]);
    expect(advisorInput(inputs({ owned: two }))!.slugs).toEqual(['a', 'b']);
  });
});

// Each of these reads a different part of the market, so a snapshot that
// carries prices but not the part they need must withhold that calculation
// rather than run it against missing data.
describe('setInput', () => {
  it('needs inventory and a snapshot with set recipes', () => {
    expect(setInput(inputs({ market: { set_to_parts: {} } as never }))).not.toBeNull();
    expect(setInput(inputs({ owned: new Map() }))).toBeNull();
    expect(setInput(inputs({ market: { items: {} } as never }))).toBeNull();
    expect(setInput(inputs({ market: null }))).toBeNull();
  });

  it('runs against the availability-adjusted inventory', () => {
    const base = inputs({ market: { set_to_parts: {} } as never });
    expect(setInput(base)!.owned).toBe(base.availableOwned);
  });
});

describe('relicInput', () => {
  it('needs inventory and a snapshot with relic rewards', () => {
    expect(relicInput(inputs({ market: { relic_rewards: {} } as never }))).not.toBeNull();
    expect(relicInput(inputs({ owned: new Map() }))).toBeNull();
    expect(relicInput(inputs({ market: { set_to_parts: {} } as never }))).toBeNull();
  });

  it('is the one that reads the raw inventory, not the availability-adjusted one', () => {
    expect(relicInput(inputs({ market: { relic_rewards: {} } as never }))!.owned).toBe(owned);
  });
});

// The shell hands these policies an object of getters over its reactive state,
// so the fields a policy reads are exactly what its `$effect` re-runs on. A field
// it does not need makes that calculation restart - and blank its result -
// whenever the field changes, which is how toggling "spares only" once discarded
// the default scoring the table falls back to.
describe('what each calculation depends on', () => {
  function reads(run: (inputs: CalcInputs) => unknown, over: Partial<CalcInputs> = {}): string[] {
    const seen = new Set<string>();
    const full = inputs({
      market: { items: {}, calendar: { primes: [1] }, set_to_parts: {}, relic_rewards: {} } as never,
      ...over,
    });
    run(new Proxy(full, {
      get(target, key, receiver) {
        seen.add(String(key));
        return Reflect.get(target, key, receiver);
      },
    }));
    return [...seen].sort();
  }

  it('reads only the fields its calculation uses', () => {
    expect(reads(i => scoreInput(i, 'default'))).toEqual(['available', 'market', 'owned', 'reserve']);
    expect(reads(i => scoreInput(i, 'spare'))).toEqual(['market', 'owned', 'sparesOnly']);
    expect(reads(i => scoreInput(i, 'previous'), { previousOwned: owned }))
      .toEqual(['market', 'previousOwned', 'reserve', 'sparesOnly']);
    expect(reads(advisorInput)).toEqual(['advisorHistory', 'market', 'owned']);
    expect(reads(setInput)).toEqual(['availableOwned', 'market']);
    expect(reads(relicInput)).toEqual(['market', 'owned']);
  });
});
