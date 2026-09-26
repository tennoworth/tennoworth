/**
 * When a listing batch may be sent, and what the button should offer instead.
 *
 * These rules lived inline in `DesktopShell.svelte` as a chain of `$derived`
 * closures, which made the decision untestable: observing it required mounting a
 * ~1600-line component. They are the app's listing gate, so a wrong precedence
 * either blocks a user who could list or lets them post with a quantity nobody
 * verified - both worth a table test.
 *
 * The inputs are computed by the shell from its controllers; nothing here reads
 * state, so the rules can be pinned directly.
 */

/** The protection view of the items this app can list. */
export interface ProtectedView {
  /** Slugs of supported items, i.e. the ones a listing can be built for. */
  supportedSlugs: readonly string[];
  /** Slugs the allocation could place a quantity for. Others are unknown. */
  known: readonly string[];
}

export interface EligibilityInputs extends ProtectedView {
  /** A game scan produced the displayed inventory. */
  hasSnapshot: boolean;
  /** A scan is currently running. */
  pullingInventory: boolean;
  /** The protection allocation describes the displayed inventory. */
  allocationMatches: boolean;
  /** A protection state exists at all. */
  hasProtection: boolean;
  /** Snapshot the protection state was computed against. */
  protectionSnapshotId: number | null;
  /** Snapshot currently displayed. */
  nativeSnapshotId: number | null;
  /** A protection read failed, so its numbers cannot be trusted. */
  protectionError: string | null;
}

/** Items whose protected quantity could not be established. */
export function unknownSlugs(view: ProtectedView): Set<string> {
  const known = new Set(view.known);
  return new Set(view.supportedSlugs.filter((slug) => !known.has(slug)));
}

/**
 * Why a listing batch cannot be sent, or null when it can.
 *
 * Ordered by how fundamental the problem is, because each message names a
 * different next action and only one can be shown. A scan in progress outranks a
 * protection problem because the protection read is about to be replaced; a
 * missing allocation outranks a stale snapshot id because there is nothing to
 * compare.
 */
export function listingBlockReason(inputs: EligibilityInputs): string | null {
  if (!inputs.hasSnapshot) {
    return 'Scan the game to verify this inventory before listing. Imported backups provide estimates only.';
  }
  if (inputs.pullingInventory) {
    return 'A scan is in progress. Your review edits are kept.';
  }
  if (!inputs.allocationMatches || !inputs.hasProtection) {
    return 'Inventory protection is unavailable. Recheck quantities before listing.';
  }
  if (inputs.protectionSnapshotId !== inputs.nativeSnapshotId) {
    return 'The displayed inventory does not match the latest game scan. Scan again before listing.';
  }
  if (inputs.protectionError || unknownSlugs(inputs).size) {
    return 'Protection quantities are unavailable for some items. Recheck before listing.';
  }
  return null;
}

/** What the listing button offers, which is also how the user clears the gate. */
export function listingActionLabel(inputs: EligibilityInputs): string {
  if (!inputs.hasSnapshot) return 'Scan game';
  if (!inputs.allocationMatches || !inputs.hasProtection) return 'Recheck protection';
  if (inputs.protectionSnapshotId !== inputs.nativeSnapshotId) return 'Scan game';
  return inputs.protectionError || unknownSlugs(inputs).size
    ? 'Recheck protection'
    : 'Check WFM listings';
}

