import type { ProtectionInventory, ProtectionState } from '../contracts/protection';
export function sampleAllocation(owned: number, unavailable: number, globalKeep: number, reserved: number, listed: number | null) {
  const protectedCount = Math.max(globalKeep, unavailable + reserved);
  return { owned, protected: protectedCount, estimated: Math.max(0, owned - protectedCount), listed,
    available: listed == null ? null : Math.max(0, owned - protectedCount - listed) };
}

export function sampleGuidance(native: ProtectionState, inventory: ProtectionInventory, globalKeep: number, required: Record<string, number>, source = 'memory'): ProtectionState {
  const nativeRows = Object.entries(native.items);
  const verified = source === 'memory' && inventory.snapshot_id != null && inventory.snapshot_id === native.snapshot_id
    && Object.keys(inventory.items).length === nativeRows.length
    && nativeRows.every(([slug, row]) => inventory.items[slug]?.count === row.owned);
  if (verified) return native;
  return { ...native, snapshot_id: null, items: Object.fromEntries(Object.entries(inventory.items).map(([slug, row]) =>
    [slug, sampleAllocation(row.count, row.leveled, globalKeep, (native.plan.reserves[slug] ?? 0) + (required[slug] ?? 0), null)])) };
}
