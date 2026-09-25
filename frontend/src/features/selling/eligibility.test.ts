import { describe, expect, it } from 'vitest';
import {
  listingBlockReason,
  listingActionLabel,
  protectedView,
  unknownSlugs,
  type EligibilityInputs,
} from './eligibility';

/** Inputs for an inventory that is ready to list. */
function ready(over: Partial<EligibilityInputs> = {}): EligibilityInputs {
  return {
    hasSnapshot: true,
    pullingInventory: false,
    allocationMatches: true,
    hasProtection: true,
    protectionSnapshotId: 7,
    nativeSnapshotId: 7,
    protectionError: null,
    supportedSlugs: ['primed_flow'],
    known: ['primed_flow'],
    ...over,
  };
}

describe('listingBlockReason', () => {
  it('lets a ready inventory through', () => {
    expect(listingBlockReason(ready())).toBeNull();
  });

  // The order matters: each message tells the user a different next action, so
  // when two conditions hold the more fundamental one has to win. These are the
  // precedence rules the shell encoded inline and nothing could observe.
  it('reports the most fundamental blocker first', () => {
    // No snapshot outranks everything - nothing else can be evaluated.
    expect(
      listingBlockReason(
        ready({ hasSnapshot: false, pullingInventory: true, allocationMatches: false }),
      ),
    ).toMatch(/Scan the game/);

    // A scan in progress outranks a protection problem: the protection read is
    // about to be replaced.
    expect(listingBlockReason(ready({ pullingInventory: true, allocationMatches: false }))).toMatch(
      /scan is in progress/,
    );

    // Missing protection outranks a stale snapshot id.
    expect(
      listingBlockReason(
        ready({ hasProtection: false, protectionSnapshotId: 1, nativeSnapshotId: 7 }),
      ),
    ).toMatch(/protection is unavailable/);

    // A stale protection snapshot outranks unknown items.
    expect(
      listingBlockReason(
        ready({
          protectionSnapshotId: 1,
          supportedSlugs: ['primed_flow', 'arcane'],
          known: ['primed_flow'],
        }),
      ),
    ).toMatch(/does not match the latest game scan/);

    // A failed protection read outranks an unknown quantity.
    expect(
      listingBlockReason(ready({ protectionError: 'ipc down', supportedSlugs: ['a', 'b'], known: ['a'] })),
    ).toMatch(/unavailable for some items/);
  });

  it('distinguishes an allocation mismatch from a stale snapshot', () => {
    // Mismatched allocation means the protection numbers describe a different
    // inventory than the one on screen.
    expect(listingBlockReason(ready({ allocationMatches: false }))).toMatch(/protection is unavailable/);
    // Matching allocation with a different snapshot id is its own message.
    expect(listingBlockReason(ready({ protectionSnapshotId: 1 }))).toMatch(/latest game scan/);
  });

  // An item the allocation cannot place is a blocker even when everything else
  // is healthy: listing it would guess at a protected quantity.
  it('blocks when any supported item has no usable quantity', () => {
    const p = protectedView({ supportedSlugs: ['primed_flow', 'arcane'], known: ['primed_flow'] });
    expect(unknownSlugs(p)).toEqual(new Set(['arcane']));
    expect(listingBlockReason(ready({ supportedSlugs: ['primed_flow', 'arcane'], known: ['primed_flow'] }))).toMatch(
      /unavailable for some items/,
    );
  });

  it('has nothing to say about an empty inventory', () => {
    const p = protectedView({ supportedSlugs: [], known: [] });
    expect(unknownSlugs(p)).toEqual(new Set());
    expect(listingBlockReason(ready({ supportedSlugs: [], known: [] }))).toBeNull();
  });
});

describe('listingActionLabel', () => {
  it('names the action that unblocks the gate', () => {
    expect(listingActionLabel({ hasSnapshot: false } as never)).toBe('Scan game');
    expect(listingActionLabel(ready())).toBe('Check WFM listings');
    // No protection read yet, or one that cannot be reconciled, means recheck.
    expect(listingActionLabel(ready({ allocationMatches: false }))).toBe('Recheck protection');
    expect(listingActionLabel(ready({ hasProtection: false }))).toBe('Recheck protection');
    // A different snapshot means the scan is what is out of date.
    expect(listingActionLabel(ready({ protectionSnapshotId: 1 }))).toBe('Scan game');
    expect(listingActionLabel(ready({ protectionError: 'x' }))).toBe('Recheck protection');
    expect(listingActionLabel(ready({ supportedSlugs: ['a'], known: [] }))).toBe(
      'Recheck protection',
    );
    // Nothing supported means nothing to check, not a blocked gate.
    expect(listingActionLabel(ready({ supportedSlugs: [], known: [] }))).toBe('Check WFM listings');
  });
});
