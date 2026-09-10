export function sampleAllocation(owned: number, unavailable: number, globalKeep: number, reserved: number, listed: number | null) {
  const protectedCount = Math.max(globalKeep, unavailable + reserved);
  return { owned, protected: protectedCount, estimated: Math.max(0, owned - protectedCount), listed,
    available: listed == null ? null : Math.max(0, owned - protectedCount - listed) };
}
